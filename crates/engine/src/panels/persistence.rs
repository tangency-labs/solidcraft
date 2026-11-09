use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::*;

pub const ENVELOPE_FORMAT: &str = "solidcraft.panel-layout";
pub const ENVELOPE_VERSION: u32 = 2;

/// Unknown panel wire values are retained verbatim as JSON values, including
/// fields a future content adapter understands. Total encoded envelope is capped.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct QuarantinedPanel {
    pub instance: InstanceId,
    pub kind: PanelKind,
    pub instance_data: serde_json::Value,
    pub section: Option<SectionId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SectionSnapshot {
    pub data: serde_json::Value,
    /// Latest complete rank; also permits reading earlier v1 envelopes without
    /// the envelope's explicit complete section_order list.
    pub index: usize,
    /// Last materialized section, so a live user edit can be distinguished from
    /// the fallback active tab/order produced while some members are dormant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projected: Option<Section>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct LayoutEnvelope {
    pub format: String,
    pub version: u32,
    pub layout: Layout,
    pub quarantine: Vec<QuarantinedPanel>,
    /// Shared, lossless section order/activation/future fields, stored once even
    /// when many unknown tabs refer to the same section. Retained after restore.
    #[serde(default)]
    pub section_snapshots: std::collections::BTreeMap<SectionId, SectionSnapshot>,
    /// Latest complete section ordering, including currently dormant sections.
    /// Retained snapshots alone cannot rank surviving sections without this list.
    #[serde(default)]
    pub section_order: Vec<SectionId>,
    /// Original unknown wire data remains available after registry reappearance.
    /// A content/persistence adapter may explicitly consume it; restoring placement
    /// alone must not erase a future field that this foundation cannot interpret.
    #[serde(default)]
    pub retained_data: std::collections::BTreeMap<InstanceId, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extent_priority_overrides: Vec<ExtentPriorityRecord>,
    /// Bounded residual unknown keys, matched by record identity during encoding.
    /// Not serialized as a second copy beside the original extension fields.
    #[serde(skip)]
    pub recovery_wire: Option<serde_json::Value>,
    /// Provenance of an admitted bounded v1 record, even when its expanded v2
    /// representation cannot be staged. Never used to bypass write validation.
    #[serde(skip)]
    retained_v1: Option<std::sync::Arc<[u8]>>,
}
impl PartialEq for LayoutEnvelope {
    fn eq(&self, other: &Self) -> bool {
        // Original source bytes are provenance, not current layout semantics.
        self.format == other.format
            && self.version == other.version
            && self.layout == other.layout
            && self.quarantine == other.quarantine
            && self.section_snapshots == other.section_snapshots
            && self.section_order == other.section_order
            && self.retained_data == other.retained_data
            && self.recovery_wire == other.recovery_wire
            && self.extent_priority_overrides == other.extent_priority_overrides
    }
}
impl LayoutEnvelope {
    pub fn new(layout: Layout) -> Self {
        let section_order = layout.sections.iter().map(|section| section.id.clone()).collect();
        Self {
            format: ENVELOPE_FORMAT.into(),
            version: ENVELOPE_VERSION,
            layout,
            quarantine: Vec::new(),
            section_snapshots: std::collections::BTreeMap::new(),
            section_order,
            retained_data: std::collections::BTreeMap::new(),
            extent_priority_overrides: Vec::new(),
            recovery_wire: None,
            retained_v1: None,
        }
    }
    pub fn retained_v1_bytes(&self) -> Option<&[u8]> {
        self.retained_v1.as_deref()
    }
}

fn raw_priority_records(envelope: &LayoutEnvelope) -> PanelResult<ExtentPriorityContext> {
    if envelope.quarantine.len() > MAX_INSTANCES
        || envelope.section_snapshots.len() > MAX_SECTIONS
        || envelope.layout.instances.len() > MAX_INSTANCES
        || envelope.layout.sections.len() > MAX_SECTIONS
    {
        return Err(error(Rule::CollectionLimit, "priority source collection cap"));
    }
    let mut records = std::collections::BTreeMap::new();
    let mut add = |target: ExtentPriorityTarget,
                   location: &SectionLocation,
                   column: &Option<ColumnId>,
                   extent: Option<f32>,
                   priority: u16|
     -> PanelResult<()> {
        validate_extent_priority(extent, priority)?;
        if extent.is_some() {
            records.entry(target.clone()).or_insert(ExtentPriorityRecord {
                target,
                zone: location.zone(),
                column: column.clone(),
                stored_priority: priority,
                priority,
            });
        }
        Ok(())
    };
    for (id, snapshot) in &envelope.section_snapshots {
        let section: Section = serde_json::from_value(snapshot.data.clone()).map_err(|_| error(Rule::Schema, "invalid priority snapshot"))?;
        add(ExtentPriorityTarget::Section(id.clone()), &section.location, &section.column, section.preferred_extent, section.extent_priority)?;
        if let SectionLocation::Window { return_to, .. } = &section.location {
            add(
                ExtentPriorityTarget::SectionReturn(id.clone()),
                &SectionLocation::Dock { zone: return_to.zone },
                &return_to.column,
                return_to.preferred_extent,
                return_to.extent_priority,
            )?;
        }
    }
    for section in &envelope.layout.sections {
        add(
            ExtentPriorityTarget::Section(section.id.clone()),
            &section.location,
            &section.column,
            section.preferred_extent,
            section.extent_priority,
        )?;
        if let SectionLocation::Window { return_to, .. } = &section.location {
            add(
                ExtentPriorityTarget::SectionReturn(section.id.clone()),
                &SectionLocation::Dock { zone: return_to.zone },
                &return_to.column,
                return_to.preferred_extent,
                return_to.extent_priority,
            )?;
        }
    }
    for panel in &envelope.layout.instances {
        add(
            ExtentPriorityTarget::Memory(panel.id.clone()),
            &panel.memory.location,
            &panel.memory.column,
            panel.memory.preferred_extent,
            panel.memory.extent_priority,
        )?;
        if let SectionLocation::Window { return_to, .. } = &panel.memory.location {
            add(
                ExtentPriorityTarget::MemoryReturn(panel.id.clone()),
                &SectionLocation::Dock { zone: return_to.zone },
                &return_to.column,
                return_to.preferred_extent,
                return_to.extent_priority,
            )?;
        }
    }
    for item in &envelope.quarantine {
        let panel: PanelInstance =
            serde_json::from_value(item.instance_data.clone()).map_err(|_| error(Rule::Schema, "invalid priority quarantine"))?;
        add(
            ExtentPriorityTarget::Memory(panel.id.clone()),
            &panel.memory.location,
            &panel.memory.column,
            panel.memory.preferred_extent,
            panel.memory.extent_priority,
        )?;
        if let SectionLocation::Window { return_to, .. } = &panel.memory.location {
            add(
                ExtentPriorityTarget::MemoryReturn(panel.id.clone()),
                &SectionLocation::Dock { zone: return_to.zone },
                &return_to.column,
                return_to.preferred_extent,
                return_to.extent_priority,
            )?;
        }
    }
    let context = ExtentPriorityContext { records: records.into_values().collect(), complete: true };
    context.validate()?;
    Ok(context)
}

pub(super) fn typed_priority(layout: &Layout, target: &ExtentPriorityTarget) -> Option<u16> {
    match target {
        ExtentPriorityTarget::Section(id) => layout.section(id).ok().map(|s| s.extent_priority),
        ExtentPriorityTarget::Memory(id) => layout.instance(id).ok().map(|p| p.memory.extent_priority),
        ExtentPriorityTarget::SectionReturn(id) => layout.section(id).ok().and_then(|s| match &s.location {
            SectionLocation::Window { return_to, .. } => Some(return_to.extent_priority),
            _ => None,
        }),
        ExtentPriorityTarget::MemoryReturn(id) => layout.instance(id).ok().and_then(|p| match &p.memory.location {
            SectionLocation::Window { return_to, .. } => Some(return_to.extent_priority),
            _ => None,
        }),
    }
}

pub(super) fn synchronize_priority_context(layout: &mut Layout) -> PanelResult<()> {
    layout.extent_priority_context.validate()?;
    let priorities: Vec<_> = layout.extent_priority_context.records.iter().map(|record| typed_priority(layout, &record.target)).collect();
    for (record, priority) in layout.extent_priority_context.records.iter_mut().zip(priorities) {
        if let Some(priority) = priority {
            record.priority = priority;
        }
    }
    Ok(())
}

fn apply_section_priorities(context: &ExtentPriorityContext, section: &mut Section) {
    for record in &context.records {
        if record.target == ExtentPriorityTarget::Section(section.id.clone()) && section.preferred_extent.is_some() {
            section.extent_priority = record.priority;
        }
        if record.target == ExtentPriorityTarget::SectionReturn(section.id.clone())
            && let SectionLocation::Window { return_to, .. } = &mut section.location
            && return_to.preferred_extent.is_some()
        {
            return_to.extent_priority = record.priority;
        }
    }
}

fn apply_memory_priorities(context: &ExtentPriorityContext, panel: &mut PanelInstance) {
    for record in &context.records {
        if record.target == ExtentPriorityTarget::Memory(panel.id.clone()) && panel.memory.preferred_extent.is_some() {
            panel.memory.extent_priority = record.priority;
        }
        if record.target == ExtentPriorityTarget::MemoryReturn(panel.id.clone())
            && let SectionLocation::Window { return_to, .. } = &mut panel.memory.location
            && return_to.preferred_extent.is_some()
        {
            return_to.extent_priority = record.priority;
        }
    }
}

fn effective_priority_context(envelope: &LayoutEnvelope) -> PanelResult<ExtentPriorityContext> {
    let mut context = raw_priority_records(envelope)?;
    for record in &mut context.records {
        if let Some(priority) = typed_priority(&envelope.layout, &record.target) {
            record.priority = priority;
        }
    }
    if envelope.extent_priority_overrides.len() > MAX_EXTENT_PRIORITY_RECORDS {
        return Err(error(Rule::CollectionLimit, "priority override cap"));
    }
    let mut seen = std::collections::BTreeSet::new();
    for saved in &envelope.extent_priority_overrides {
        if !seen.insert(&saved.target) {
            return Err(error(Rule::DuplicateId, "duplicate priority override"));
        }
        let record = context
            .records
            .iter_mut()
            .find(|r| r.target == saved.target)
            .ok_or_else(|| error(Rule::MissingTarget, "priority override has no raw preference"))?;
        if record.zone != saved.zone || record.column != saved.column || record.stored_priority != saved.stored_priority {
            return Err(error(Rule::Schema, "priority override disagrees with raw source"));
        }
        record.priority = saved.priority;
    }
    context.validate()?;
    Ok(context)
}

fn seed_priority_context(envelope: &mut LayoutEnvelope) -> PanelResult<()> {
    let context = effective_priority_context(envelope)?;
    for section in &mut envelope.layout.sections {
        apply_section_priorities(&context, section);
    }
    for panel in &mut envelope.layout.instances {
        apply_memory_priorities(&context, panel);
    }
    envelope.layout.extent_priority_context = context;
    Ok(())
}

fn reconcile_priority_context(envelope: &mut LayoutEnvelope) -> PanelResult<()> {
    let mut context = raw_priority_records(envelope)?;
    for record in &mut context.records {
        if let Some(priority) = typed_priority(&envelope.layout, &record.target) {
            record.priority = priority;
        } else if let Some(old) = envelope
            .layout
            .extent_priority_context
            .records
            .iter()
            .find(|old| old.target == record.target && old.zone == record.zone && old.column == record.column)
        {
            record.priority = old.priority;
        }
    }
    envelope.extent_priority_overrides = context.overrides().cloned().collect();
    envelope.layout.extent_priority_context = context;
    Ok(())
}

