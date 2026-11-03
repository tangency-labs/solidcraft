use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::*;

pub const ENVELOPE_FORMAT: &str = "solidcraft.panel-layout";
pub const ENVELOPE_VERSION: u32 = 1;

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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
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
        }
    }
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
    let mut writer = BoundedEnvelopeWriter { bytes: Vec::new() };
    serde_json::to_writer(&mut writer, envelope).map_err(|_| error(Rule::CollectionLimit, "cannot encode panel envelope within byte cap"))?;
    // Aggregated opaque entries must also fit the decoder's global node/depth
    // budget. Never persist bytes that this same version would reject on reopen.
    let value: serde_json::Value = serde_json::from_slice(&writer.bytes).map_err(|_| error(Rule::Schema, "invalid encoded panel envelope"))?;
    bounded_json(&value)?;
    Ok(writer.bytes)
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
    if envelope.quarantine.len() + envelope.layout.instances.len() > MAX_INSTANCES {
        return Err(error(Rule::CollectionLimit, "quarantine instance cap"));
    }
    if envelope.retained_data.len() > MAX_INSTANCES {
        return Err(error(Rule::CollectionLimit, "retained data cap"));
    }
    for (id, data) in &envelope.retained_data {
        valid_id(&id.0)?;
        bounded_json(data)?;
    }
    if envelope.section_snapshots.len() > MAX_SECTIONS {
        return Err(error(Rule::CollectionLimit, "section snapshot cap"));
    }
    validate_section_order(&envelope.section_order)?;
    for (id, snapshot) in &envelope.section_snapshots {
        validate_snapshot(id, snapshot)?;
    }
    let mut ids = std::collections::BTreeSet::new();
    for item in &envelope.quarantine {
        bounded_json(&item.instance_data)?;
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

fn validate_snapshot(id: &SectionId, snapshot: &SectionSnapshot) -> PanelResult<()> {
    valid_id(&id.0)?;
    bounded_json(&snapshot.data)?;
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
) -> PanelResult<()> {
    if envelope.section_snapshots.len() > MAX_SECTIONS || envelope.quarantine.len() > MAX_INSTANCES {
        return Err(error(Rule::CollectionLimit, "section memory collection cap"));
    }
    validate_section_order(&envelope.section_order)?;
    for (id, snapshot) in &envelope.section_snapshots {
        validate_snapshot(id, snapshot)?;
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
        let object = data.as_object_mut().ok_or_else(|| error(Rule::Schema, "section memory must be an object"))?;
        let current = current.as_object().ok_or_else(|| error(Rule::Schema, "saved section must be an object"))?;
        for (key, value) in current {
            object.insert(key.clone(), value.clone());
        }
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

/// Decode bounded bytes, quarantine unknown kinds, and retry retained panels when
/// registered again. Corrupt/future input returns a validated safe preset plus notice.
pub fn decode(bytes: &[u8], registry: &PanelRegistry, fallback: &Layout) -> PanelResult<LoadOutcome> {
    validate(fallback, registry)?;
    match decode_inner(bytes, registry, &fallback.workspace) {
        Ok(envelope) => Ok(LoadOutcome { envelope, notice: None, rejected: None }),
        Err(reason) => Ok(LoadOutcome {
            envelope: LayoutEnvelope::new(fallback.clone()),
            notice: Some(reason),
            rejected: if bytes.len() <= MAX_ENVELOPE_BYTES { Some(bytes.to_vec()) } else { None },
        }),
    }
}

fn decode_inner(bytes: &[u8], registry: &PanelRegistry, workspace: &WorkspaceId) -> PanelResult<LayoutEnvelope> {
    if bytes.len() > MAX_ENVELOPE_BYTES {
        return Err(error(Rule::CollectionLimit, "panel envelope exceeds byte cap"));
    }
    // serde_json's recursion limit remains enabled. Parsing starts only after the
    // byte bound, so oversized arrays/strings cannot allocate without a bound.
    let raw: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| error(Rule::Schema, "invalid panel envelope JSON"))?;
    bounded_json(&raw)?;
    let mut envelope: LayoutEnvelope = serde_json::from_value(raw.clone()).map_err(|_| error(Rule::Schema, "invalid panel envelope shape"))?;
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
    validate_wire_structure(&envelope.layout)?;
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
    refresh_section_memory(&mut envelope, Some(&raw), &capture)?;
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
    validate_envelope(&envelope, registry)?;
    restore_quarantine(&mut envelope, registry)?;
    // Success means the lossless transformed envelope is itself persistable,
    // including its aggregate byte/node/depth budget. Otherwise retain recovery.
    encode(&envelope, registry)?;
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
    validate_envelope(envelope, registry)?;
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
    // Direct consumers may have edited the live layout since the last decode or
    // restore. Merge those edits before using saved ranks/activation again.
    refresh_section_memory(&mut working, None, &std::collections::BTreeSet::new())?;
    // Build a candidate for each entry. An incompatible reappearance stays in
    // quarantine instead of invalidating the known layout or erasing its wire data.
    let mut remaining = Vec::new();
    for item in &envelope.quarantine {
        if registry.get(&item.kind).is_err() || availability.get(&item.instance).is_some_and(|state| *state != Availability::Available) {
            remaining.push(item.clone());
            continue;
        }
        let mut candidate = working.layout.clone();
        let instance: PanelInstance =
            serde_json::from_value(item.instance_data.clone()).map_err(|_| error(Rule::Schema, "invalid quarantined instance"))?;
        candidate.instances.push(instance.clone());
        if instance.requested_open {
            let Some(section_id) = &item.section else {
                remaining.push(item.clone());
                continue;
            };
            let snapshot = working.section_snapshots.get(section_id).ok_or_else(|| error(Rule::MissingTarget, "shared snapshot missing"))?;
            let mut saved: Section = serde_json::from_value(snapshot.data.clone()).map_err(|_| error(Rule::Schema, "invalid quarantined section"))?;
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
    encode(&working, registry)?;
    *envelope = working;
    Ok(())
}

fn validate_wire_structure(layout: &Layout) -> PanelResult<()> {
    let mut instances = std::collections::BTreeSet::new();
    let mut sections = std::collections::BTreeSet::new();
    let mut seen = std::collections::BTreeSet::new();
    for instance in &layout.instances {
        valid_id(&instance.id.0)?;
        valid_id(&instance.kind.0)?;
        if !instances.insert(&instance.id) {
            return Err(error(Rule::DuplicateId, "duplicate unknown/known instance"));
        }
    }
    for section in &layout.sections {
        valid_id(&section.id.0)?;
        section.location.validate()?;
        section.size.validate()?;
        if !sections.insert(&section.id) {
            return Err(error(Rule::DuplicateId, "duplicate unknown/known section"));
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