/// Raw sidecars have already been merged, but their effective tiers must travel
/// with them. Live preset fields remain authoritative; shared latent sources
/// cannot silently choose between conflicting effective recovery intents.
pub(super) fn merge_retained_priority_context(
    envelope: &mut LayoutEnvelope,
    current: &ExtentPriorityContext,
    incoming: &ExtentPriorityContext,
) -> PanelResult<()> {
    current.validate()?;
    incoming.validate()?;
    let mut merged = raw_priority_records(envelope)?;
    for record in &mut merged.records {
        let matches = |source: &&ExtentPriorityRecord| {
            source.target == record.target
                && source.zone == record.zone
                && source.column == record.column
                && source.stored_priority == record.stored_priority
        };
        let retained = current.records.iter().find(matches);
        let preset = incoming.records.iter().find(matches);
        if let Some(priority) = typed_priority(&envelope.layout, &record.target) {
            record.priority = priority;
        } else {
            if let (Some(retained), Some(preset)) = (retained, preset)
                && retained.priority != preset.priority
            {
                return Err(error(Rule::DuplicateId, "retained effective priority collision"));
            }
            if let Some(source) = preset.or(retained) {
                record.priority = source.priority;
            }
        }
    }
    merged.validate()?;
    envelope.extent_priority_overrides = merged.overrides().cloned().collect();
    envelope.layout.extent_priority_context = merged;
    Ok(())
}

fn authors_priority(envelope: &LayoutEnvelope, layout: &Layout, context: &ExtentPriorityContext) -> PanelResult<bool> {
    // Compare owner intent, not just surviving section identities. Open and
    // native redock may copy a saved tier into a new section; close may leave
    // the only newly authored intent in a retained placement memory. A Window
    // primary tuple cannot certify dock inheritance; its return descriptor can.
    type Intent<'a> = (Zone, Option<&'a ColumnId>, f32, u16);
    fn section_intents(section: &Section) -> [Option<Intent<'_>>; 2] {
        [
            section.preferred_extent.map(|extent| (section.location.zone(), section.column.as_ref(), extent, section.extent_priority)),
            match &section.location {
                SectionLocation::Window { return_to, .. } => {
                    return_to.preferred_extent.map(|extent| (return_to.zone, return_to.column.as_ref(), extent, return_to.extent_priority))
                }
                _ => None,
            },
        ]
    }
    fn memory_intents(panel: &PanelInstance) -> [Option<Intent<'_>>; 2] {
        [
            panel
                .memory
                .preferred_extent
                .map(|extent| (panel.memory.location.zone(), panel.memory.column.as_ref(), extent, panel.memory.extent_priority)),
            match &panel.memory.location {
                SectionLocation::Window { return_to, .. } => {
                    return_to.preferred_extent.map(|extent| (return_to.zone, return_to.column.as_ref(), extent, return_to.extent_priority))
                }
                _ => None,
            },
        ]
    }
    let mut sections = envelope.layout.sections.clone();
    for snapshot in envelope.section_snapshots.values() {
        sections.push(serde_json::from_value(snapshot.data.clone()).map_err(|_| error(Rule::Schema, "invalid inherited priority snapshot"))?);
    }
    let mut panels = envelope.layout.instances.clone();
    for item in &envelope.quarantine {
        panels.push(serde_json::from_value(item.instance_data.clone()).map_err(|_| error(Rule::Schema, "invalid inherited priority memory"))?);
    }
    for section in &mut sections {
        apply_section_priorities(context, section);
    }
    for panel in &mut panels {
        apply_memory_priorities(context, panel);
    }
    for section in &layout.sections {
        for intent in section_intents(section).into_iter().flatten().filter(|(_, _, _, rank)| *rank > 0) {
            let inherited = sections.iter().any(|old| {
                (old.id == section.id || old.tabs.iter().any(|id| section.tabs.contains(id))) && section_intents(old).contains(&Some(intent))
            }) || panels.iter().any(|old| section.tabs.contains(&old.id) && memory_intents(old).contains(&Some(intent)));
            if !inherited {
                return Ok(true);
            }
        }
    }
    for panel in &layout.instances {
        for intent in memory_intents(panel).into_iter().flatten().filter(|(_, _, _, rank)| *rank > 0) {
            let inherited = panels.iter().any(|old| old.id == panel.id && memory_intents(old).contains(&Some(intent)))
                || sections
                    .iter()
                    .any(|old| (old.id == panel.memory.section || old.tabs.contains(&panel.id)) && section_intents(old).contains(&Some(intent)));
            if !inherited {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn reserve_context_location(context: &mut LayoutPreparationContext, location: &SectionLocation) {
    if let SectionLocation::Window { return_to, .. } = location {
        if let Some(id) = &return_to.section {
            context.sections.insert(id.clone());
        }
        if let Some(join) = &return_to.join {
            context.sections.insert(join.section.clone());
        }
        if let Some(id) = &return_to.column {
            context.columns.insert(id.clone());
        }
    }
}

/// Immutable full-envelope preparation metadata, without placed surrogates or encoding.
pub fn preparation_context(envelope: &LayoutEnvelope) -> PanelResult<LayoutPreparationContext> {
    validate_wire_structure(&envelope.layout)?;
    validate_envelope_metadata_for_read(envelope, envelope.retained_v1.is_some())?;
    let mut seeded = envelope.clone();
    seeded.layout.extent_priority_context = effective_priority_context(envelope)?;
    // Live edits already carry effective ranks; do not replace them with an old snapshot.
    reconcile_priority_context(&mut seeded)?;
    let mut context = LayoutPreparationContext {
        extent_priorities: seeded.layout.extent_priority_context,
        replacement_ownership: Some(ReplacementIdentityOwnership::from_layout(&envelope.layout)?),
        ..LayoutPreparationContext::default()
    };
    for section in &envelope.layout.sections {
        context.sections.insert(section.id.clone());
        if let Some(id) = &section.column {
            context.columns.insert(id.clone());
        }
        reserve_context_location(&mut context, &section.location);
    }
    for column in &envelope.layout.columns {
        context.columns.insert(column.id.clone());
    }
    for panel in &envelope.layout.instances {
        context.sections.insert(panel.memory.section.clone());
        if let Some(id) = &panel.memory.column {
            context.columns.insert(id.clone());
        }
        reserve_context_location(&mut context, &panel.memory.location);
    }
    for item in &envelope.quarantine {
        let panel: PanelInstance = serde_json::from_value(item.instance_data.clone()).map_err(|_| error(Rule::Schema, "invalid context instance"))?;
        context.sections.insert(panel.memory.section.clone());
        if let Some(id) = &panel.memory.column {
            context.columns.insert(id.clone());
        }
        reserve_context_location(&mut context, &panel.memory.location);
    }
    for (id, snapshot) in &envelope.section_snapshots {
        let section: Section = serde_json::from_value(snapshot.data.clone()).map_err(|_| error(Rule::Schema, "invalid context section"))?;
        context.sections.insert(id.clone());
        if let Some(id) = &section.column {
            context.columns.insert(id.clone());
        }
        reserve_context_location(&mut context, &section.location);
    }
    if context.sections.len() > MAX_RESERVED_IDENTITIES || context.columns.len() > MAX_RESERVED_IDENTITIES {
        return Err(error(Rule::CollectionLimit, "preparation context identity cap"));
    }
    Ok(context)
}

#[derive(Clone, Debug)]
pub struct LoadOutcome {
    pub envelope: LayoutEnvelope,
    /// Adapter reports this once; fallback never blocks startup or touches other preferences.
    pub notice: Option<PanelError>,
    /// Bounded corrupt/future input, available to a recovery adapter before replacement.
    pub rejected: Option<Vec<u8>>,
}

/// Implementations read/write ONLY the workspace panel-layout preference key.
/// `save` must replace atomically, preserving unrelated preferences. Production
/// preference migration is intentionally outside the foundation.
pub trait LayoutPersistence {
    fn load(&mut self, workspace: &WorkspaceId) -> PanelResult<Option<Vec<u8>>>;
    fn save(&mut self, workspace: &WorkspaceId, envelope: &[u8]) -> PanelResult<()>;
}

pub fn encode(envelope: &LayoutEnvelope, registry: &PanelRegistry) -> PanelResult<Vec<u8>> {
    validate_envelope(envelope, registry)?;
    encode_record(envelope)
}

fn encode_record(envelope: &LayoutEnvelope) -> PanelResult<Vec<u8>> {
    if let Some(raw) = &envelope.recovery_wire {
        bounded_json(raw)?;
    }
    let current = serde_json::to_value(envelope).map_err(|_| error(Rule::Schema, "cannot encode panel envelope"))?;
    let value = match &envelope.recovery_wire {
        Some(raw) => {
            let mut raw = raw.clone();
            if let Some(layout) = raw.get_mut("layout") {
                if let Some(sections) = layout.get_mut("sections").and_then(|v| v.as_array_mut()) {
                    sections.retain(|section| {
                        !envelope.section_snapshots.keys().any(|id| section.get("id").and_then(|v| v.as_str()) == Some(id.0.as_str()))
                    });
                }
                if let Some(instances) = layout.get_mut("instances").and_then(|v| v.as_array_mut()) {
                    instances.retain(|instance| {
                        let id = instance.get("id").and_then(|v| v.as_str());
                        !envelope.retained_data.values().any(|data| {
                            data.get("instance").and_then(|p| p.get("id")).and_then(|v| v.as_str()).is_some_and(|saved| Some(saved) == id)
                        })
                    });
                }
            }
            overlay_wire(&raw, current)
        }
        None => current,
    };
    // Aggregated opaque entries must fit the decoder's global node/depth
    // budget before serialization; the writer separately enforces the byte cap.
    bounded_json(&value)?;
    let mut writer = BoundedEnvelopeWriter { bytes: Vec::new() };
    serde_json::to_writer(&mut writer, &value).map_err(|_| error(Rule::CollectionLimit, "cannot encode panel envelope within byte cap"))?;
    Ok(writer.bytes)
}

/// Unknown object keys survive; known arrays use current membership/order.
/// Identity matching prevents an old tab/section's fields following a new peer.
fn overlay_wire(saved: &serde_json::Value, current: serde_json::Value) -> serde_json::Value {
    match (saved, current) {
        (serde_json::Value::Object(old), serde_json::Value::Object(mut new)) => {
            for (key, value) in old {
                let merged = match new.remove(key) {
                    Some(current) => overlay_wire(value, current),
                    None => value.clone(),
                };
                new.insert(key.clone(), merged);
            }
            serde_json::Value::Object(new)
        }
        (serde_json::Value::Array(old), serde_json::Value::Array(new)) => serde_json::Value::Array(
            new.into_iter()
                .map(|current| {
                    let identity = current.get("id").or_else(|| current.get("instance"));
                    match identity.and_then(|id| old.iter().find(|entry| entry.get("id").or_else(|| entry.get("instance")) == Some(id))) {
                        Some(saved) => overlay_wire(saved, current),
                        None => current,
                    }
                })
                .collect(),
        ),
        (_, current) => current,
    }
}

/// These callers supply a typed Section, PlacementMemory or DockReturn. An
/// omitted optional preference clears old known intent, rather than retaining
/// it as an opaque extension. Other nested opaque fields still merge losslessly.
fn overlay_preference_record(saved: &serde_json::Value, current: serde_json::Value) -> serde_json::Value {
    let clear = current.get("preferred_extent").is_none_or(|v| v.is_null());
    let clear_priority = current.get("extent_priority").is_none_or(|v| v.as_u64() == Some(0));
    let clear_native = current
        .get("location")
        .filter(|v| v.get("type").and_then(|v| v.as_str()) == Some("window"))
        .and_then(|v| v.get("return_to"))
        .is_some_and(|v| v.get("preferred_extent").is_none_or(|v| v.is_null()));
    let clear_native_priority = current
        .get("location")
        .filter(|v| v.get("type").and_then(|v| v.as_str()) == Some("window"))
        .and_then(|v| v.get("return_to"))
        .is_some_and(|v| v.get("extent_priority").is_none_or(|v| v.as_u64() == Some(0)));
    let mut merged = overlay_wire(saved, current);
    if clear && let Some(object) = merged.as_object_mut() {
        object.remove("preferred_extent");
    }
    if clear_priority && let Some(object) = merged.as_object_mut() {
        object.remove("extent_priority");
    }
    if clear_native && let Some(object) = merged.get_mut("location").and_then(|v| v.get_mut("return_to")).and_then(|v| v.as_object_mut()) {
        object.remove("preferred_extent");
    }
    if clear_native_priority && let Some(object) = merged.get_mut("location").and_then(|v| v.get_mut("return_to")).and_then(|v| v.as_object_mut()) {
        object.remove("extent_priority");
    }
    merged
}

fn retain_unknown_wire(envelope: &mut LayoutEnvelope, raw: serde_json::Value) -> PanelResult<()> {
    let typed = serde_json::to_value(&*envelope).map_err(|_| error(Rule::Schema, "cannot compare panel wire semantics"))?;
    envelope.recovery_wire = unknown_wire(&raw, &typed);
    Ok(())
}

fn unknown_wire(saved: &serde_json::Value, current: &serde_json::Value) -> Option<serde_json::Value> {
    match (saved, current) {
        (serde_json::Value::Object(old), serde_json::Value::Object(new)) => {
            let mut unknown = serde_json::Map::new();
            let placement_record = new.contains_key("weight")
                && new.contains_key("size")
                && ((new.contains_key("location") && (new.contains_key("tabs") || new.contains_key("tab_index")))
                    || (new.contains_key("zone") && new.contains_key("index")));
            for (key, value) in old {
                // A recognized optional field encoded as null means None, not
                // an unknown extension that can resurrect an omitted override.
                if placement_record && key == "preferred_extent" && value.is_null() && !new.contains_key(key) {
                    continue;
                }
                if placement_record && key == "extent_priority" && value.as_u64() == Some(0) && !new.contains_key(key) {
                    continue;
                }
                let retained = match new.get(key) {
                    Some(current) => unknown_wire(value, current),
                    None => Some(value.clone()),
                };
                if let Some(value) = retained {
                    unknown.insert(key.clone(), value);
                }
            }
            (!unknown.is_empty()).then_some(serde_json::Value::Object(unknown))
        }
        (serde_json::Value::Array(old), serde_json::Value::Array(new)) => {
            let mut unknown = Vec::new();
            for saved in old {
                let key = if saved.get("id").is_some() { "id" } else { "instance" };
                if let Some(id) = saved.get(key)
                    && let Some(current) = new.iter().find(|entry| entry.get(key) == Some(id))
                    && let Some(mut retained) = unknown_wire(saved, current)
                    && let Some(object) = retained.as_object_mut()
                {
                    object.insert(key.into(), id.clone());
                    unknown.push(retained);
                }
            }
            (!unknown.is_empty()).then_some(serde_json::Value::Array(unknown))
        }
        _ => None,
    }
}

fn refresh_recovery_wire(envelope: &mut LayoutEnvelope, encoded: &[u8]) -> PanelResult<()> {
    let raw = serde_json::from_slice(encoded).map_err(|_| error(Rule::Schema, "invalid canonical panel wire"))?;
    retain_unknown_wire(envelope, raw)
}

struct BoundedEnvelopeWriter {
    bytes: Vec<u8>,
}
impl std::io::Write for BoundedEnvelopeWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > MAX_ENVELOPE_BYTES {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "panel envelope byte cap"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn validate_envelope(envelope: &LayoutEnvelope, registry: &PanelRegistry) -> PanelResult<()> {
    if envelope.format != ENVELOPE_FORMAT {
        return Err(error(Rule::Schema, "unknown panel envelope format"));
    }
    if envelope.version != ENVELOPE_VERSION {
        return Err(error(Rule::Version, "unknown panel envelope version"));
    }
    validate(&envelope.layout, registry)?;
    validate_envelope_metadata(envelope)
}

fn validate_envelope_metadata(envelope: &LayoutEnvelope) -> PanelResult<()> {
    validate_envelope_metadata_for_read(envelope, false)
}

/// Only private decoding of an already globally bounded original v1 tree may
/// defer budgets for ADDED typed fields. Semantic limits still apply in full.
/// Public writes always use the strict wrapper and the unchanged encoded cap.
fn validate_envelope_metadata_for_read(envelope: &LayoutEnvelope, migrated_v1: bool) -> PanelResult<()> {
    if envelope.quarantine.len() + envelope.layout.instances.len() > MAX_INSTANCES {
        return Err(error(Rule::CollectionLimit, "quarantine instance cap"));
    }
    if envelope.retained_data.len() > MAX_INSTANCES {
        return Err(error(Rule::CollectionLimit, "retained data cap"));
    }
    for (id, data) in &envelope.retained_data {
        valid_id(&id.0)?;
        if !migrated_v1 {
            bounded_json(data)?;
        }
    }
    if envelope.section_snapshots.len() > MAX_SECTIONS {
        return Err(error(Rule::CollectionLimit, "section snapshot cap"));
    }
    validate_section_order(&envelope.section_order)?;
    for (id, snapshot) in &envelope.section_snapshots {
        validate_snapshot_for_read(id, snapshot, migrated_v1)?;
    }
    let mut ids = std::collections::BTreeSet::new();
    for item in &envelope.quarantine {
        if !migrated_v1 {
            bounded_json(&item.instance_data)?;
        }
        valid_id(&item.instance.0)?;
        valid_id(&item.kind.0)?;
        if !ids.insert(&item.instance) || envelope.layout.instances.iter().any(|p| p.id == item.instance) {
            return Err(error(Rule::DuplicateId, "quarantine identity or index invalid"));
        }
        let instance: PanelInstance =
            serde_json::from_value(item.instance_data.clone()).map_err(|_| error(Rule::Schema, "invalid quarantined instance"))?;
        if instance.id != item.instance || instance.kind != item.kind {
            return Err(error(Rule::Id, "quarantine identity mismatch"));
        }
        if instance.requested_open != item.section.is_some() {
            return Err(error(Rule::ExactlyOnce, "quarantine open/section mismatch"));
        }
        valid_id(&instance.memory.section.0)?;
        instance.memory.location.validate()?;
        instance.memory.size.validate()?;
        validate_column_memory(
            instance.memory.location.zone(),
            &instance.memory.column,
            instance.memory.column_index,
            instance.memory.column_width,
            instance.memory.weight,
        )?;
        validate_extent_priority(instance.memory.preferred_extent, instance.memory.extent_priority)?;
        if instance.memory.section_index > MAX_SECTIONS || instance.memory.tab_index >= MAX_TABS {
            return Err(error(Rule::CollectionLimit, "quarantine placement cap"));
        }
        if let Some(id) = &item.section {
            let snapshot = envelope.section_snapshots.get(id).ok_or_else(|| error(Rule::MissingTarget, "quarantined section snapshot absent"))?;
            let section: Section = serde_json::from_value(snapshot.data.clone()).map_err(|_| error(Rule::Schema, "invalid quarantined section"))?;
            if !section.tabs.contains(&item.instance) {
                return Err(error(Rule::ExactlyOnce, "quarantined instance absent from section snapshot"));
            }
        }
    }
    effective_priority_context(envelope)?;
    Ok(())
}

fn validate_section_order(order: &[SectionId]) -> PanelResult<()> {
    if order.len() > MAX_SECTIONS {
        return Err(error(Rule::CollectionLimit, "complete section order cap"));
    }
    let mut seen = std::collections::BTreeSet::new();
    for id in order {
        valid_id(&id.0)?;
        if !seen.insert(id) {
            return Err(error(Rule::DuplicateId, "duplicate complete section rank"));
        }
    }
    Ok(())
}

fn validate_section_record(section: &Section) -> PanelResult<()> {
    valid_id(&section.id.0)?;
    section.location.validate()?;
    section.size.validate()?;
    validate_weight(section.weight)?;
    validate_extent_priority(section.preferred_extent, section.extent_priority)?;
    if section.location.zone().side() != section.column.is_some() {
        return Err(error(Rule::Placement, "snapshot column/zone mismatch"));
    }
    if let Some(column) = &section.column {
        valid_id(&column.0)?;
    }
    if section.tabs.is_empty() || section.tabs.len() > MAX_TABS || !section.tabs.contains(&section.active) {
        return Err(error(Rule::CollectionLimit, "section snapshot tabs/active bound"));
    }
    let mut seen = std::collections::BTreeSet::new();
    for id in &section.tabs {
        valid_id(&id.0)?;
        if !seen.insert(id) {
            return Err(error(Rule::ExactlyOnce, "section snapshot duplicate tab"));
        }
    }
    Ok(())
}

fn validate_snapshot_for_read(id: &SectionId, snapshot: &SectionSnapshot, migrated_v1: bool) -> PanelResult<()> {
    valid_id(&id.0)?;
    if !migrated_v1 {
        bounded_json(&snapshot.data)?;
    }
    let section: Section = serde_json::from_value(snapshot.data.clone()).map_err(|_| error(Rule::Schema, "invalid shared section snapshot"))?;
    if section.id != *id || snapshot.index > MAX_SECTIONS {
        return Err(error(Rule::Id, "section snapshot identity/index mismatch"));
    }
    validate_section_record(&section)?;
    if let Some(projected) = &snapshot.projected {
        if projected.id != *id {
            return Err(error(Rule::Id, "projected section identity mismatch"));
        }
        validate_section_record(projected)?;
    }
    Ok(())
}

/// Overlay the live relative order on its previous slots, keeping dormant gaps.
/// New live entries attach before their next surviving live neighbour, rather
/// than consuming a dormant slot or reversing a run of newly added entries.
fn merge_visible_order<T: Clone + Ord>(saved: &[T], live: &[T], dormant: &std::collections::BTreeSet<T>) -> Vec<T> {
    let survivors: std::collections::BTreeSet<_> = saved.iter().filter(|id| live.contains(id)).cloned().collect();
    let mut live_survivors = live.iter().filter(|id| survivors.contains(*id));
    let mut merged = Vec::new();
    for id in saved {
        if dormant.contains(id) && !live.contains(id) {
            merged.push(id.clone());
        } else if survivors.contains(id)
            && let Some(next) = live_survivors.next()
        {
            merged.push(next.clone());
        }
    }
    for (index, id) in live.iter().enumerate() {
        if survivors.contains(id) {
            continue;
        }
        let before = live.iter().skip(index + 1).find(|next| survivors.contains(*next));
        let at = before.and_then(|next| merged.iter().position(|old| old == next)).unwrap_or(merged.len());
        merged.insert(at, id.clone());
    }
    merged
}

/// Refresh live semantics before a new quarantine episode, or before a direct
/// restoration after the caller has edited the known layout. Opaque data stays
/// shared; only semantic fields supplied by the current layout are refreshed.
fn refresh_section_memory(
    envelope: &mut LayoutEnvelope,
    raw: Option<&serde_json::Value>,
    capture: &std::collections::BTreeSet<SectionId>,
    migrated_v1: bool,
) -> PanelResult<()> {
    if envelope.section_snapshots.len() > MAX_SECTIONS || envelope.quarantine.len() > MAX_INSTANCES {
        return Err(error(Rule::CollectionLimit, "section memory collection cap"));
    }
    validate_section_order(&envelope.section_order)?;
    for (id, snapshot) in &envelope.section_snapshots {
        validate_snapshot_for_read(id, snapshot, migrated_v1)?;
    }
    let dormant_sections: std::collections::BTreeSet<_> = envelope.quarantine.iter().filter_map(|item| item.section.clone()).collect();
    let live_order: Vec<_> = envelope.layout.sections.iter().map(|section| section.id.clone()).collect();
    let mut saved_order = envelope.section_order.clone();
    if saved_order.is_empty() {
        // Read earlier v1 envelopes without the complete rank list. Sort missing
        // ranks before insertion, never in instance/quarantine processing order.
        saved_order = live_order.clone();
        let mut missing = Vec::new();
        for id in &dormant_sections {
            let snapshot = envelope.section_snapshots.get(id).ok_or_else(|| error(Rule::MissingTarget, "dormant section snapshot missing"))?;
            if !saved_order.contains(id) {
                missing.push((snapshot.index, id.clone()));
            }
        }
        missing.sort();
        for (index, id) in missing {
            saved_order.insert(index.min(saved_order.len()), id);
        }
    } else if dormant_sections.iter().any(|id| !saved_order.contains(id)) {
        return Err(error(Rule::MissingTarget, "complete order omits a dormant section"));
    }
    envelope.section_order = merge_visible_order(&saved_order, &live_order, &dormant_sections);
    validate_section_order(&envelope.section_order)?;
    for (index, section) in envelope.layout.sections.iter().enumerate() {
        let previous = envelope.section_snapshots.get(&section.id);
        if previous.is_none() && !capture.contains(&section.id) {
            continue;
        }
        let current = match raw {
            Some(raw) => raw
                .get("layout")
                .and_then(|layout| layout.get("sections"))
                .and_then(|sections| sections.get(index))
                .cloned()
                .ok_or_else(|| error(Rule::Schema, "saved section raw data missing"))?,
            None => serde_json::to_value(section).map_err(|_| error(Rule::Schema, "cannot encode live section semantics"))?,
        };
        let (mut data, projected) = match previous {
            Some(previous) => (previous.data.clone(), previous.projected.clone()),
            None => (serde_json::json!({}), None),
        };
        let dormant_tabs: std::collections::BTreeSet<_> =
            envelope.quarantine.iter().filter(|item| item.section.as_ref() == Some(&section.id)).map(|item| item.instance.clone()).collect();
        let previous_section: Option<Section> = previous
            .map(|snapshot| serde_json::from_value(snapshot.data.clone()).map_err(|_| error(Rule::Schema, "invalid section memory")))
            .transpose()?;
        let tabs = match &previous_section {
            Some(saved) if !dormant_tabs.is_empty() => merge_visible_order(&saved.tabs, &section.tabs, &dormant_tabs),
            _ => section.tabs.clone(),
        };
        if tabs.len() > MAX_TABS {
            return Err(error(Rule::CollectionLimit, "complete tab order cap"));
        }
        let active = match &previous_section {
            Some(saved)
                if dormant_tabs.contains(&saved.active)
                    && !section.tabs.contains(&saved.active)
                    && !projected.as_ref().is_some_and(|last| last.active != section.active) =>
            {
                saved.active.clone()
            }
            _ => section.active.clone(),
        };
        // Overlay current fields, preserving opaque keys absent from the typed
        // layout. A current raw value for the same key is the latest saved intent.
        let raw_priority = data.get("extent_priority").cloned();
        let raw_return_priority = data.get("location").and_then(|v| v.get("return_to")).and_then(|v| v.get("extent_priority")).cloned();
        data = overlay_preference_record(&data, current);
        if section.preferred_extent.is_some()
            && let Some(object) = data.as_object_mut()
        {
            match raw_priority {
                Some(priority) => {
                    object.insert("extent_priority".into(), priority);
                }
                None => {
                    object.remove("extent_priority");
                }
            }
        }
        if let SectionLocation::Window { return_to, .. } = &section.location
            && return_to.preferred_extent.is_some()
            && let Some(object) = data.get_mut("location").and_then(|v| v.get_mut("return_to")).and_then(|v| v.as_object_mut())
        {
            match raw_return_priority {
                Some(priority) => {
                    object.insert("extent_priority".into(), priority);
                }
                None => {
                    object.remove("extent_priority");
                }
            }
        }
        let object = data.as_object_mut().ok_or_else(|| error(Rule::Schema, "section memory must be an object"))?;
        object.insert("tabs".into(), serde_json::to_value(tabs).map_err(|_| error(Rule::Schema, "cannot encode complete tab order"))?);
        object.insert("active".into(), serde_json::to_value(active).map_err(|_| error(Rule::Schema, "cannot encode complete activation"))?);
        let rank = envelope
            .section_order
            .iter()
            .position(|id| *id == section.id)
            .ok_or_else(|| error(Rule::MissingTarget, "live section has no complete rank"))?;
        envelope.section_snapshots.insert(section.id.clone(), SectionSnapshot { data, index: rank, projected });
    }
    for (id, snapshot) in &mut envelope.section_snapshots {
        if let Some(rank) = envelope.section_order.iter().position(|known| known == id) {
            snapshot.index = rank;
        }
    }
    Ok(())
}

fn record_section_projections(envelope: &mut LayoutEnvelope) {
    let layout = &envelope.layout;
    for (id, snapshot) in &mut envelope.section_snapshots {
        snapshot.projected = layout.sections.iter().find(|section| section.id == *id).cloned();
    }
}

/// Stage live edits without losing dormant order, projected activation or opaque
/// fields. The caller publishes only after its guarded layout dispatch succeeds.
pub fn with_live_layout(envelope: &LayoutEnvelope, mut layout: Layout, registry: &PanelRegistry) -> PanelResult<LayoutEnvelope> {
    encode(envelope, registry)?;
    validate(&layout, registry)?;
    let full = preparation_context(envelope)?;
    let authors_priority = authors_priority(envelope, &layout, &full.extent_priorities)?;
    let unresolved = |old: &ExtentPriorityRecord| {
        typed_priority(&envelope.layout, &old.target).is_none()
            || !(match &old.target {
                ExtentPriorityTarget::Section(id) | ExtentPriorityTarget::SectionReturn(id) => envelope
                    .layout
                    .section(id)
                    .is_ok_and(|s| s.tabs.iter().any(|id| envelope.layout.instance(id).is_ok_and(|p| registry.get(&p.kind).is_ok()))),
                ExtentPriorityTarget::Memory(id) | ExtentPriorityTarget::MemoryReturn(id) => {
                    envelope.layout.instance(id).is_ok_and(|p| registry.get(&p.kind).is_ok())
                }
            })
    };
    if authors_priority
        && full.extent_priorities.records.iter().any(&unresolved)
        && (!layout.extent_priority_context.complete
            || full.extent_priorities.records.iter().filter(|old| unresolved(old)).any(|old| {
                !layout.extent_priority_context.records.iter().any(|record| {
                    record.target == old.target
                        && record.zone == old.zone
                        && record.column == old.column
                        && record.stored_priority == old.stored_priority
                })
            }))
    {
        return Err(error(Rule::Capability, "authored priority requires full recovery context"));
    }
    if authors_priority
        && full.extent_priorities.records.iter().filter(|old| unresolved(old)).any(|old| {
            layout.extent_priority_context.records.iter().any(|record| {
                record.target == old.target
                    && record.zone == old.zone
                    && record.column == old.column
                    && record.stored_priority == old.stored_priority
                    && record.priority > old.priority
            })
        })
    {
        return Err(error(Rule::Revision, "stale recovery context would promote an older opaque preference"));
    }
    let mut priorities = full.extent_priorities;
    for record in &mut priorities.records {
        if let Some(current) = layout.extent_priority_context.records.iter().find(|current| {
            current.target == record.target
                && current.zone == record.zone
                && current.column == record.column
                && current.stored_priority == record.stored_priority
        }) {
            record.priority = current.priority;
        }
    }
    layout.extent_priority_context = priorities;
    synchronize_priority_context(&mut layout)?;
    let reserved = reserve_envelope_identities(envelope)?;
    validate_incoming_reservations(&envelope.layout, &reserved, &layout)?;
    let mut dormant = std::collections::BTreeSet::new();
    for item in &envelope.quarantine {
        let p: PanelInstance = serde_json::from_value(item.instance_data.clone()).map_err(|_| error(Rule::Schema, "invalid dormant placement"))?;
        if let Some(column) = p.memory.column {
            dormant.insert(column);
        }
        if let SectionLocation::Window { return_to, .. } = p.memory.location
            && let Some(column) = return_to.column
        {
            dormant.insert(column);
        }
    }
    if let Some(columns) = envelope.recovery_wire.as_ref().and_then(|raw| raw.get("layout")).and_then(|l| l.get("columns")).and_then(|v| v.as_array())
    {
        for column in columns {
            if let Some(id) = column.get("id").and_then(|v| v.as_str()) {
                dormant.insert(ColumnId::new(id)?);
            }
        }
    }
    for section in &layout.sections {
        let restoring = envelope.layout.instances.iter().any(|p| !p.requested_open && p.memory.section == section.id && section.tabs.contains(&p.id));
        let remembered = reserved.instances.iter().any(|p| {
            p.memory.section == section.id
                || matches!(&p.memory.location, SectionLocation::Window { return_to, .. }
                if return_to.section.as_ref() == Some(&section.id) || return_to.join.as_ref().is_some_and(|join| join.section == section.id))
        });
        let native_return = reserved.sections.iter().any(|s| {
            matches!(&s.location, SectionLocation::Window { return_to, .. }
            if return_to.section.as_ref() == Some(&section.id) || return_to.join.as_ref().is_some_and(|join| join.section == section.id))
        });
        let native_restoring = envelope.layout.sections.iter().any(|s| {
            matches!(&s.location, SectionLocation::Window { return_to, .. }
            if return_to.section.as_ref() == Some(&section.id) && s.tabs.iter().all(|id| section.tabs.contains(id)))
        }) || envelope.layout.instances.iter().any(|p| {
            !p.requested_open
                && section.tabs.contains(&p.id)
                && matches!(&p.memory.location, SectionLocation::Window { return_to, .. } if return_to.section.as_ref() == Some(&section.id))
        });
        if !restoring
            && !native_restoring
            && envelope.layout.section(&section.id).is_err()
            && (remembered || native_return || reserved.sections.iter().any(|saved| saved.id == section.id))
        {
            return Err(error(Rule::DuplicateId, "new section collides with quarantined section identity"));
        }
    }
    for column in &layout.columns {
        let restoring = envelope.layout.instances.iter().any(|p| {
            !p.requested_open
                && p.memory.column.as_ref() == Some(&column.id)
                && layout.containing(&p.id).is_ok_and(|s| s.column.as_ref() == Some(&column.id) && s.id == p.memory.section)
        });
        let native_reserved = reserved
            .instances
            .iter()
            .any(|p| matches!(&p.memory.location, SectionLocation::Window { return_to, .. } if return_to.column.as_ref() == Some(&column.id)))
            || reserved
                .sections
                .iter()
                .any(|s| matches!(&s.location, SectionLocation::Window { return_to, .. } if return_to.column.as_ref() == Some(&column.id)));
        let native_restoring = envelope.layout.sections.iter().any(|s| {
            matches!(&s.location, SectionLocation::Window { return_to, .. }
            if return_to.column.as_ref() == Some(&column.id) && layout.sections.iter().any(|target| target.column.as_ref() == Some(&column.id)
                && s.tabs.iter().all(|id| target.tabs.contains(id))))
        }) || envelope.layout.instances.iter().any(|p| {
            !p.requested_open
                && matches!(&p.memory.location, SectionLocation::Window { return_to, .. }
                if return_to.column.as_ref() == Some(&column.id) && layout.containing(&p.id).is_ok_and(|s| s.column.as_ref() == Some(&column.id)
                    && return_to.section.as_ref().is_none_or(|id| s.id == *id)))
        });
        if !restoring
            && !native_restoring
            && envelope.layout.column(&column.id).is_err()
            && (native_reserved || reserved.columns.iter().any(|saved| saved.id == column.id))
        {
            return Err(error(Rule::DuplicateId, "new column collides with quarantined column identity"));
        }
        if !restoring
            && !native_restoring
            && dormant.contains(&column.id)
            && !envelope.layout.sections.iter().any(|s| s.column.as_ref() == Some(&column.id))
            && layout.sections.iter().any(|s| s.column.as_ref() == Some(&column.id))
        {
            return Err(error(Rule::DuplicateId, "new live column reuses dormant extension identity"));
        }
    }
    // Keep opaque dormant column records and their width/order slots even when
    // the live reducer no longer needs them. They do not reserve paint geometry.
    let saved: Vec<_> = envelope.layout.columns.iter().map(|c| c.id.clone()).collect();
    let live: Vec<_> = layout.columns.iter().map(|c| c.id.clone()).collect();
    let order = merge_visible_order(&saved, &live, &dormant);
    let mut columns = Vec::new();
    for id in order {
        if let Some(column) = layout.columns.iter().chain(&envelope.layout.columns).find(|c| c.id == id) {
            columns.push(column.clone());
        }
    }
    layout.columns = columns;
    validate(&layout, registry)?;
    let mut next = envelope.clone();
    retain_displaced_wire(&mut next, &layout)?;
    next.layout = layout;
    refresh_section_memory(&mut next, None, &std::collections::BTreeSet::new(), false)?;
    record_section_projections(&mut next);
    reconcile_priority_context(&mut next)?;
    let encoded = encode(&next, registry)?;
    refresh_recovery_wire(&mut next, &encoded)?;
    Ok(next)
}

fn reserve_owners<T: Ord + Clone>(
    reservations: &mut std::collections::BTreeMap<T, std::collections::BTreeSet<InstanceId>>,
    id: &T,
    owners: &[InstanceId],
) {
    reservations.entry(id.clone()).or_default().extend(owners.iter().cloned());
}

fn reserve_native_owners(
    sections: &mut std::collections::BTreeMap<SectionId, std::collections::BTreeSet<InstanceId>>,
    columns: &mut std::collections::BTreeMap<ColumnId, std::collections::BTreeSet<InstanceId>>,
    location: &SectionLocation,
    owners: &[InstanceId],
) {
    if let SectionLocation::Window { return_to, .. } = location {
        if let Some(id) = &return_to.section {
            reserve_owners(sections, id, owners);
        }
        if let Some(join) = &return_to.join {
            reserve_owners(sections, &join.section, owners);
        }
        if let Some(id) = &return_to.column {
            reserve_owners(columns, id, owners);
        }
    }
}

/// Validate claims introduced as data, before they can become placed geometry.
/// Reserved includes closed quarantine memories and complete section snapshots.
/// Existing live groups may accept compatible tabs; dormant identities require
/// a same-kind original owner, including a peer sharing the actual new group.
fn validate_incoming_reservations(before: &Layout, reserved: &Layout, incoming: &Layout) -> PanelResult<()> {
    let mut sections = std::collections::BTreeMap::new();
    let mut columns = std::collections::BTreeMap::new();
    for column in &reserved.columns {
        reserve_owners(&mut columns, &column.id, &[]);
    }
    for section in &reserved.sections {
        reserve_owners(&mut sections, &section.id, &section.tabs);
        if let Some(id) = &section.column {
            reserve_owners(&mut columns, id, &section.tabs);
        }
        reserve_native_owners(&mut sections, &mut columns, &section.location, &section.tabs);
    }
    for panel in &reserved.instances {
        let owners = std::slice::from_ref(&panel.id);
        reserve_owners(&mut sections, &panel.memory.section, owners);
        if let Some(id) = &panel.memory.column {
            reserve_owners(&mut columns, id, owners);
        }
        reserve_native_owners(&mut sections, &mut columns, &panel.memory.location, owners);
    }
    let owned = |owners: &[InstanceId], previous: &std::collections::BTreeSet<InstanceId>| {
        owners
            .iter()
            .any(|id| previous.contains(id) && reserved.instance(id).is_ok_and(|old| incoming.instance(id).is_ok_and(|new| old.kind == new.kind)))
    };
    let section_claim = |id: &SectionId, owners: &[InstanceId]| -> PanelResult<()> {
        if let Some(previous) = sections.get(id)
            && before.section(id).is_err()
            && !owned(owners, previous)
        {
            return Err(error(Rule::DuplicateId, "incoming latent section steals a dormant identity"));
        }
        Ok(())
    };
    let column_claim = |id: &ColumnId, owners: &[InstanceId]| -> PanelResult<()> {
        if let Some(previous) = columns.get(id)
            && !before.sections.iter().any(|s| s.column.as_ref() == Some(id))
            && !owned(owners, previous)
        {
            return Err(error(Rule::DuplicateId, "incoming latent column steals a dormant identity"));
        }
        Ok(())
    };
    let native_claim = |location: &SectionLocation, owners: &[InstanceId]| -> PanelResult<()> {
        if let SectionLocation::Window { return_to, .. } = location {
            if let Some(id) = &return_to.section {
                section_claim(id, owners)?;
            }
            if let Some(join) = &return_to.join {
                section_claim(&join.section, owners)?;
            }
            if let Some(id) = &return_to.column {
                column_claim(id, owners)?;
            }
        }
        Ok(())
    };
    for section in &incoming.sections {
        native_claim(&section.location, &section.tabs)?;
    }
    for panel in &incoming.instances {
        let owner = std::slice::from_ref(&panel.id);
        let group = incoming.containing(&panel.id).ok().filter(|_| panel.requested_open);
        let section_owners = group.filter(|s| s.id == panel.memory.section).map_or(owner, |s| s.tabs.as_slice());
        section_claim(&panel.memory.section, section_owners)?;
        if let Some(id) = &panel.memory.column {
            let column_owners = group.filter(|s| s.column.as_ref() == Some(id)).map_or(owner, |s| s.tabs.as_slice());
            column_claim(id, column_owners)?;
        }
        let native_owners = group.filter(|s| s.location == panel.memory.location).map_or(owner, |s| s.tabs.as_slice());
        native_claim(&panel.memory.location, native_owners)?;
    }
    Ok(())
}

fn retain_displaced_wire(envelope: &mut LayoutEnvelope, next: &Layout) -> PanelResult<()> {
    let Some(raw) = envelope.recovery_wire.as_ref().and_then(|raw| raw.get("layout")) else {
        return Ok(());
    };
    if let Some(records) = raw.get("sections").and_then(|v| v.as_array()) {
        for record in records {
            let Some(id) = record.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            let id = SectionId::new(id)?;
            if next.section(&id).is_ok() || envelope.section_snapshots.contains_key(&id) {
                continue;
            }
            let previous = envelope.layout.section(&id)?;
            let data = overlay_wire(record, serde_json::to_value(previous).map_err(|_| error(Rule::Schema, "cannot retain displaced section"))?);
            let index = envelope.layout.sections.iter().position(|s| s.id == id).unwrap_or(MAX_SECTIONS);
            envelope.section_snapshots.insert(id, SectionSnapshot { data, index, projected: None });
        }
    }
    if let Some(records) = raw.get("instances").and_then(|v| v.as_array()) {
        for record in records {
            let Some(id) = record.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            let id = InstanceId::new(id)?;
            if next.instance(&id).is_ok() || envelope.quarantine.iter().any(|item| item.instance == id) {
                continue;
            }
            let previous = envelope.layout.instance(&id)?;
            let instance = overlay_wire(record, serde_json::to_value(previous).map_err(|_| error(Rule::Schema, "cannot retain displaced instance"))?);
            let data = serde_json::json!({"instance":instance,"section":previous.memory.section});
            if envelope.retained_data.get(&id).is_some_and(|old| *old != data) {
                return Err(error(Rule::DuplicateId, "displaced instance opaque intent collision"));
            }
            envelope.retained_data.insert(id, data);
        }
    }
    Ok(())
}

/// Allocation-only projection includes dormant identities without exposing
/// quarantined panels to the live reducer or presentation.
pub fn reserve_envelope_identities(envelope: &LayoutEnvelope) -> PanelResult<Layout> {
    validate_envelope_metadata(envelope)?;
    let mut layout = envelope.layout.clone();
    for item in &envelope.quarantine {
        let instance: PanelInstance =
            serde_json::from_value(item.instance_data.clone()).map_err(|_| error(Rule::Schema, "invalid reserved instance"))?;
        layout.instances.push(instance);
    }
    for snapshot in envelope.section_snapshots.values() {
        let section: Section = serde_json::from_value(snapshot.data.clone()).map_err(|_| error(Rule::Schema, "invalid reserved section"))?;
        if layout.section(&section.id).is_err() {
            layout.sections.push(section);
        }
    }
    for instance in &layout.instances {
        if let Some(id) = &instance.memory.column
            && !layout.columns.iter().any(|c| c.id == *id)
        {
            layout.columns.push(Column {
                id: id.clone(),
                zone: instance.memory.location.zone(),
                width: instance.memory.column_width.unwrap_or(instance.memory.size.width),
            });
        }
    }
    for section in &layout.sections {
        if let Some(id) = &section.column
            && !layout.columns.iter().any(|c| c.id == *id)
        {
            layout.columns.push(Column { id: id.clone(), zone: section.location.zone(), width: section.size.width });
        }
    }
    for location in layout.sections.iter().map(|s| &s.location).chain(layout.instances.iter().map(|p| &p.memory.location)) {
        if let SectionLocation::Window { return_to, .. } = location
            && let Some(id) = &return_to.column
            && !layout.columns.iter().any(|column| column.id == *id)
        {
            layout.columns.push(Column { id: id.clone(), zone: return_to.zone, width: return_to.column_width.unwrap_or(return_to.size.width) });
        }
    }
    Ok(layout)
}

/// Iterative traversal also bounds caller-constructed JSON, which did not pass
/// through serde_json's parser/recursion limit.
pub fn bounded_json(value: &serde_json::Value) -> PanelResult<()> {
    let mut stack = vec![(value, 0_usize)];
    let mut nodes = 0_usize;
    let mut bytes = 0_usize;
    while let Some((value, depth)) = stack.pop() {
        nodes = nodes.saturating_add(1);
        if nodes > 8192 || depth > 32 {
            return Err(error(Rule::CollectionLimit, "JSON node/depth cap"));
        }
        match value {
            serde_json::Value::String(s) => bytes = bytes.saturating_add(s.len()),
            serde_json::Value::Array(values) => {
                if values.len() > 8192 || stack.len().saturating_add(values.len()) > 8192 {
                    return Err(error(Rule::CollectionLimit, "JSON array cap"));
                }
                stack.extend(values.iter().map(|value| (value, depth + 1)));
            }
            serde_json::Value::Object(values) => {
                if values.len() > 8192 || stack.len().saturating_add(values.len()) > 8192 {
                    return Err(error(Rule::CollectionLimit, "JSON object cap"));
                }
                for (key, value) in values {
                    bytes = bytes.saturating_add(key.len());
                    stack.push((value, depth + 1));
                }
            }
            serde_json::Value::Number(n) if n.as_f64().is_some_and(|n| !n.is_finite()) => return Err(error(Rule::Bounds, "nonfinite JSON value")),
            _ => {}
        }
        if bytes > MAX_ENVELOPE_BYTES {
            return Err(error(Rule::CollectionLimit, "JSON byte cap"));
        }
    }
    Ok(())
}

#[derive(Default)]
struct MigrationColumns {
    columns: Vec<Column>,
    sections: std::collections::BTreeMap<SectionId, ColumnId>,
    bounds: std::collections::BTreeMap<ColumnId, (f32, f32)>,
    section_bounds: std::collections::BTreeMap<SectionId, (f32, f32)>,
}
impl MigrationColumns {
    fn assign(&mut self, zone: Zone, section: Option<&SectionId>, width: f32) -> PanelResult<(ColumnId, usize)> {
        if let Some(column) = section.and_then(|id| self.sections.get(id))
            && self.columns.iter().any(|c| c.id == *column && c.zone == zone)
        {
            let rank = self.columns.iter().filter(|c| c.zone == zone).position(|c| c.id == *column).unwrap_or(0);
            return Ok((column.clone(), rank));
        }
        let (min, max) = section.and_then(|id| self.section_bounds.get(id)).copied().unwrap_or((width, width));
        // Preserve the v1 vertical stack and every preferred size. A shared
        // column width is independently clamped to the composite interval.
        // Only genuinely incompatible old sections require a further column.
        let existing = self.columns.iter().rev().find(|c| c.zone == zone).and_then(|column| {
            let (lower, upper) = self.bounds.get(&column.id).copied().unwrap_or((column.width, column.width));
            (lower.max(min) <= upper.min(max)).then_some((column.id.clone(), lower.max(min), upper.min(max)))
        });
        let column = match existing {
            Some((id, lower, upper)) => {
                if let Some(column) = self.columns.iter_mut().find(|c| c.id == id) {
                    column.width = column.width.clamp(lower, upper);
                }
                self.bounds.insert(id.clone(), (lower, upper));
                id
            }
            None => {
                let rank = self.columns.iter().filter(|c| c.zone == zone).count();
                let side = if zone == Zone::Left { "left" } else { "right" };
                let column = ColumnId::new(format!("column-v1-{side}-{}", rank + 1))?;
                self.columns.push(Column { id: column.clone(), zone, width });
                self.bounds.insert(column.clone(), (min, max));
                column
            }
        };
        if let Some(section) = section {
            self.sections.entry(section.clone()).or_insert_with(|| column.clone());
        }
        let rank = self.columns.iter().filter(|c| c.zone == zone).position(|c| c.id == column).unwrap_or(0);
        Ok((column, rank))
    }
}

fn migrate_location(location: &mut serde_json::Value, columns: &mut MigrationColumns) -> PanelResult<()> {
    if location.get("type").and_then(|v| v.as_str()) == Some("window") {
        let destination = location.get_mut("return_to").ok_or_else(|| error(Rule::Schema, "native return absent"))?;
        let mut destination_typed: DockReturn =
            serde_json::from_value(destination.clone()).map_err(|_| error(Rule::Schema, "invalid v1 native return"))?;
        if destination_typed.zone.side() && destination_typed.column.is_none() {
            let section = destination_typed.section.as_ref().or_else(|| destination_typed.join.as_ref().map(|join| &join.section));
            let (column, rank) = columns.assign(destination_typed.zone, section, destination_typed.size.width)?;
            destination_typed.column = Some(column);
            destination_typed.column_index = rank;
            destination_typed.column_width = Some(destination_typed.size.width);
        }
        if destination.get("weight").is_none() {
            destination_typed.weight =
                if destination_typed.zone == Zone::Bottom { destination_typed.size.width } else { destination_typed.size.height };
        }
        let current = serde_json::to_value(destination_typed).map_err(|_| error(Rule::Schema, "cannot migrate native return"))?;
        *destination = overlay_preference_record(destination, current);
    }
    Ok(())
}

fn migrate_section(value: &mut serde_json::Value, columns: &mut MigrationColumns) -> PanelResult<()> {
    if let Some(location) = value.get_mut("location") {
        migrate_location(location, columns)?;
    }
    let mut section: Section = serde_json::from_value(value.clone()).map_err(|_| error(Rule::Schema, "invalid v1 section"))?;
    let zone = section.location.zone();
    if zone.side() && section.column.is_none() {
        section.column = Some(columns.assign(zone, Some(&section.id), section.size.width)?.0);
    }
    if value.get("weight").is_none() {
        section.weight = if zone == Zone::Bottom { section.size.width } else { section.size.height };
    }
    let current = serde_json::to_value(section).map_err(|_| error(Rule::Schema, "cannot migrate section"))?;
    *value = overlay_preference_record(value, current);
    Ok(())
}

fn migrate_instance(value: &mut serde_json::Value, columns: &mut MigrationColumns) -> PanelResult<()> {
    let memory = value.get_mut("memory").ok_or_else(|| error(Rule::Schema, "v1 placement memory absent"))?;
    if let Some(location) = memory.get_mut("location") {
        migrate_location(location, columns)?;
    }
    let mut typed: PlacementMemory = serde_json::from_value(memory.clone()).map_err(|_| error(Rule::Schema, "invalid v1 placement memory"))?;
    let zone = typed.location.zone();
    if zone.side() && typed.column.is_none() {
        let (column, rank) = columns.assign(zone, Some(&typed.section), typed.size.width)?;
        typed.column = Some(column);
        typed.column_index = rank;
        typed.column_width = Some(typed.size.width);
    }
    if memory.get("weight").is_none() {
        typed.weight = if zone == Zone::Bottom { typed.size.width } else { typed.size.height };
    }
    let current = serde_json::to_value(typed).map_err(|_| error(Rule::Schema, "cannot migrate placement memory"))?;
    *memory = overlay_preference_record(memory, current);
    Ok(())
}

/// Migrate the existing Rust envelope only. Opaque extension keys are overlaid,
/// not deserialized away; a failed migration leaves the exact bytes recoverable.
fn migrate_wire(raw: &mut serde_json::Value, registry: &PanelRegistry) -> PanelResult<()> {
    let envelope_version = raw.get("version").and_then(|v| v.as_u64()).ok_or_else(|| error(Rule::Version, "envelope version absent"))?;
    let layout_version =
        raw.get("layout").and_then(|l| l.get("version")).and_then(|v| v.as_u64()).ok_or_else(|| error(Rule::Version, "layout version absent"))?;
    if envelope_version == u64::from(ENVELOPE_VERSION) && layout_version == u64::from(LAYOUT_VERSION) {
        return Ok(());
    }
    if envelope_version != 1 || layout_version != 1 {
        return Err(error(Rule::Version, "future or unsupported panel layout"));
    }
    let mut columns = MigrationColumns::default();
    let mut kinds = std::collections::BTreeMap::new();
    if let Some(instances) = raw.get("layout").and_then(|l| l.get("instances")).and_then(|v| v.as_array()) {
        for instance in instances {
            if let (Some(id), Some(kind)) = (instance.get("id").and_then(|v| v.as_str()), instance.get("kind").and_then(|v| v.as_str())) {
                kinds.insert(id.to_owned(), PanelKind::new(kind)?);
            }
        }
    }
    if let Some(quarantine) = raw.get("quarantine").and_then(|v| v.as_array()) {
        for instance in quarantine.iter().filter_map(|item| item.get("instance_data")) {
            if let (Some(id), Some(kind)) = (instance.get("id").and_then(|v| v.as_str()), instance.get("kind").and_then(|v| v.as_str())) {
                kinds.entry(id.to_owned()).or_insert(PanelKind::new(kind)?);
            }
        }
    }
    let mut records = Vec::new();
    if let Some(sections) = raw.get("layout").and_then(|l| l.get("sections")).and_then(|v| v.as_array()) {
        records.extend(sections.iter());
    }
    if let Some(snapshots) = raw.get("section_snapshots").and_then(|v| v.as_object()) {
        records.extend(snapshots.values().filter_map(|v| v.get("data")));
    }
    for record in records {
        let section: Section = serde_json::from_value(record.clone()).map_err(|_| error(Rule::Schema, "invalid v1 section limits"))?;
        let mut min: f32 = 0.0;
        let mut max = MAX_LOGICAL;
        for tab in &section.tabs {
            if let Some(spec) = kinds.get(&tab.0).and_then(|kind| registry.get(kind).ok()) {
                min = min.max(spec.min_size.width);
                max = max.min(spec.max_size.width);
            } else {
                // An absent peer's saved valid width is its only known safe
                // interval. Preserve that slot instead of grouping across it.
                min = min.max(section.size.width);
                max = max.min(section.size.width);
            }
        }
        if min > max {
            return Err(error(Rule::IncompatibleTabs, "v1 tab width intervals do not intersect"));
        }
        columns.section_bounds.entry(section.id).or_insert((min, max));
    }
    // Group in COMPLETE order before assigning any live column. Grouping A/C
    // first cannot later insert an incompatible dormant B between them.
    let mut records = std::collections::BTreeMap::new();
    let mut live_order = Vec::new();
    if let Some(sections) = raw.get("layout").and_then(|l| l.get("sections")).and_then(|v| v.as_array()) {
        for value in sections {
            let section: Section = serde_json::from_value(value.clone()).map_err(|_| error(Rule::Schema, "invalid migration section"))?;
            live_order.push(section.id.clone());
            records.insert(section.id, value.clone());
        }
    }
    let dormant: std::collections::BTreeSet<_> = raw
        .get("quarantine")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("section").and_then(|v| v.as_str()))
        .map(SectionId::new)
        .collect::<PanelResult<_>>()?;
    let mut missing = Vec::new();
    if let Some(snapshots) = raw.get("section_snapshots").and_then(|v| v.as_object()) {
        for (id, snapshot) in snapshots {
            let id = SectionId::new(id)?;
            if !dormant.contains(&id) {
                continue;
            }
            let data = snapshot.get("data").ok_or_else(|| error(Rule::Schema, "dormant migration snapshot absent"))?;
            records.entry(id.clone()).or_insert_with(|| data.clone());
            let rank = snapshot.get("index").and_then(|v| v.as_u64()).and_then(|v| usize::try_from(v).ok()).unwrap_or(MAX_SECTIONS);
            missing.push((rank, id));
        }
    }
    let mut complete = Vec::new();
    if let Some(order) = raw.get("section_order").and_then(|v| v.as_array()) {
        for id in order {
            let id = SectionId::new(id.as_str().ok_or_else(|| error(Rule::Id, "invalid complete migration rank"))?)?;
            if records.contains_key(&id) && !complete.contains(&id) {
                complete.push(id);
            }
        }
    }
    for id in live_order {
        if !complete.contains(&id) {
            complete.push(id);
        }
    }
    missing.sort();
    for (rank, id) in missing {
        if !complete.contains(&id) {
            complete.insert(rank.min(complete.len()), id);
        }
    }
    if complete.len() > MAX_SECTIONS {
        return Err(error(Rule::CollectionLimit, "complete migration section cap"));
    }
    for id in &complete {
        let record = records.get_mut(id).ok_or_else(|| error(Rule::MissingTarget, "complete migration record absent"))?;
        let section: Section = serde_json::from_value(record.clone()).map_err(|_| error(Rule::Schema, "invalid complete migration record"))?;
        if section.location.zone().side() {
            migrate_section(record, &mut columns)?;
        }
    }
    if let Some(sections) = raw.get_mut("layout").and_then(|l| l.get_mut("sections")).and_then(|v| v.as_array_mut()) {
        for section in sections {
            migrate_section(section, &mut columns)?;
        }
    }
    // Retain dormant column records as identity/order/width data, never paint.
    let saved_columns: std::collections::BTreeSet<_> = columns.columns.iter().map(|c| c.id.clone()).collect();
    if let Some(snapshots) = raw.get_mut("section_snapshots").and_then(|v| v.as_object_mut()) {
        for snapshot in snapshots.values_mut() {
            if let Some(data) = snapshot.get_mut("data") {
                migrate_section(data, &mut columns)?;
            }
            if let Some(projected) = snapshot.get_mut("projected").filter(|v| !v.is_null()) {
                migrate_section(projected, &mut columns)?;
            }
        }
    }
    if let Some(instances) = raw.get_mut("layout").and_then(|l| l.get_mut("instances")).and_then(|v| v.as_array_mut()) {
        for instance in instances {
            migrate_instance(instance, &mut columns)?;
        }
    }
    if let Some(quarantine) = raw.get_mut("quarantine").and_then(|v| v.as_array_mut()) {
        for item in quarantine {
            if let Some(data) = item.get_mut("instance_data") {
                migrate_instance(data, &mut columns)?;
            }
        }
    }
    if let Some(retained) = raw.get_mut("retained_data").and_then(|v| v.as_object_mut()) {
        for data in retained.values_mut() {
            if let Some(instance) = data.get_mut("instance").filter(|v| v.get("memory").is_some()) {
                migrate_instance(instance, &mut columns)?;
            }
        }
    }
    let layout = raw.get_mut("layout").and_then(|v| v.as_object_mut()).ok_or_else(|| error(Rule::Schema, "layout must be object"))?;
    let saved_columns: Vec<_> = columns.columns.into_iter().filter(|c| saved_columns.contains(&c.id)).collect();
    layout.insert("columns".into(), serde_json::to_value(saved_columns).map_err(|_| error(Rule::Schema, "cannot migrate columns"))?);
    layout.insert("version".into(), LAYOUT_VERSION.into());
    let envelope = raw.as_object_mut().ok_or_else(|| error(Rule::Schema, "envelope must be object"))?;
    envelope.insert("version".into(), ENVELOPE_VERSION.into());
    // The caller bounded the ORIGINAL v1 tree. Added typed fields/columns are
    // bounded by semantic collection limits; v2 encoding budgets apply at staging.
    Ok(())
}

/// Decode bounded bytes, quarantine unknown kinds, and retry retained panels.
/// A valid v1 record retains its original bytes even if v2 expansion will refuse
/// explicit staging. Corrupt/future input returns a safe preset plus notice.
pub fn decode(bytes: &[u8], registry: &PanelRegistry, fallback: &Layout) -> PanelResult<LoadOutcome> {
    validate(fallback, registry)?;
    match decode_inner(bytes, registry, &fallback.workspace, true) {
        Ok(envelope) => Ok(LoadOutcome { envelope, notice: None, rejected: None }),
        Err(reason) => Ok(LoadOutcome {
            envelope: LayoutEnvelope::new(fallback.clone()),
            notice: Some(reason),
            rejected: if bytes.len() <= MAX_ENVELOPE_BYTES { Some(bytes.to_vec()) } else { None },
        }),
    }
}

/// Validate an already-retained record exactly, without generating section order,
/// projections or quarantine/restoration metadata. Missing registrations do not
/// invalidate saved intent. Structure and every applicable known-kind constraint
/// are still validated, but this retained layout is NOT permission to commit it
/// to a live sink. Use `project_retained_record` to stage registry migration.
pub fn decode_retained_record(bytes: &[u8], registry: &PanelRegistry) -> PanelResult<LayoutEnvelope> {
    if bytes.len() > MAX_ENVELOPE_BYTES {
        return Err(error(Rule::CollectionLimit, "panel envelope exceeds byte cap"));
    }
    let mut raw: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| error(Rule::Schema, "invalid panel envelope JSON"))?;
    bounded_json(&raw)?;
    let migrated_v1 = raw.get("version").and_then(|v| v.as_u64()) == Some(1);
    migrate_wire(&mut raw, registry)?;
    let mut envelope: LayoutEnvelope = serde_json::from_value(raw.clone()).map_err(|_| error(Rule::Schema, "invalid panel envelope shape"))?;
    retain_unknown_wire(&mut envelope, raw)?;
    if migrated_v1 {
        envelope.retained_v1 = Some(std::sync::Arc::from(bytes));
    }
    if envelope.format != ENVELOPE_FORMAT {
        return Err(error(Rule::Schema, "unknown panel envelope format"));
    }
    if envelope.version != ENVELOPE_VERSION {
        return Err(error(Rule::Version, "unknown panel envelope version"));
    }
    validate_retained_layout(&envelope.layout, registry)?;
    validate_envelope_metadata_for_read(&envelope, migrated_v1)?;
    seed_priority_context(&mut envelope)?;
    // Current records retain the canonical encodability guarantee. Legacy reads
    // preserve the original globally bounded bytes; only explicit staging may
    // refuse the extra known-field overhead of the migrated v2 encoding.
    if !migrated_v1 {
        let encoded = encode_record(&envelope)?;
        refresh_recovery_wire(&mut envelope, &encoded)?;
    }
    Ok(envelope)
}

/// Read a retained record in its workspace without consuming a reappearance
/// obligation or materializing absent order/projection metadata.
pub fn decode_retained(bytes: &[u8], registry: &PanelRegistry, workspace: &WorkspaceId) -> PanelResult<LayoutEnvelope> {
    valid_id(&workspace.0)?;
    let envelope = decode_retained_record(bytes, registry)?;
    if envelope.layout.workspace != *workspace {
        return Err(error(Rule::Schema, "panel envelope workspace mismatch"));
    }
    Ok(envelope)
}

/// Stage a usable current-registry layout without consuming quarantined
/// reappearance obligations. Newly absent kinds move to the foundation's
/// lossless quarantine. Metadata expansion can fail at the existing caps; the
/// original record remains available for retry and no live state is published.
pub fn project_retained_record(bytes: &[u8], registry: &PanelRegistry, workspace: &WorkspaceId) -> PanelResult<LayoutEnvelope> {
    let envelope = decode_retained(bytes, registry, workspace)?;
    if envelope.layout.instances.iter().any(|instance| registry.get(&instance.kind).is_err()) {
        decode_inner(bytes, registry, workspace, false)
    } else {
        Ok(envelope)
    }
}

fn decode_inner(bytes: &[u8], registry: &PanelRegistry, workspace: &WorkspaceId, restore: bool) -> PanelResult<LayoutEnvelope> {
    if bytes.len() > MAX_ENVELOPE_BYTES {
        return Err(error(Rule::CollectionLimit, "panel envelope exceeds byte cap"));
    }
    // serde_json's recursion limit remains enabled. Parsing starts only after the
    // byte bound, so oversized arrays/strings cannot allocate without a bound.
    let mut raw: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| error(Rule::Schema, "invalid panel envelope JSON"))?;
    bounded_json(&raw)?;
    let migrated_v1 = raw.get("version").and_then(|v| v.as_u64()) == Some(1);
    migrate_wire(&mut raw, registry)?;
    let mut envelope: LayoutEnvelope = serde_json::from_value(raw.clone()).map_err(|_| error(Rule::Schema, "invalid panel envelope shape"))?;
    retain_unknown_wire(&mut envelope, raw.clone())?;
    if migrated_v1 {
        envelope.retained_v1 = Some(std::sync::Arc::from(bytes));
    }
    if envelope.version != ENVELOPE_VERSION || envelope.layout.version != LAYOUT_VERSION {
        return Err(error(Rule::Version, "future or unsupported panel layout"));
    }
    if envelope.format != ENVELOPE_FORMAT || envelope.layout.workspace != *workspace {
        return Err(error(Rule::Schema, "panel envelope format/workspace mismatch"));
    }
    if envelope.layout.instances.len() > MAX_INSTANCES
        || envelope.layout.sections.len() > MAX_SECTIONS
        || envelope.quarantine.len() > MAX_INSTANCES
        || envelope.layout.sections.iter().any(|s| s.tabs.len() > MAX_TABS)
    {
        return Err(error(Rule::CollectionLimit, "panel collection limit"));
    }
    validate_retained_layout(&envelope.layout, registry)?;
    validate_envelope_metadata_for_read(&envelope, migrated_v1)?;
    seed_priority_context(&mut envelope)?;
    let unknown: Vec<_> = envelope.layout.instances.iter().filter(|p| registry.get(&p.kind).is_err()).cloned().collect();
    let capture: std::collections::BTreeSet<_> = envelope
        .layout
        .sections
        .iter()
        .filter(|section| unknown.iter().any(|p| section.tabs.contains(&p.id)))
        .map(|section| section.id.clone())
        .collect();
    // Refresh from the saved live layout BEFORE newly unknown members are
    // removed. A previous fully-restored snapshot is opaque metadata, not an
    // authority that can overwrite intervening saved edits.
    refresh_section_memory(&mut envelope, Some(&raw), &capture, migrated_v1)?;
    for instance in unknown {
        let section_index = envelope.layout.sections.iter().position(|s| s.tabs.contains(&instance.id));
        let instance_data = raw
            .get("layout")
            .and_then(|l| l.get("instances"))
            .and_then(|a| a.as_array())
            .and_then(|a| a.iter().find(|v| v.get("id").and_then(|s| s.as_str()) == Some(instance.id.0.as_str())))
            .cloned()
            .ok_or_else(|| error(Rule::Schema, "unknown instance raw data missing"))?;
        let section = if let Some(index) = section_index {
            let id = envelope.layout.sections.get(index).ok_or_else(|| error(Rule::MissingTarget, "unknown section absent"))?.id.clone();
            Some(id)
        } else {
            None
        };
        envelope.quarantine.push(QuarantinedPanel { instance: instance.id.clone(), kind: instance.kind, instance_data, section });
        envelope.layout.instances.retain(|p| p.id != instance.id);
    }
    let retained: std::collections::BTreeSet<_> = envelope.layout.instances.iter().map(|p| p.id.clone()).collect();
    for section in &mut envelope.layout.sections {
        section.tabs.retain(|id| retained.contains(id));
        if !section.tabs.contains(&section.active)
            && let Some(first) = section.tabs.first()
        {
            section.active = first.clone();
        }
    }
    envelope.layout.sections.retain(|s| !s.tabs.is_empty());
    // Newly quarantining an active tab creates a fallback selection. That
    // projection is produced by us, not an intervening user activation.
    record_section_projections(&mut envelope);
    reconcile_priority_context(&mut envelope)?;
    validate(&envelope.layout, registry)?;
    validate_envelope_metadata_for_read(&envelope, migrated_v1)?;
    if restore {
        restore_quarantine_for_read(&mut envelope, registry, &AvailabilityMap::new(), migrated_v1)?;
    }
    // v2 reads remain persistable. Valid v1 projection retains original bytes
    // and defers an expanded encoding refusal to an explicit staging operation.
    if !migrated_v1 {
        let encoded = encode(&envelope, registry)?;
        refresh_recovery_wire(&mut envelope, &encoded)?;
    }
    Ok(envelope)
}

pub fn restore_quarantine(envelope: &mut LayoutEnvelope, registry: &PanelRegistry) -> PanelResult<()> {
    restore_quarantine_with_availability(envelope, registry, &AvailabilityMap::new())
}

pub fn restore_quarantine_with_availability(
    envelope: &mut LayoutEnvelope,
    registry: &PanelRegistry,
    availability: &AvailabilityMap,
) -> PanelResult<()> {
    restore_quarantine_for_read(envelope, registry, availability, false)
}

fn restore_quarantine_for_read(
    envelope: &mut LayoutEnvelope,
    registry: &PanelRegistry,
    availability: &AvailabilityMap,
    migrated_v1: bool,
) -> PanelResult<()> {
    if migrated_v1 {
        validate(&envelope.layout, registry)?;
        validate_envelope_metadata_for_read(envelope, true)?;
    } else {
        validate_envelope(envelope, registry)?;
    }
    if availability.len() > MAX_INSTANCES {
        return Err(error(Rule::CollectionLimit, "reappearance availability cap"));
    }
    for (id, state) in availability {
        valid_id(&id.0)?;
        if matches!(state, Availability::Unavailable { reason } if reason.is_empty() || reason.len() > 256) {
            return Err(error(Rule::Availability, "invalid reappearance availability reason"));
        }
    }
    let mut working = envelope.clone();
    working.layout.extent_priority_context = preparation_context(envelope)?.extent_priorities;
    // Direct consumers may have edited the live layout since the last decode or
    // restore. Merge those edits before using saved ranks/activation again.
    refresh_section_memory(&mut working, None, &std::collections::BTreeSet::new(), migrated_v1)?;
    // Build a candidate for each entry. An incompatible reappearance stays in
    // quarantine instead of invalidating the known layout or erasing its wire data.
    let mut remaining = Vec::new();
    for item in &envelope.quarantine {
        if registry.get(&item.kind).is_err() || availability.get(&item.instance).is_some_and(|state| *state != Availability::Available) {
            remaining.push(item.clone());
            continue;
        }
        let mut candidate = working.layout.clone();
        let mut instance: PanelInstance =
            serde_json::from_value(item.instance_data.clone()).map_err(|_| error(Rule::Schema, "invalid quarantined instance"))?;
        apply_memory_priorities(&working.layout.extent_priority_context, &mut instance);
        candidate.instances.push(instance.clone());
        if instance.requested_open {
            let Some(section_id) = &item.section else {
                remaining.push(item.clone());
                continue;
            };
            let snapshot = working.section_snapshots.get(section_id).ok_or_else(|| error(Rule::MissingTarget, "shared snapshot missing"))?;
            let mut saved: Section = serde_json::from_value(snapshot.data.clone()).map_err(|_| error(Rule::Schema, "invalid quarantined section"))?;
            apply_section_priorities(&working.layout.extent_priority_context, &mut saved);
            if let Some(section) = candidate.sections.iter_mut().find(|s| s.id == saved.id) {
                let members: std::collections::BTreeSet<_> = section.tabs.iter().cloned().chain(std::iter::once(instance.id.clone())).collect();
                section.tabs = saved
                    .tabs
                    .iter()
                    .filter(|id| members.contains(*id))
                    .cloned()
                    .chain(section.tabs.iter().filter(|id| !saved.tabs.contains(id)).cloned())
                    .collect();
                if saved.active == instance.id {
                    section.active = saved.active.clone();
                }
            } else {
                if let Some(column) = &saved.column
                    && candidate.column(column).is_err()
                {
                    let zone = saved.location.zone();
                    let rank = instance.memory.column_index;
                    let index = candidate
                        .columns
                        .iter()
                        .enumerate()
                        .filter(|(_, c)| c.zone == zone)
                        .nth(rank)
                        .map(|(i, _)| i)
                        .unwrap_or(candidate.columns.len());
                    candidate
                        .columns
                        .insert(index, Column { id: column.clone(), zone, width: instance.memory.column_width.unwrap_or(saved.size.width) });
                }
                saved.tabs = vec![instance.id.clone()];
                saved.active = instance.id.clone();
                candidate.sections.push(saved);
            }
        }
        // Rank against the full saved order, not an index clamped into a partial
        // list. Sorting also includes surviving sections with no snapshot.
        candidate.sections.sort_by_key(|section| working.section_order.iter().position(|id| *id == section.id).unwrap_or(MAX_SECTIONS));
        if validate(&candidate, registry).is_ok() {
            if working.retained_data.len() >= MAX_INSTANCES && !working.retained_data.contains_key(&item.instance) {
                remaining.push(item.clone());
                continue;
            }
            working
                .retained_data
                .entry(item.instance.clone())
                .or_insert_with(|| serde_json::json!({"instance":item.instance_data,"section":item.section}));
            working.layout = candidate;
        } else {
            remaining.push(item.clone());
        }
    }
    working.quarantine = remaining;
    record_section_projections(&mut working);
    reconcile_priority_context(&mut working)?;
    if !migrated_v1 {
        let encoded = encode(&working, registry)?;
        refresh_recovery_wire(&mut working, &encoded)?;
    } else {
        validate(&working.layout, registry)?;
        validate_envelope_metadata_for_read(&working, true)?;
    }
    *envelope = working;
    Ok(())
}

/// Validate saved identities/ownership independently of optional peers, then use
/// the ordinary validator for known members. The temporary known-only view is
/// validation input only: it is never returned, encoded, or dispatched.
fn validate_retained_layout(layout: &Layout, registry: &PanelRegistry) -> PanelResult<()> {
    validate_wire_structure(layout)?;
    if layout.instances.iter().all(|instance| registry.get(&instance.kind).is_ok()) {
        return validate(layout, registry);
    }
    let mut edges = std::collections::BTreeSet::new();
    for section in &layout.sections {
        let zone = section.location.zone();
        let first = edges.insert(zone);
        for id in &section.tabs {
            let instance = layout.instance(id)?;
            if let Ok(spec) = registry.get(&instance.kind) {
                // Filtering absent tabs/sections must not hide these constraints
                // of a known member in the ORIGINAL full saved placement.
                if section.tabs.len() > 1 && !spec.features.tabbable {
                    return Err(error(Rule::FeatureDisabled, "panel cannot join tabs"));
                }
                if spec.edge_rule == EdgeRule::PinOuter && matches!(zone, Zone::Left | Zone::Right) && !first {
                    return Err(error(Rule::PinOrder, "pinned section must be outermost"));
                }
            }
        }
    }
    let mut known = layout.clone();
    known.instances.retain(|instance| registry.get(&instance.kind).is_ok());
    let ids: std::collections::BTreeSet<_> = known.instances.iter().map(|instance| instance.id.clone()).collect();
    for section in &mut known.sections {
        section.tabs.retain(|id| ids.contains(id));
        if !section.tabs.contains(&section.active)
            && let Some(first) = section.tabs.first()
        {
            section.active = first.clone();
        }
    }
    known.sections.retain(|section| !section.tabs.is_empty());
    validate(&known, registry)
}

fn validate_wire_structure(layout: &Layout) -> PanelResult<()> {
    if layout.version != LAYOUT_VERSION {
        return Err(error(Rule::Version, "unsupported layout version"));
    }
    valid_id(&layout.workspace.0)?;
    if layout.instances.len() > MAX_INSTANCES || layout.sections.len() > MAX_SECTIONS || layout.columns.len() > MAX_SECTIONS {
        return Err(error(Rule::CollectionLimit, "layout collection limit"));
    }
    let mut instances = std::collections::BTreeSet::new();
    let mut sections = std::collections::BTreeSet::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut hosts = std::collections::BTreeSet::new();
    let mut columns = std::collections::BTreeSet::new();
    let mut last_column = std::collections::BTreeMap::<Zone, usize>::new();
    for column in &layout.columns {
        valid_id(&column.id.0)?;
        if !columns.insert(&column.id) {
            return Err(error(Rule::DuplicateId, "duplicate saved column"));
        }
        if !column.zone.side() || !column.width.is_finite() || column.width <= 0.0 || column.width > MAX_LOGICAL {
            return Err(error(Rule::Bounds, "invalid saved column"));
        }
    }
    for instance in &layout.instances {
        valid_id(&instance.id.0)?;
        valid_id(&instance.kind.0)?;
        valid_id(&instance.memory.section.0)?;
        instance.memory.location.validate()?;
        instance.memory.size.validate()?;
        validate_column_memory(
            instance.memory.location.zone(),
            &instance.memory.column,
            instance.memory.column_index,
            instance.memory.column_width,
            instance.memory.weight,
        )?;
        validate_extent_priority(instance.memory.preferred_extent, instance.memory.extent_priority)?;
        if instance.memory.tab_index >= MAX_TABS || instance.memory.section_index > MAX_SECTIONS {
            return Err(error(Rule::Bounds, "invalid placement memory"));
        }
        if !instances.insert(&instance.id) {
            return Err(error(Rule::DuplicateId, "duplicate unknown/known instance"));
        }
    }
    for section in &layout.sections {
        valid_id(&section.id.0)?;
        section.location.validate()?;
        section.size.validate()?;
        validate_section_record(section)?;
        let zone = section.location.zone();
        if let Some(column) = &section.column {
            let rank = layout
                .columns
                .iter()
                .filter(|c| c.zone == zone)
                .position(|c| c.id == *column)
                .ok_or_else(|| error(Rule::Placement, "saved section column absent"))?;
            if last_column.get(&zone).is_some_and(|last| *last > rank) {
                return Err(error(Rule::Placement, "saved column sections are not contiguous"));
            }
            last_column.insert(zone, rank);
        }
        if !sections.insert(&section.id) {
            return Err(error(Rule::DuplicateId, "duplicate unknown/known section"));
        }
        if section.tabs.len() > MAX_TABS {
            return Err(error(Rule::CollectionLimit, "section tab cap"));
        }
        if let SectionLocation::Window { host, .. } = &section.location
            && !hosts.insert(host)
        {
            return Err(error(Rule::DuplicateId, "native host reused"));
        }
        if section.tabs.is_empty() || !section.tabs.contains(&section.active) {
            return Err(error(Rule::ExactlyOnce, "invalid unknown/known active tab"));
        }
        for id in &section.tabs {
            if !instances.contains(id) || !seen.insert(id) {
                return Err(error(Rule::ExactlyOnce, "unknown/known tab ownership invalid"));
            }
        }
    }
    for instance in &layout.instances {
        if instance.requested_open != seen.contains(&instance.id) {
            return Err(error(Rule::ExactlyOnce, "unknown/known open state invalid"));
        }
    }
    Ok(())
}

/// JSON schema for the wire contract. Semantic constraints are enforced by decode
/// and the reducer; schema alone is never treated as a validation proof.
pub fn layout_action_schema() -> schemars::Schema {
    schemars::schema_for!(LayoutAction)
}
pub fn layout_reply_schema() -> schemars::Schema {
    schemars::schema_for!(LayoutReply)
}
pub fn layout_error_schema() -> schemars::Schema {
    schemars::schema_for!(PanelError)
}
pub fn layout_envelope_schema() -> schemars::Schema {
    schemars::schema_for!(LayoutEnvelope)
}

pub fn decode_action(bytes: &[u8]) -> PanelResult<LayoutAction> {
    if bytes.len() > MAX_ENVELOPE_BYTES {
        return Err(error(Rule::CollectionLimit, "action byte cap"));
    }
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| error(Rule::Schema, "invalid action JSON"))?;
    bounded_json(&value)?;
    let action: LayoutAction = serde_json::from_value(value).map_err(|_| error(Rule::Schema, "invalid action shape"))?;
    if let LayoutAction::Apply { mutations, .. } = &action
        && (mutations.is_empty() || mutations.len() > MAX_MUTATIONS)
    {
        return Err(error(Rule::CollectionLimit, "action mutation cap"));
    }
    Ok(action)
}
