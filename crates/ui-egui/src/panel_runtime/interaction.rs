//! Transient pointer capture. This module prepares candidates but never dispatches
//! or commits them: the retained runtime owns lifecycle/native side effects.

use solidcraft_engine::panels::*;

use super::LocalScope;
use super::layout::{
    HostContentMetrics, HostLayout, HostMetrics, bottom, next_host_column_id, next_host_section_id, rect, resolve_host_prepared_with_snapshot,
    resolve_host_projected, resolve_host_projected_with_snapshot, right,
};

pub const HOST_DRAG_THRESHOLD: f32 = 6.0;

const SIDE_DOCK_BAND: f32 = 16.0;
const EMPTY_SIDE_EXTRA: f32 = 40.0;
const BOTTOM_DOCK_BAND: f32 = 64.0;
const COLUMN_OUTWARD: f32 = 18.0;
const CENTRE_COLUMN_OUTWARD: f32 = 64.0;

/// Resize target edges are logical for docks and physical for floating cards.
/// Pointer movement and rendered bounds use the corresponding physical edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HostResizeEdge {
    Left,
    Right,
    Top,
    Bottom,
    BottomRight,
}

impl HostResizeEdge {
    /// Dock targets retain their logical identity when the workspace mirrors.
    /// Floating corners remain physical because floating rects do not mirror.
    pub(super) fn physical(self, zone: Zone, direction: Direction) -> Self {
        if zone.docked() && direction == Direction::Rtl {
            match self {
                Self::Left => Self::Right,
                Self::Right => Self::Left,
                other => other,
            }
        } else {
            self
        }
    }
}

/// Combine this typed identity with the owner/window-local scope for egui IDs.
/// Titles, coordinates, section ordering and short wire ControlIds are not IDs.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum HostPointerTarget {
    Header {
        instance: InstanceId,
    },
    Tab {
        instance: InstanceId,
    },
    /// Legacy semantic identity; no rail is allocated or handled.
    Rail {
        instance: InstanceId,
    },
    Collapse {
        instance: InstanceId,
    },
    Close {
        instance: InstanceId,
    },
    HeaderAction {
        instance: InstanceId,
        action: ControlId,
    },
    Surface {
        instance: InstanceId,
    },
    Menu {
        instance: InstanceId,
    },
    TabOverflow {
        section: SectionId,
    },
    Sash {
        first: SectionId,
        second: SectionId,
    },
    Resize {
        section: SectionId,
        edge: HostResizeEdge,
    },
    Overflow {
        zone: Zone,
    },
    OverflowAll,
}

impl HostPointerTarget {
    pub fn validate(&self) -> PanelResult<()> {
        match self {
            Self::Header { instance }
            | Self::Tab { instance }
            | Self::Rail { instance }
            | Self::Surface { instance }
            | Self::Menu { instance }
            | Self::Collapse { instance }
            | Self::Close { instance } => valid_id(&instance.0),
            Self::HeaderAction { instance, action } => {
                valid_id(&instance.0)?;
                valid_id(&action.0)
            }
            Self::TabOverflow { section } | Self::Resize { section, .. } => valid_id(&section.0),
            Self::Sash { first, second } => {
                valid_id(&first.0)?;
                valid_id(&second.0)?;
                if first == second {
                    return Err(error(Rule::Placement, "sash targets must be distinct"));
                }
                Ok(())
            }
            Self::Overflow { .. } | Self::OverflowAll => Ok(()),
        }
    }
}

impl std::hash::Hash for HostPointerTarget {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::Header { instance }
            | Self::Tab { instance }
            | Self::Rail { instance }
            | Self::Surface { instance }
            | Self::Menu { instance }
            | Self::Collapse { instance }
            | Self::Close { instance } => instance.hash(state),
            Self::HeaderAction { instance, action } => {
                instance.hash(state);
                action.hash(state);
            }
            Self::TabOverflow { section } => section.hash(state),
            Self::Sash { first, second } => {
                first.hash(state);
                second.hash(state);
            }
            Self::Resize { section, edge } => {
                section.hash(state);
                edge.hash(state);
            }
            Self::Overflow { zone } => (*zone as u8).hash(state),
            Self::OverflowAll => {}
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum HostPointerEvent {
    Press {
        at: Point,
        target: Option<HostPointerTarget>,
    },
    Move {
        at: Point,
    },
    Release {
        at: Point,
    },
    DoubleClick {
        target: HostPointerTarget,
    },
    Escape,
    Cancel,
    /// Transient pointer absence while the primary button remains held. Keep
    /// capture, but require a fresh Move before any release can commit.
    Leave,
    /// Revalidate capture on a frame with no pointer movement.
    Reconcile,
    /// Panel-owned popovers count as inside. Dismissal never consumes outside
    /// input, so the existing navigation path can use that same press.
    OutsidePress {
        at: Point,
        inside_owned_overlay: bool,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum HostDropTargetId {
    Dock { zone: Zone, before: Option<SectionId> },
    Tab { section: SectionId, before: Option<InstanceId> },
    Relative { section: SectionId, placement: RelativePlacement },
}

#[derive(Clone, Debug, PartialEq)]
pub struct HostDropTarget {
    pub id: HostDropTargetId,
    /// Explicit discoverable hover region in the committed layout.
    pub bounds: LogicalRect,
    pub destination: Destination,
    pub rejection: Option<PanelError>,
    pub priority: u8,
    /// (horizontal axis, physical edge coordinate).
    pub edge: Option<(bool, f32)>,
    pub stable_key: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HostPreview {
    pub action: Option<LayoutAction>,
    pub bounds: LogicalRect,
    pub destination: Option<Destination>,
    pub rejection: Option<PanelError>,
    pub projected: Option<HostLayout>,
    pub target: Option<HostDropTargetId>,
    pub targets: Vec<HostDropTarget>,
    pub tab_marker: Option<LogicalRect>,
}

impl HostPreview {
    fn empty(bounds: LogicalRect) -> Self {
        Self { action: None, bounds, destination: None, rejection: None, projected: None, target: None, targets: Vec::new(), tab_marker: None }
    }
}

#[derive(Debug, Default)]
pub struct HostInteractionUpdate {
    pub candidate: Option<PreparedHostLayout>,
    pub preview: Option<HostPreview>,
    pub action: Option<LayoutAction>,
    pub rejection: Option<PanelError>,
    pub captured: bool,
    pub consumed: bool,
}

/// Captured exactly once for the proposed owner cohort, before final allocation.
#[derive(Clone, Debug)]
pub struct HostProposalSnapshot {
    pub availability: AvailabilityMap,
    pub metrics: std::sync::Arc<dyn HostContentMetrics>,
}

/// Adapter for synthetic callers with exact captured measurements but no provider.
/// It never guesses a new width and never measures a hidden/stretched body.
#[derive(Debug)]
struct CapturedHostMetrics {
    revision: u64,
    content: Vec<SideContentMeasure>,
}
impl HostContentMetrics for CapturedHostMetrics {
    fn revision(&self) -> u64 {
        self.revision
    }
    fn measure(&self, request: &ContentMeasureRequest) -> PanelResult<SideContentMeasure> {
        self.content
            .iter()
            .find(|measure| measure.request == *request)
            .cloned()
            .ok_or_else(|| error(Rule::Revision, "captured owner metrics have no requested width"))
    }
}
fn current_owner_snapshot(service: &LayoutService, reference: &HostLayout, proposed: &Layout) -> HostProposalSnapshot {
    let metrics = reference.geometry_policy.measure.clone().unwrap_or_else(|| {
        std::sync::Arc::new(CapturedHostMetrics {
            revision: reference.allocation_environment.content_revision,
            content: reference.allocation_environment.content.clone(),
        })
    });
    HostProposalSnapshot {
        availability: proposed.instances.iter().map(|instance| (instance.id.clone(), service.availability(&instance.id))).collect(),
        metrics,
    }
}

/// One owned engine proposal plus its exact admitted host geometry. The engine
/// state cannot be forged, cloned or reconstructed from a preview action.
pub struct PreparedHostLayout {
    prepared: PreparedLayout,
    snapshot: HostProposalSnapshot,
    action: LayoutAction,
    layout: HostLayout,
    reference: HostLayout,
    scope: LocalScope,
}
impl std::fmt::Debug for PreparedHostLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedHostLayout").field("layout", &self.layout).field("scope", &self.scope).finish_non_exhaustive()
    }
}
impl PreparedHostLayout {
    pub fn layout(&self) -> &HostLayout {
        &self.layout
    }
    pub fn prepared(&self) -> &PreparedLayout {
        &self.prepared
    }
    pub fn proposal_snapshot(&self) -> &HostProposalSnapshot {
        &self.snapshot
    }
    /// Original intent for lifecycle preflight only; commit uses into_prepared.
    pub fn action(&self) -> &LayoutAction {
        &self.action
    }
    pub fn validate_current(&self, service: &LayoutService, scope: &LocalScope, reference: &HostLayout) -> PanelResult<()> {
        validate_context(service, scope, reference)?;
        if self.scope != *scope || self.reference != *reference {
            return Err(error(Rule::Revision, "prepared host environment or scope changed"));
        }
        self.layout.validate()?;
        if self.snapshot.metrics.revision() != self.layout.allocation_environment.content_revision {
            return Err(error(Rule::Revision, "proposed owner metric snapshot became stale"));
        }
        let available: Vec<_> = self
            .prepared
            .layout()
            .instances
            .iter()
            .filter(|instance| self.snapshot.availability.get(&instance.id).is_none_or(|state| *state == Availability::Available))
            .map(|instance| instance.id.clone())
            .collect();
        if available != self.layout.allocation_environment.available {
            return Err(error(Rule::Revision, "proposed availability differs from admitted geometry"));
        }
        Ok(())
    }
    pub fn into_prepared(self, service: &LayoutService, scope: &LocalScope, reference: &HostLayout) -> PanelResult<PreparedLayout> {
        self.validate_current(service, scope, reference)?;
        Ok(self.prepared)
    }
}

/// Every command and gesture calls this admission path before runtime lifecycle
/// preflight. It has no controller, preference, document or ID-allocation effects.
pub fn prepare_host_layout(
    service: &LayoutService,
    scope: &LocalScope,
    reference: &HostLayout,
    action: LayoutAction,
) -> PanelResult<PreparedHostLayout> {
    prepare_host_layout_with_snapshot(service, scope, reference, action, |proposed| Ok(current_owner_snapshot(service, reference, proposed)))
}

/// One pure owner/topology projection, one capture, then numeric normalization
/// and final admission. C retains that cohort through every prefix and commit.
pub fn prepare_host_layout_with_snapshot(
    service: &LayoutService,
    scope: &LocalScope,
    reference: &HostLayout,
    action: LayoutAction,
    capture: impl FnOnce(&Layout) -> PanelResult<HostProposalSnapshot>,
) -> PanelResult<PreparedHostLayout> {
    validate_context(service, scope, reference)?;
    let exempt = matches!(&action, LayoutAction::Apply { mutations, .. } if !mutations.is_empty() && mutations.iter().all(|mutation|
        matches!(mutation, Mutation::Close { .. } | Mutation::Collapse { .. } | Mutation::Focus { .. } | Mutation::Activate { .. } | Mutation::DismissPeek))
        && mutations.iter().any(|mutation| !matches!(mutation, Mutation::DismissPeek)));
    let owners = project_host_action(service, reference, &action)?;
    let snapshot = capture(&owners)?;
    if snapshot.availability.len() > MAX_INSTANCES {
        return Err(error(Rule::CollectionLimit, "proposed availability cap"));
    }
    for (id, state) in &snapshot.availability {
        valid_id(&id.0)?;
        owners.instance(id)?;
        if let Availability::Unavailable { reason } = state
            && (reason.is_empty() || reason.len() > 256)
        {
            return Err(error(Rule::Bounds, "proposed availability reason bound"));
        }
    }
    // Refuse unavailable initiators before a numeric prefix can fail on their
    // missing allocation. The private owner projection deliberately defers this
    // check until the proposed owner's availability has been captured once.
    if let LayoutAction::Apply { mutations, .. } = &action {
        for mutation in mutations {
            if let Mutation::Open { instance, .. }
            | Mutation::Move { instance, .. }
            | Mutation::Activate { instance }
            | Mutation::Focus { instance }
            | Mutation::Peek { instance }
            | Mutation::HeaderAction { instance, .. } = mutation
                && let Some(state) = snapshot.availability.get(instance)
            {
                state.require()?;
            }
        }
    }
    let normalized = normalize_host_action(service, reference, &action, &snapshot)?;
    let proposed = project_captured(service, reference, normalized.action.clone(), &snapshot)?;
    if !same_owner_topology(&owners, &proposed) {
        return Err(error(Rule::Revision, "numeric normalization changed captured owner topology"));
    }
    let context = reference.geometry_policy.identity_reservations.preparation_context()?;
    let prepared = service.prepare_with_context(normalized.action.clone(), &snapshot.availability, &context)?;
    if prepared.layout() != &proposed || prepared.layout().extent_priority_context != proposed.extent_priority_context {
        return Err(error(Rule::Revision, "captured availability changed the structural projection"));
    }
    let layout = resolve_host_prepared_with_snapshot(service, &prepared, reference, &snapshot)?;
    // Final allocation remains authoritative even after cohort capture.
    if let Some(expected) = &normalized.pair_geometry
        && (expected.extent != layout.extent
            || expected.sections.len() != layout.sections.len()
            || expected.sections.iter().any(|old| !layout.sections.iter().any(|new| new.section == old.section && near_rect(old.outer, new.outer))))
    {
        return Err(error(Rule::Bounds, "captured allocation changed the admitted pair geometry"));
    }
    for section in &prepared.layout().sections {
        if section.location.zone() != Zone::Floating {
            continue;
        }
        if presented_instance(section, &layout.allocation_environment.available).is_none() {
            continue;
        }
        let old = service.layout().section(&section.id).ok();
        let changed = old.is_none_or(|old| old.location != section.location || old.size != section.size || old.tabs != section.tabs);
        if changed && !layout.sections.iter().any(|card| card.section == section.id && intersect_rect(card.header, layout.capacity).is_some()) {
            return Err(error(Rule::NoSpace, "floating landing has no accessible clipped header"));
        }
    }
    let extent = layout.extent;
    let overflow = extent.width > layout.capacity.size.width || extent.height > layout.capacity.size.height;
    let before = reference.extent;
    let improves = extent.width <= before.width && extent.height <= before.height && (extent.width < before.width || extent.height < before.height);
    if overflow && !exempt && !improves {
        return Err(error(Rule::NoSpace, "candidate does not strictly improve existing overflow"));
    }
    Ok(PreparedHostLayout { prepared, snapshot, action, layout, reference: reference.clone(), scope: scope.clone() })
}

fn project_reserved(service: &LayoutService, reference: &HostLayout, action: LayoutAction) -> PanelResult<Layout> {
    service.project_with_context(action, &reference.geometry_policy.identity_reservations.preparation_context()?)
}

fn project_captured(service: &LayoutService, reference: &HostLayout, action: LayoutAction, snapshot: &HostProposalSnapshot) -> PanelResult<Layout> {
    Ok(service
        .prepare_with_context(action, &snapshot.availability, &reference.geometry_policy.identity_reservations.preparation_context()?)?
        .layout()
        .clone())
}

fn same_owner_topology(a: &Layout, b: &Layout) -> bool {
    let location = |a: &SectionLocation, b: &SectionLocation| match (a, b) {
        (SectionLocation::Dock { zone: a }, SectionLocation::Dock { zone: b }) => a == b,
        (SectionLocation::Floating { origin: a }, SectionLocation::Floating { origin: b }) => a == b,
        (SectionLocation::Window { host: a, origin: x, .. }, SectionLocation::Window { host: b, origin: y, .. }) => a == b && x == y,
        _ => false,
    };
    a.version == b.version
        && a.workspace == b.workspace
        && a.columns == b.columns
        && a.instances.len() == b.instances.len()
        && a.sections.len() == b.sections.len()
        && a.instances.iter().zip(&b.instances).all(|(a, b)| a.id == b.id && a.kind == b.kind && a.requested_open == b.requested_open)
        && a.sections.iter().zip(&b.sections).all(|(a, b)| {
            a.id == b.id
                && location(&a.location, &b.location)
                && a.tabs == b.tabs
                && a.active == b.active
                && a.collapsed == b.collapsed
                && a.column == b.column
        })
}

fn geometry_tolerance(values: &[f32]) -> f32 {
    values.iter().fold(1.0_f32, |largest, value| largest.max(value.abs())) * f32::EPSILON * 8.0
}
fn axis(zone: Zone, size: Size) -> f32 {
    if zone == Zone::Bottom { size.width } else { size.height }
}
fn with_axis(zone: Zone, mut size: Size, extent: f32) -> Size {
    if zone == Zone::Bottom {
        size.width = extent;
    } else {
        size.height = extent;
    }
    size
}
fn near_rect(a: LogicalRect, b: LogicalRect) -> bool {
    let tolerance = geometry_tolerance(&[a.origin.x, a.origin.y, b.origin.x, b.origin.y, a.size.width, a.size.height, b.size.width, b.size.height]);
    (a.origin.x - b.origin.x).abs() <= tolerance
        && (a.origin.y - b.origin.y).abs() <= tolerance
        && (a.size.width - b.size.width).abs() <= tolerance
        && (a.size.height - b.size.height).abs() <= tolerance
}

struct AllocatedPair<'a> {
    first: &'a super::layout::HostSection,
    second: &'a super::layout::HostSection,
    zone: Zone,
    total: f32,
    lower: f32,
    upper: f32,
    first_min: f32,
    second_min: f32,
}

fn allocated_pair<'a>(
    state: &Layout,
    service: &LayoutService,
    host: &'a HostLayout,
    first: &SectionId,
    second: &SectionId,
) -> PanelResult<AllocatedPair<'a>> {
    let a = state.section(first)?;
    let b = state.section(second)?;
    let zone = a.location.zone();
    if !zone.docked() || b.location.zone() != zone || a.column != b.column || a.collapsed || b.collapsed {
        return Err(error(Rule::Placement, "allocated sash requires one expanded dock group"));
    }
    let presented: Vec<_> = host
        .sections
        .iter()
        .filter(|card| card.zone == zone && state.section(&card.section).is_ok_and(|section| section.column == a.column))
        .collect();
    let pair = presented
        .windows(2)
        .find(|pair| pair.first().is_some_and(|card| card.section == *first) && pair.get(1).is_some_and(|card| card.section == *second))
        .ok_or_else(|| error(Rule::Placement, "allocated sash requires presented adjacency"))?;
    let first = pair.first().copied().ok_or_else(|| error(Rule::MissingTarget, "first allocated neighbour absent"))?;
    let second = pair.get(1).copied().ok_or_else(|| error(Rule::MissingTarget, "second allocated neighbour absent"))?;
    let (a_min, a_max) = effective_allocation_bounds(state, service.registry(), a, &host.allocation_environment)?;
    let (b_min, b_max) = effective_allocation_bounds(state, service.registry(), b, &host.allocation_environment)?;
    let total = axis(zone, first.outer.size) + axis(zone, second.outer.size);
    let lower = f64::from(axis(zone, a_min)).max(f64::from(total) - f64::from(axis(zone, b_max)));
    let upper = f64::from(axis(zone, a_max)).min(f64::from(total) - f64::from(axis(zone, b_min)));
    let mut lo = lower as f32;
    let mut hi = upper as f32;
    if f64::from(lo) < lower {
        lo = f32::from_bits(lo.to_bits().saturating_add(1));
    }
    if f64::from(hi) > upper {
        hi = f32::from_bits(hi.to_bits().saturating_sub(1));
    }
    if !total.is_finite() || lo > hi {
        return Err(error(Rule::Bounds, "allocated pair has no feasible interval"));
    }
    Ok(AllocatedPair { first, second, zone, total, lower: lo, upper: hi, first_min: axis(zone, a_min), second_min: axis(zone, b_min) })
}

fn requested_pair_axis(pair: &AllocatedPair<'_>, a: Size, b: Size) -> PanelResult<f32> {
    a.validate()?;
    b.validate()?;
    let tolerance = geometry_tolerance(&[pair.total]);
    let cross = |size: Size| if pair.zone == Zone::Bottom { size.height } else { size.width };
    if (axis(pair.zone, a) + axis(pair.zone, b) - pair.total).abs() > tolerance
        || (cross(a) - cross(pair.first.outer.size)).abs() > tolerance
        || (cross(b) - cross(pair.second.outer.size)).abs() > tolerance
    {
        return Err(error(Rule::Bounds, "pair request must conserve actual extent and cross axes"));
    }
    Ok(axis(pair.zone, a))
}

fn normalized_allocated_pair(
    service: &LayoutService,
    state: &Layout,
    host: &HostLayout,
    first: &SectionId,
    second: &SectionId,
    requested: Option<(Size, Size)>,
) -> PanelResult<Mutation> {
    let pair = allocated_pair(state, service, host, first, second)?;
    let wanted = if let Some((a, b)) = requested {
        requested_pair_axis(&pair, a, b)?
    } else {
        let a = service.registry().get(&state.instance(&pair.first.instance)?.kind)?.default_size;
        let b = service.registry().get(&state.instance(&pair.second.instance)?.kind)?.default_size;
        (f64::from(pair.total) * f64::from(axis(pair.zone, a)) / (f64::from(axis(pair.zone, a)) + f64::from(axis(pair.zone, b)))) as f32
    };
    let split = wanted.clamp(pair.lower, pair.upper);
    let first_size = with_axis(pair.zone, pair.first.outer.size, split);
    let second_size = with_axis(pair.zone, pair.second.outer.size, pair.total - split);
    let (first_weight, second_weight, other_weight_scale) = effective_pair_weights(
        service,
        state,
        host,
        first,
        second,
        (f64::from(split) - f64::from(pair.first_min)).max(0.0),
        (f64::from(pair.total - split) - f64::from(pair.second_min)).max(0.0),
    )?;
    Ok(Mutation::ResizeAllocatedPair {
        first: first.clone(),
        second: second.clone(),
        first_size,
        second_size,
        first_weight,
        second_weight,
        other_weight_scale,
        prefer_allocated: false,
    })
}

/// Infer the constrained allocator's surplus unit, not the latent total of
/// capped weights. Scaling all unpaired saved records preserves their ratios.
fn effective_pair_weights(
    service: &LayoutService,
    state: &Layout,
    host: &HostLayout,
    first: &SectionId,
    second: &SectionId,
    first_surplus: f64,
    second_surplus: f64,
) -> PanelResult<(f32, f32, f32)> {
    let anchor = state.section(first)?;
    let zone = anchor.location.zone();
    let (old_first, old_second) = normalized_pair_weights(state, first, second)?;
    if first_surplus + second_surplus <= 0.0 {
        // At an all-minimum allocation, retaining positive saved weights is
        // harmless and avoids inventing a degenerate all-zero saved group.
        return Ok((old_first, old_second, 1.0));
    }
    let mut outside_surplus = 0.0_f64;
    let mut outside_coefficients = 0.0_f64;
    let mut pair_surplus = 0.0_f64;
    let mut pair_coefficients = 0.0_f64;
    let mut capped_lower_unit = 0.0_f64;
    for card in &host.sections {
        let member = state.section(&card.section)?;
        if card.zone != zone || member.column != anchor.column || member.collapsed || member.preferred_extent.is_some() {
            continue;
        }
        let paired = member.id == *first || member.id == *second;
        let coefficient = if member.id == *first {
            old_first
        } else if member.id == *second {
            old_second
        } else {
            normalized_pair_weights(state, first, &member.id)?.1
        };
        if coefficient <= 0.0 {
            continue;
        }
        let (min, max) = effective_allocation_bounds(state, service.registry(), member, &host.allocation_environment)?;
        let actual = axis(zone, card.outer.size);
        let surplus = (f64::from(actual) - f64::from(axis(zone, min))).max(0.0);
        if actual < axis(zone, max) && surplus > 0.0 {
            if paired {
                pair_surplus += surplus;
                pair_coefficients += f64::from(coefficient);
            } else {
                outside_surplus += surplus;
                outside_coefficients += f64::from(coefficient);
            }
        } else if !paired {
            capped_lower_unit = capped_lower_unit.max(surplus / f64::from(coefficient));
        }
    }
    let unit = if outside_coefficients > 0.0 {
        outside_surplus / outside_coefficients
    } else if pair_coefficients > 0.0 {
        pair_surplus / pair_coefficients
    } else {
        capped_lower_unit.max(1.0)
    };
    let unit = unit.max(capped_lower_unit);
    let denominator = unit.max(first_surplus).max(second_surplus);
    let a = (first_surplus / denominator) as f32;
    let b = (second_surplus / denominator) as f32;
    let scale = (unit / denominator) as f32;
    if !denominator.is_finite()
        || denominator <= 0.0
        || !a.is_finite()
        || !b.is_finite()
        || !(0.0..=1.0).contains(&a)
        || !(0.0..=1.0).contains(&b)
        || !scale.is_finite()
        || scale <= 0.0
        || scale > MAX_LOGICAL
    {
        return Err(error(Rule::Bounds, "actual pair cannot be represented by bounded saved weights"));
    }
    Ok((a, b, scale))
}

fn validate_allocated_pair(before: &HostLayout, after: &HostLayout, mutation: &Mutation) -> PanelResult<()> {
    let Mutation::ResizeAllocatedPair { first, second, first_size, second_size, .. } = mutation else {
        return Ok(());
    };
    for (id, size) in [(first, *first_size), (second, *second_size)] {
        let old = before.sections.iter().find(|card| card.section == *id).ok_or_else(|| error(Rule::MissingTarget, "old pair member absent"))?;
        let new = after.sections.iter().find(|card| card.section == *id).ok_or_else(|| error(Rule::MissingTarget, "new pair member absent"))?;
        if !near_rect(LogicalRect { origin: new.outer.origin, size }, new.outer) {
            return Err(error(Rule::Bounds, "saved shares cannot represent requested actual pair sizes"));
        }
        let cross = |size: Size| if old.zone == Zone::Bottom { size.height } else { size.width };
        if (cross(old.outer.size) - cross(new.outer.size)).abs() > geometry_tolerance(&[cross(old.outer.size)]) {
            return Err(error(Rule::Bounds, "allocated pair changes actual cross axis"));
        }
    }
    for old in &before.sections {
        if old.section != *first
            && old.section != *second
            && !after.sections.iter().any(|new| new.section == old.section && near_rect(old.outer, new.outer))
        {
            return Err(error(Rule::Bounds, "allocated pair would move an unrelated neighbour"));
        }
    }
    let envelope = |host: &HostLayout| -> PanelResult<LogicalRect> {
        let a =
            host.sections.iter().find(|card| card.section == *first).ok_or_else(|| error(Rule::MissingTarget, "first pair envelope absent"))?.outer;
        let b =
            host.sections.iter().find(|card| card.section == *second).ok_or_else(|| error(Rule::MissingTarget, "second pair envelope absent"))?.outer;
        let x = a.origin.x.min(b.origin.x);
        let y = a.origin.y.min(b.origin.y);
        Ok(rect(x, y, right(a).max(right(b)) - x, bottom(a).max(bottom(b)) - y))
    };
    if !near_rect(envelope(before)?, envelope(after)?) {
        return Err(error(Rule::Bounds, "allocated pair changes its anchored actual envelope"));
    }
    if before.extent != after.extent {
        return Err(error(Rule::Bounds, "allocated pair changes scroll extent"));
    }
    Ok(())
}

struct NormalizedHostAction {
    action: LayoutAction,
    pair_geometry: Option<HostLayout>,
}

/// Normalize public/gesture/reset intent against current allocated rectangles.
/// Each prefix becomes the current geometry for a subsequent overlapping edit.
/// The original action stays separately owned for lifecycle preflight.
fn normalize_host_action(
    service: &LayoutService,
    reference: &HostLayout,
    action: &LayoutAction,
    snapshot: &HostProposalSnapshot,
) -> PanelResult<NormalizedHostAction> {
    let LayoutAction::Apply { expected_revision, mutations } = action else {
        return Ok(NormalizedHostAction { action: action.clone(), pair_geometry: None });
    };
    if mutations.len() > MAX_MUTATIONS {
        return Err(error(Rule::CollectionLimit, "host mutation cap"));
    }
    if !mutations
        .iter()
        .any(|mutation| matches!(mutation, Mutation::ResizePair { .. } | Mutation::ResetPair { .. } | Mutation::ResizeAllocatedPair { .. }))
    {
        return Ok(NormalizedHostAction { action: action.clone(), pair_geometry: None });
    }
    let mut normalized = Vec::new();
    let mut state = service.layout().clone();
    let mut host = resolve_host_projected_with_snapshot(service, &state, reference, snapshot)?;
    for mutation in mutations {
        let next = match mutation {
            Mutation::ResizePair { first, second, first_size, second_size } => {
                normalized_allocated_pair(service, &state, &host, first, second, Some((*first_size, *second_size)))?
            }
            Mutation::ResetPair { first, second } => normalized_allocated_pair(service, &state, &host, first, second, None)?,
            Mutation::ResizeAllocatedPair { first, second, first_size, second_size, first_weight, second_weight, other_weight_scale, .. } => {
                let pair = allocated_pair(&state, service, &host, first, second)?;
                let requested = requested_pair_axis(&pair, *first_size, *second_size)?;
                if requested < pair.lower
                    || requested > pair.upper
                    || !first_weight.is_finite()
                    || !second_weight.is_finite()
                    || !(0.0..=1.0).contains(first_weight)
                    || !(0.0..=1.0).contains(second_weight)
                    || !other_weight_scale.is_finite()
                    || *other_weight_scale <= 0.0
                    || *other_weight_scale > MAX_LOGICAL
                {
                    return Err(error(Rule::Bounds, "invalid explicit allocated pair plan"));
                }
                // Common-factor-equivalent plans are valid: actual forward
                // allocation proves the requested split and untouched neighbours.
                mutation.clone()
            }
            other => other.clone(),
        };
        normalized.push(next.clone());
        let mut proposed = project_captured(
            service,
            reference,
            LayoutAction::Apply { expected_revision: *expected_revision, mutations: normalized.clone() },
            snapshot,
        )?;
        let mut projected = resolve_host_projected_with_snapshot(service, &proposed, reference, snapshot)?;
        if let Mutation::ResizeAllocatedPair { first, prefer_allocated: false, .. } = &next {
            let anchor = state.section(first)?;
            let explicit_group = state.sections.iter().any(|member| {
                member.location.zone() == anchor.location.zone() && member.column == anchor.column && member.preferred_extent.is_some()
            });
            if explicit_group {
                // Preserve explicit pair intent even when weights happen to
                // reproduce today's allocation. Reuse the captured cohort.
                if let Some(Mutation::ResizeAllocatedPair { prefer_allocated, .. }) = normalized.last_mut() {
                    *prefer_allocated = true;
                }
                proposed = project_captured(
                    service,
                    reference,
                    LayoutAction::Apply { expected_revision: *expected_revision, mutations: normalized.clone() },
                    snapshot,
                )?;
                projected = resolve_host_projected_with_snapshot(service, &proposed, reference, snapshot)?;
            }
        }
        validate_allocated_pair(&host, &projected, &next)?;
        host = projected;
        // Capture already occurred; final admission retains this same snapshot.
        state = proposed;
    }
    Ok(NormalizedHostAction {
        action: LayoutAction::Apply { expected_revision: *expected_revision, mutations: normalized },
        pair_geometry: Some(host),
    })
}

/// Owner/topology staging only, before numeric pair normalization and capture.
/// Omitted pair dimensions never create owners. This is not geometry admission.
pub fn project_host_action(service: &LayoutService, reference: &HostLayout, action: &LayoutAction) -> PanelResult<Layout> {
    reference.validate()?;
    if reference.source_state != service.inspect()
        || reference.source_state.layout.extent_priority_context != service.layout().extent_priority_context
        || reference.registry_specs.iter().ne(service.registry().specs())
    {
        return Err(error(Rule::Revision, "host projection source changed"));
    }
    let topology = if let LayoutAction::Apply { expected_revision, mutations } = action {
        if mutations.is_empty() || mutations.len() > MAX_MUTATIONS {
            return Err(error(Rule::CollectionLimit, "host mutation cap"));
        }
        if expected_revision.is_some_and(|revision| revision != service.inspect().revision) {
            return Err(error(Rule::Revision, "stale host owner projection"));
        }
        validate_header_action_transaction(mutations)?;
        let owners: Vec<_> = mutations
            .iter()
            .filter(|mutation| !matches!(mutation, Mutation::ResizePair { .. } | Mutation::ResetPair { .. } | Mutation::ResizeAllocatedPair { .. }))
            .cloned()
            .collect();
        if owners.is_empty() { LayoutAction::Inspect } else { LayoutAction::Apply { expected_revision: *expected_revision, mutations: owners } }
    } else {
        action.clone()
    };
    project_reserved(service, reference, topology)
}

#[derive(Clone, Debug)]
enum GestureKind {
    Move { instance: InstanceId, section: SectionId, click_activates: bool, targets: Vec<HostDropTarget> },
    Resize { section: SectionId, edge: HostResizeEdge, size: Size, outer: LogicalRect },
    Pair { first: SectionId, second: SectionId, first_size: Size, second_size: Size, zone: Zone, outer: LogicalRect },
}

#[derive(Clone, Debug)]
struct Gesture {
    kind: GestureKind,
    scope: LocalScope,
    revision: u64,
    direction: Direction,
    capacity: LogicalRect,
    metrics: HostMetrics,
    render_domain: LogicalRect,
    chrome_obstacles: Vec<LogicalRect>,
    policy: super::layout::HostGeometryPolicy,
    reference: HostLayout,
    pressed_target: HostPointerTarget,
    start: Point,
    dragging: bool,
}

#[derive(Debug, Default)]
pub struct HostInteraction {
    candidate: Option<PreparedHostLayout>,
    last_position: Option<Point>,
    capture: Option<Gesture>,
    preview: Option<HostPreview>,
}

impl HostInteraction {
    pub fn is_captured(&self) -> bool {
        self.capture.is_some()
    }

    pub fn preview(&self) -> Option<&HostPreview> {
        self.preview.as_ref()
    }

    /// Identity feedback follows the held Move, independently of drop admission.
    /// A suspended drag retains its source but has no current cursor position.
    pub(crate) fn move_source(&self) -> Option<&InstanceId> {
        match &self.capture {
            Some(Gesture { kind: GestureKind::Move { instance, .. }, dragging: true, .. }) => Some(instance),
            _ => None,
        }
    }

    pub(crate) fn move_position(&self) -> Option<Point> {
        self.move_source().and(self.last_position)
    }

    pub fn cancel(&mut self) {
        self.capture = None;
        self.suspend();
    }

    fn suspend(&mut self) {
        if let Some(Gesture { kind: GestureKind::Move { click_activates, .. }, .. }) = &mut self.capture {
            *click_activates = false;
        }
        self.preview = None;
        self.candidate = None;
        self.last_position = None;
    }

    /// Independent commands invalidate even a preview whose command refuses.
    /// Hosts without a live capture call prepare_host_layout directly.
    pub fn prepare_command(
        &mut self,
        service: &LayoutService,
        scope: &LocalScope,
        reference: &HostLayout,
        action: LayoutAction,
    ) -> PanelResult<PreparedHostLayout> {
        self.cancel();
        prepare_host_layout(service, scope, reference, action)
    }

    /// Call with live service/scope on each input event, including idle captured
    /// frames. Runtime preflights and consumes the owned release candidate.
    pub fn update(&mut self, service: &LayoutService, scope: &LocalScope, layout: &HostLayout, event: HostPointerEvent) -> HostInteractionUpdate {
        if matches!(event, HostPointerEvent::Cancel) {
            self.cancel();
            return HostInteractionUpdate::default();
        }
        if let Err(reason) = validate_context(service, scope, layout) {
            self.cancel();
            return rejected(reason, false);
        }
        if let Some(capture) = &self.capture
            && let Err(reason) = validate_capture(capture, service, scope, layout)
        {
            self.cancel();
            return rejected(reason, false);
        }
        match event {
            HostPointerEvent::Cancel => HostInteractionUpdate::default(),
            HostPointerEvent::Leave => {
                self.suspend();
                self.current(false)
            }
            HostPointerEvent::Reconcile => self.current(false),
            HostPointerEvent::Escape => {
                let consumed = self.is_captured();
                self.cancel();
                HostInteractionUpdate { consumed, ..Default::default() }
            }
            HostPointerEvent::OutsidePress { at, inside_owned_overlay } => {
                if let Err(reason) = at.validate() {
                    self.cancel();
                    return rejected(reason, false);
                }
                if !inside_owned_overlay {
                    self.cancel();
                }
                self.current(false)
            }
            HostPointerEvent::Press { at, target } => {
                self.cancel();
                if let Err(reason) = at.validate() {
                    return rejected(reason, false);
                }
                let target = target.or_else(|| layout.hit_test(at));
                if let Some(target) = &target
                    && !layout.control(target).is_some_and(|control| control.bounds.contains(at))
                {
                    return rejected(error(Rule::MissingTarget, "pointer target is not under the press"), false);
                }
                let Some(target) = target else {
                    return HostInteractionUpdate::default();
                };
                let kind = match begin_gesture(service, layout, &target) {
                    Ok(kind) => kind,
                    Err(reason) => return rejected(reason, true),
                };
                if let Some(kind) = kind {
                    self.capture = Some(Gesture {
                        kind,
                        scope: scope.clone(),
                        revision: layout.revision,
                        direction: layout.direction,
                        capacity: layout.capacity,
                        metrics: layout.metrics,
                        render_domain: layout.render_domain,
                        chrome_obstacles: layout.chrome_obstacles.clone(),
                        policy: layout.geometry_policy.clone(),
                        reference: layout.clone(),
                        pressed_target: target,
                        start: at,
                        dragging: false,
                    });
                    self.last_position = Some(at);
                }
                self.current(true)
            }
            HostPointerEvent::Move { at } => {
                if let Err(reason) = at.validate() {
                    let captured = self.is_captured();
                    self.cancel();
                    return rejected(reason, captured);
                }
                // Native capture can report bounded negative/outside positions
                // before PointerGone. Neither may destroy a valid gesture.
                if !layout.render_domain.contains(at) {
                    if let Some(capture) = &mut self.capture {
                        capture.dragging |= distance_squared(capture.start, at) >= HOST_DRAG_THRESHOLD * HOST_DRAG_THRESHOLD;
                    }
                    self.suspend();
                    return self.current(self.is_captured());
                }
                let Some(capture) = &mut self.capture else {
                    return HostInteractionUpdate::default();
                };
                self.last_position = Some(at);
                capture.dragging |= distance_squared(capture.start, at) >= HOST_DRAG_THRESHOLD * HOST_DRAG_THRESHOLD;
                if capture.dragging {
                    let (preview, candidate) = preview_gesture(capture, service, layout, at);
                    self.preview = Some(preview);
                    self.candidate = candidate;
                }
                self.current(true)
            }
            HostPointerEvent::Release { at } => {
                let capture = self.capture.take();
                let shown = self.preview.take();
                let candidate = self.candidate.take();
                let last_position = self.last_position.take();
                let Some(capture) = capture else {
                    return HostInteractionUpdate::default();
                };
                if let Err(reason) = at.validate() {
                    return rejected(reason, true);
                }
                let release_domain = if matches!(&capture.kind, GestureKind::Move { .. }) { layout.render_domain } else { layout.capacity };
                if !release_domain.contains(at) {
                    return rejected(error(Rule::Placement, "release outside placement capacity"), true);
                }
                if last_position.is_none() {
                    return HostInteractionUpdate { consumed: true, ..Default::default() };
                }
                if !capture.dragging {
                    // A release without a final Move is still positional input.
                    // It cannot turn an away-from-tab release into a click.
                    let click = distance_squared(capture.start, at) < HOST_DRAG_THRESHOLD * HOST_DRAG_THRESHOLD
                        && layout.control(&capture.pressed_target).is_some_and(|control| control.bounds.contains(at))
                        && layout.hit_test(at).as_ref() == Some(&capture.pressed_target);
                    if !click {
                        return HostInteractionUpdate { consumed: true, ..Default::default() };
                    }
                    if let GestureKind::Move { instance, click_activates: true, .. } = capture.kind
                        && service.layout().containing(&instance).is_ok_and(|section| section.active != instance)
                    {
                        return prepared_update(service, scope, layout, vec![Mutation::Activate { instance }], true);
                    }
                    return HostInteractionUpdate { consumed: true, ..Default::default() };
                }
                let Some(candidate) = candidate else {
                    return HostInteractionUpdate { consumed: true, ..Default::default() };
                };
                // Resolve only the hit identity at release. Never allocate a new
                // candidate at a position that was not shown to the user.
                let still_inside = match &capture.kind {
                    GestureKind::Move { targets, .. } => {
                        selected_target(targets, layout, at).map(|target| &target.id) == shown.as_ref().and_then(|preview| preview.target.as_ref())
                    }
                    _ => last_position == Some(at),
                };
                if !still_inside {
                    return rejected(error(Rule::Placement, "release left the shown candidate"), true);
                }
                if let Err(reason) = candidate.validate_current(service, scope, layout) {
                    return rejected(reason, true);
                }
                HostInteractionUpdate { candidate: Some(candidate), preview: shown, consumed: true, ..Default::default() }
            }
            HostPointerEvent::DoubleClick { target } => {
                self.cancel();
                if layout.control(&target).is_none() {
                    return rejected(error(Rule::MissingTarget, "double-click target is not presented"), false);
                }
                if let HostPointerTarget::Sash { first, second } = target {
                    prepared_update(service, scope, layout, vec![Mutation::ResetPair { first, second }], true)
                } else {
                    HostInteractionUpdate::default()
                }
            }
        }
    }

    fn current(&self, consumed: bool) -> HostInteractionUpdate {
        HostInteractionUpdate {
            preview: self.preview.clone(),
            rejection: self.preview.as_ref().and_then(|preview| preview.rejection.clone()),
            action: None,
            candidate: None,
            captured: self.is_captured(),
            consumed,
        }
    }
}

fn validate_context(service: &LayoutService, scope: &LocalScope, layout: &HostLayout) -> PanelResult<()> {
    scope.validate()?;
    layout.validate()?;
    if scope.workspace != service.layout().workspace {
        return Err(error(Rule::Placement, "pointer scope differs from current workspace"));
    }
    let state = service.inspect();
    if layout.revision != state.revision
        || layout.source_state != state
        || layout.source_state.layout.extent_priority_context != state.layout.extent_priority_context
        || layout.registry_specs.iter().ne(service.registry().specs())
    {
        return Err(error(Rule::Revision, "pointer geometry became stale"));
    }
    Ok(())
}

fn validate_capture(capture: &Gesture, service: &LayoutService, scope: &LocalScope, layout: &HostLayout) -> PanelResult<()> {
    if capture.reference != *layout {
        return Err(error(Rule::Revision, "pointer environment signature changed"));
    }
    if capture.scope != *scope {
        return Err(error(Rule::Placement, "pointer owner or local context changed"));
    }
    if capture.revision != layout.revision {
        return Err(error(Rule::Revision, "pointer source revision changed"));
    }
    if capture.direction != layout.direction
        || capture.capacity != layout.capacity
        || capture.metrics != layout.metrics
        || capture.render_domain != layout.render_domain
        || capture.chrome_obstacles != layout.chrome_obstacles
        || capture.policy != layout.geometry_policy
    {
        return Err(error(Rule::Bounds, "pointer geometry context changed"));
    }
    match &capture.kind {
        GestureKind::Move { instance, section, .. } => {
            service.availability(instance).require()?;
            if service.layout().containing(instance)?.id != *section
                || !layout.sections.iter().any(|presented| {
                    presented.section == *section && (presented.instance == *instance || presented.tabs.iter().any(|tab| tab.instance == *instance))
                })
            {
                return Err(error(Rule::MissingTarget, "pointer source is no longer presented"));
            }
        }
        GestureKind::Resize { section, .. } => {
            let geometry = layout
                .sections
                .iter()
                .find(|presented| presented.section == *section)
                .ok_or_else(|| error(Rule::MissingTarget, "resize source is no longer presented"))?;
            service.layout().section(section)?;
            service.availability(&geometry.instance).require()?;
        }
        GestureKind::Pair { first, second, .. } => {
            if !layout.sashes.iter().any(|sash| sash.first == *first && sash.second == *second) {
                return Err(error(Rule::MissingTarget, "sash neighbours are no longer presented"));
            }
        }
    }
    Ok(())
}

fn begin_gesture(service: &LayoutService, layout: &HostLayout, target: &HostPointerTarget) -> PanelResult<Option<GestureKind>> {
    match target {
        HostPointerTarget::Header { instance } | HostPointerTarget::Tab { instance } => {
            service.availability(instance).require()?;
            let section = service.layout().containing(instance)?;
            layout
                .sections
                .iter()
                .find(|geometry| geometry.section == section.id)
                .ok_or_else(|| error(Rule::MissingTarget, "drag source is not presented"))?;
            Ok(Some(GestureKind::Move {
                instance: instance.clone(),
                section: section.id.clone(),
                click_activates: matches!(target, HostPointerTarget::Tab { .. }),
                targets: host_drop_targets(service, layout, instance)?,
            }))
        }
        HostPointerTarget::Resize { section, edge } => {
            let source = service.layout().section(section)?;
            let geometry = layout
                .sections
                .iter()
                .find(|geometry| geometry.section == *section)
                .ok_or_else(|| error(Rule::MissingTarget, "resize section is not presented"))?;
            Ok(Some(GestureKind::Resize {
                section: section.clone(),
                edge: edge.physical(source.location.zone(), layout.direction),
                size: source.size,
                outer: geometry.outer,
            }))
        }
        HostPointerTarget::Sash { first, second } => {
            let a = service.layout().section(first)?;
            let b = service.layout().section(second)?;
            let outer = layout
                .sashes
                .iter()
                .find(|sash| sash.first == *first && sash.second == *second)
                .ok_or_else(|| error(Rule::MissingTarget, "sash is not presented"))?
                .bounds;
            Ok(Some(GestureKind::Pair {
                first: first.clone(),
                second: second.clone(),
                first_size: layout
                    .sections
                    .iter()
                    .find(|card| card.section == a.id)
                    .map(|card| card.outer.size)
                    .ok_or_else(|| error(Rule::MissingTarget, "first allocated section absent"))?,
                second_size: layout
                    .sections
                    .iter()
                    .find(|card| card.section == b.id)
                    .map(|card| card.outer.size)
                    .ok_or_else(|| error(Rule::MissingTarget, "second allocated section absent"))?,
                zone: a.location.zone(),
                outer,
            }))
        }
        _ => Ok(None),
    }
}

/// Keyboard resizing uses the same pure candidate geometry as a pointer preview.
/// It neither captures a pointer nor publishes a second allocation.
pub(crate) fn keyboard_resize_action(
    service: &LayoutService,
    layout: &HostLayout,
    target: &HostPointerTarget,
    action: ContextualPanelAction,
) -> PanelResult<LayoutAction> {
    use ContextualPanelAction::*;
    if action == ResetSash {
        return match target {
            HostPointerTarget::Sash { first, second } => {
                Ok(self::action(layout.revision, vec![Mutation::ResetPair { first: first.clone(), second: second.clone() }]))
            }
            _ => Err(error(Rule::Availability, "focused control has no neighbour split")),
        };
    }
    let step = crate::theme::Tokens::get().space_8.max(HOST_DRAG_THRESHOLD);
    let (dx, dy) = match action {
        ResizeLeft => (-step, 0.0),
        ResizeRight => (step, 0.0),
        ResizeUp => (0.0, -step),
        ResizeDown => (0.0, step),
        _ => return Err(error(Rule::Availability, "action is not a resize")),
    };
    let preview = match begin_gesture(service, layout, target)? {
        Some(GestureKind::Resize { section, edge, size, outer }) => {
            let horizontal = matches!(edge, HostResizeEdge::Left | HostResizeEdge::Right);
            if edge != HostResizeEdge::BottomRight && ((horizontal && dx == 0.0) || (!horizontal && dy == 0.0)) {
                return Err(error(Rule::Availability, "resize key is perpendicular to focused edge"));
            }
            resize_preview(service, layout, &section, edge, size, outer, dx, dy)
        }
        Some(GestureKind::Pair { first, second, first_size, second_size, zone, outer }) => {
            if (zone == Zone::Bottom && dx == 0.0) || (zone != Zone::Bottom && dy == 0.0) {
                return Err(error(Rule::Availability, "resize key is perpendicular to focused split"));
            }
            let delta = if zone == Zone::Bottom { dx * if layout.direction == Direction::Rtl { -1.0 } else { 1.0 } } else { dy };
            pair_preview(service, layout, &first, &second, first_size, second_size, zone, outer, delta)
        }
        _ => return Err(error(Rule::Availability, "focused control is not a resize target")),
    };
    if let Some(reason) = preview.rejection {
        return Err(reason);
    }
    // A clamped split still owns the key, without manufacturing pointer input.
    preview.action.ok_or_else(|| error(Rule::Bounds, "focused resize has reached its bound"))
}

fn preview_gesture(capture: &Gesture, service: &LayoutService, layout: &HostLayout, at: Point) -> (HostPreview, Option<PreparedHostLayout>) {
    let mut preview = match &capture.kind {
        GestureKind::Move { instance, section, targets, .. } => move_preview(service, layout, instance, section, at, targets),
        GestureKind::Resize { section, edge, size, outer } => {
            resize_preview(service, layout, section, *edge, *size, *outer, at.x - capture.start.x, at.y - capture.start.y)
        }
        GestureKind::Pair { first, second, first_size, second_size, zone, outer } => {
            let delta = if *zone == Zone::Bottom {
                (at.x - capture.start.x) * if layout.direction == Direction::Rtl { -1.0 } else { 1.0 }
            } else {
                at.y - capture.start.y
            };
            pair_preview(service, layout, first, second, *first_size, *second_size, *zone, *outer, delta)
        }
    };
    let candidate = if let Some(action) = &preview.action {
        match prepare_host_layout(service, &capture.scope, layout, action.clone()) {
            Ok(candidate) => {
                let shown_bounds = match &capture.kind {
                    GestureKind::Move { instance, .. } => {
                        candidate.layout().sections.iter().find(|card| card.instance == *instance).map(|card| card.outer)
                    }
                    GestureKind::Resize { section, .. } => {
                        candidate.layout().sections.iter().find(|card| card.section == *section).map(|card| card.outer)
                    }
                    GestureKind::Pair { first, second, .. } => {
                        candidate.layout().sashes.iter().find(|sash| sash.first == *first && sash.second == *second).map(|sash| sash.bounds)
                    }
                };
                if shown_bounds != Some(preview.bounds)
                    || preview.projected.as_ref().is_some_and(|shown| {
                        shown.sections != candidate.layout().sections
                            || shown.sashes != candidate.layout().sashes
                            || shown.extent != candidate.layout().extent
                            || shown.painted != candidate.layout().painted
                            || shown.input != candidate.layout().input
                    })
                {
                    preview.action = None;
                    preview.projected = None;
                    preview.targets.clear();
                    preview.tab_marker = None;
                    preview.rejection = Some(error(Rule::Revision, "candidate metrics changed during preparation"));
                    return (preview, None);
                }
                preview.projected = Some(candidate.layout().clone());
                Some(candidate)
            }
            Err(reason) => {
                preview.action = None;
                preview.projected = None;
                preview.targets.clear();
                preview.tab_marker = None;
                preview.rejection = Some(reason);
                None
            }
        }
    } else {
        None
    };
    (preview, candidate)
}

fn move_preview(
    service: &LayoutService,
    layout: &HostLayout,
    instance: &InstanceId,
    source: &SectionId,
    at: Point,
    targets: &[HostDropTarget],
) -> HostPreview {
    let fallback = layout.sections.iter().find(|section| section.section == *source).map_or(layout.capacity, |section| section.outer);
    let result = (|| -> PanelResult<HostPreview> {
        let Some(target) = selected_target(targets, layout, at) else {
            return Ok(HostPreview {
                rejection: Some(error(Rule::Placement, "no explicit docking target")),
                targets: Vec::new(),
                ..HostPreview::empty(fallback)
            });
        };
        if let Some(reason) = &target.rejection {
            return Ok(HostPreview {
                rejection: Some(reason.clone()),
                target: Some(target.id.clone()),
                targets: Vec::new(),
                ..HostPreview::empty(fallback)
            });
        }
        let candidate = move_action(service, layout, instance, &target.destination)?;
        let proposed = project_reserved(service, layout, candidate.clone())?;
        let projected = resolve_host_projected(service, &proposed, layout)?;
        let target_section = proposed.containing(instance)?;
        let card = projected
            .sections
            .iter()
            .find(|card| card.section == target_section.id)
            .ok_or_else(|| error(Rule::Bounds, "docking candidate cannot be presented"))?;
        let tab_marker = if matches!(target.destination, Destination::JoinTab { .. }) {
            let bounds = card.tabs.iter().find(|tab| tab.instance == *instance).map_or(card.header, |tab| tab.bounds);
            let width = layout.metrics.sash_width.min(bounds.size.width);
            Some(rect(
                if layout.direction == Direction::Rtl { right(bounds) - width } else { bounds.origin.x },
                bounds.origin.y,
                width,
                bounds.size.height,
            ))
        } else {
            None
        };
        Ok(HostPreview {
            action: Some(candidate),
            bounds: card.outer,
            destination: Some(target.destination.clone()),
            rejection: None,
            projected: Some(projected),
            target: Some(target.id.clone()),
            targets: vec![target.clone()],
            tab_marker,
        })
    })();
    match result {
        Ok(preview) => preview,
        Err(reason) => HostPreview { rejection: Some(reason), ..HostPreview::empty(fallback) },
    }
}

fn move_action(service: &LayoutService, layout: &HostLayout, instance: &InstanceId, destination: &Destination) -> PanelResult<LayoutAction> {
    let mut mutations = vec![Mutation::Move { instance: instance.clone(), destination: destination.clone() }];
    // The selected instance is the landing tab, including when the source tab
    // was inactive. Activation is part of the same guarded transaction.
    if let Destination::JoinTab { .. } = destination {
        mutations.push(Mutation::Activate { instance: instance.clone() });
    }
    service.layout().instance(instance)?;
    Ok(action(layout.revision, mutations))
}

/// Invisible physical regions. Geometry is logical already: do not multiply by
/// device scale. Rejection is evaluated only for the selected target.
pub fn host_drop_targets(service: &LayoutService, layout: &HostLayout, instance: &InstanceId) -> PanelResult<Vec<HostDropTarget>> {
    layout.validate()?;
    let state = service.inspect();
    if layout.revision != state.revision
        || layout.source_state != state
        || layout.source_state.layout.extent_priority_context != state.layout.extent_priority_context
        || layout.registry_specs.iter().ne(service.registry().specs())
    {
        return Err(error(Rule::Revision, "drop geometry became stale"));
    }
    service.availability(instance).require()?;
    let source = service.layout().containing(instance)?;
    let new_section = if source.tabs.len() == 1 {
        source.id.clone()
    } else {
        next_host_section_id(service.layout(), &layout.geometry_policy.identity_reservations)?
    };
    let new_column = next_host_column_id(service.layout(), &layout.geometry_policy.identity_reservations)?;
    let inner_left = innermost_receiving_column(layout, source, instance, Zone::Left);
    let inner_right = innermost_receiving_column(layout, source, instance, Zone::Right);
    let mut targets = Vec::new();
    let mut add =
        |id: HostDropTargetId, bounds: LogicalRect, destination: Destination, priority: u8, edge: Option<(bool, f32)>, stable_key: String| {
            if bounds.validate().is_ok() {
                targets.push(HostDropTarget { id, bounds, destination, rejection: None, priority, edge, stable_key });
            }
        };
    for card in &layout.sections {
        let target = service.layout().section(&card.section)?;
        if source.id == target.id && source.tabs.len() == 1 {
            continue;
        }
        if intersect_rect(card.outer, layout.capacity).is_none() {
            continue;
        }
        let tabs: Vec<_> = target.tabs.iter().filter(|tab| *tab != instance).collect();
        let side = matches!(card.zone, Zone::Left | Zone::Right);
        let inward = 64.0_f32.min(card.outer.size.width * 0.28);
        let vertical = 56.0_f32.min(card.outer.size.height * 0.25);
        if side {
            let physical = layout.direction.edge(card.zone)?;
            for left in [true, false] {
                let at = if left { card.outer.origin.x } else { right(card.outer) };
                let placement =
                    if left == (physical == PhysicalEdge::Left) { RelativePlacement::BeforeColumn } else { RelativePlacement::AfterColumn };
                let inner = if card.zone == Zone::Left { &inner_left } else { &inner_right };
                let outward =
                    if placement == RelativePlacement::AfterColumn && target.column.as_ref().is_some_and(|column| Some(column) == inner.as_ref()) {
                        clear_column_outward(layout, card.outer, left)
                    } else {
                        COLUMN_OUTWARD
                    };
                add(
                    HostDropTargetId::Relative { section: card.section.clone(), placement },
                    rect(if left { at - outward } else { at - inward }, card.outer.origin.y, inward + outward, card.outer.size.height),
                    Destination::Relative {
                        section: card.section.clone(),
                        placement,
                        new_section: new_section.clone(),
                        new_column: Some(new_column.clone()),
                        size: source.size,
                    },
                    3,
                    Some((true, at)),
                    format!("column:{}:{}", card.section.0, if left { "left" } else { "right" }),
                );
            }
            for above in [true, false] {
                let at = if above { card.outer.origin.y } else { bottom(card.outer) };
                let placement = if above { RelativePlacement::Above } else { RelativePlacement::Below };
                add(
                    HostDropTargetId::Relative { section: card.section.clone(), placement },
                    rect(
                        card.outer.origin.x,
                        if above { at } else { at - vertical },
                        card.outer.size.width,
                        vertical + if above { 0.0 } else { clear_below_end(layout, card.outer) - at },
                    ),
                    Destination::Relative {
                        section: card.section.clone(),
                        placement,
                        new_section: new_section.clone(),
                        new_column: None,
                        size: source.size,
                    },
                    3,
                    Some((false, at)),
                    format!("stack:{}:{}", card.section.0, if above { "above" } else { "below" }),
                );
            }
        }
        add(
            HostDropTargetId::Tab { section: card.section.clone(), before: None },
            rect(
                card.outer.origin.x + if side { inward } else { 0.0 },
                card.outer.origin.y + if side { vertical } else { 0.0 },
                card.outer.size.width - if side { 2.0 * inward } else { 0.0 },
                card.outer.size.height - if side { 2.0 * vertical } else { 0.0 },
            ),
            Destination::JoinTab { section: card.section.clone(), index: tabs.len() },
            4,
            None,
            format!("tab:{}:end", card.section.0),
        );
        for tab in &card.tabs {
            // Only the leading half selects insertion before this measured tab.
            // Keep the dragged tab's own slot, indexing after its removal; a
            // trailing half on every tab would steal the natural edge regions.
            let index = target.tabs.iter().take_while(|id| **id != tab.instance).filter(|id| *id != instance).count();
            let before = tabs.get(index).map(|id| (*id).clone());
            let bounds = rect(
                tab.bounds.origin.x + if layout.direction == Direction::Rtl { tab.bounds.size.width / 2.0 } else { 0.0 },
                tab.bounds.origin.y + 9.0,
                tab.bounds.size.width / 2.0,
                (tab.bounds.size.height - 18.0).max(1.0),
            );
            add(
                HostDropTargetId::Tab { section: card.section.clone(), before },
                bounds,
                Destination::JoinTab { section: card.section.clone(), index },
                // A measured own-group slot stays reorderable even when a
                // short Bottom header lies wholly inside the docking band.
                if card.zone == Zone::Bottom && source.id == target.id { 0 } else { 2 },
                None,
                format!("tab:{}:{index}", card.section.0),
            );
        }
        if let Some(last) = card.tabs.last() {
            let bounds = rect(
                last.bounds.origin.x + if layout.direction == Direction::Ltr { last.bounds.size.width / 2.0 } else { 0.0 },
                last.bounds.origin.y + 9.0,
                last.bounds.size.width / 2.0,
                (last.bounds.size.height - 18.0).max(1.0),
            );
            add(
                HostDropTargetId::Tab { section: card.section.clone(), before: None },
                bounds,
                Destination::JoinTab { section: card.section.clone(), index: tabs.len() },
                if card.zone == Zone::Bottom && source.id == target.id { 0 } else { 2 },
                None,
                format!("tab:{}:{}:end", card.section.0, tabs.len()),
            );
        }
    }
    // Capacity excludes shell insets and reserved bottom chrome. Perimeter
    // targets include the shell gutter, without moving underneath a status bar.
    let workspace_bottom = (bottom(layout.capacity) + layout.metrics.gap).min(bottom(layout.render_domain));
    let workspace_height = (workspace_bottom - layout.capacity.origin.y).max(0.0);
    let side_height = (workspace_height - 9.0).max(0.0);
    for zone in [Zone::Left, Zone::Right, Zone::Bottom] {
        let first = layout.sections.iter().find(|card| card.zone == zone && !(card.section == source.id && source.tabs.len() == 1));
        let edge = layout.direction.edge(zone)?;
        let thickness =
            if zone == Zone::Bottom { BOTTOM_DOCK_BAND.min(workspace_height) } else { SIDE_DOCK_BAND.min(layout.render_domain.size.width) };
        let bounds = match edge {
            PhysicalEdge::Left => rect(layout.render_domain.origin.x, layout.capacity.origin.y, thickness, side_height),
            PhysicalEdge::Right => rect(right(layout.render_domain) - thickness, layout.capacity.origin.y, thickness, side_height),
            PhysicalEdge::Bottom => rect(layout.render_domain.origin.x, workspace_bottom - thickness, layout.render_domain.size.width, thickness),
        };
        let destination = if zone != Zone::Bottom
            && let Some(first) = first
        {
            Destination::Relative {
                section: first.section.clone(),
                placement: RelativePlacement::BeforeColumn,
                new_section: new_section.clone(),
                new_column: Some(new_column.clone()),
                size: source.size,
            }
        } else {
            let index = service.layout().sections.iter().filter(|section| !(section.id == source.id && source.tabs.len() == 1)).count();
            Destination::NewSection { section: new_section.clone(), index, location: SectionLocation::Dock { zone }, size: source.size }
        };
        let zone_key = match zone {
            Zone::Left => "Left",
            Zone::Right => "Right",
            _ => "Bottom",
        };
        // Outside measured own-Bottom reorder slots, Bottom wins corners, then
        // sides, measured tabs, relative edges and joins.
        add(
            HostDropTargetId::Dock { zone, before: None },
            bounds,
            destination.clone(),
            if zone == Zone::Bottom { 0 } else { 1 },
            None,
            format!("edge:{zone_key}"),
        );
        if first.is_none() && zone != Zone::Bottom {
            let width = (layout.render_domain.size.width - thickness).clamp(0.0, EMPTY_SIDE_EXTRA);
            let x = if edge == PhysicalEdge::Left { bounds.origin.x + thickness } else { bounds.origin.x - width };
            add(
                HostDropTargetId::Dock { zone, before: None },
                rect(x, bounds.origin.y, width, bounds.size.height),
                destination,
                1,
                None,
                format!("empty:{zone_key}"),
            );
        }
    }
    if targets.len() > MAX_INSTANCES * 4 + MAX_SECTIONS * 8 {
        return Err(error(Rule::CollectionLimit, "drop target limit"));
    }
    Ok(targets)
}

/// Use captured presentation, ignoring a whole source card that will vacate.
/// Available retained tabs and other presented cards remain receivers.
fn innermost_receiving_column(layout: &HostLayout, source: &Section, instance: &InstanceId, zone: Zone) -> Option<ColumnId> {
    let physical_left = layout.direction.edge(zone).ok()? == PhysicalEdge::Left;
    layout
        .sections
        .iter()
        .filter(|card| card.zone == zone && intersect_rect(card.outer, layout.capacity).is_some())
        .filter_map(|card| {
            let section = layout.source_state.layout.section(&card.section).ok()?;
            if section.id == source.id && !section.tabs.iter().any(|tab| tab != instance && layout.allocation_environment.available.contains(tab)) {
                return None;
            }
            Some((section.column.as_ref()?, if physical_left { right(card.outer) } else { -card.outer.origin.x }))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(column, _)| column.clone())
}

/// Only the added strip enters clear canvas. Keep the original eighteen points
/// even when paint/chrome already overlaps them, and stop at the first obstacle.
fn clear_column_outward(layout: &HostLayout, card: LogicalRect, left: bool) -> f32 {
    let Some(span) = intersect_rect(card, layout.capacity) else {
        return COLUMN_OUTWARD;
    };
    let at = if left { card.origin.x } else { right(card) };
    let limit = if left { at - layout.capacity.origin.x } else { right(layout.capacity) - at };
    layout
        .painted
        .iter()
        .chain(&layout.chrome_obstacles)
        .filter_map(|bounds| intersect_rect(*bounds, layout.capacity))
        .filter(|bounds| {
            bounds.origin.y < bottom(span) && bottom(*bounds) > span.origin.y && if left { bounds.origin.x < at } else { right(*bounds) > at }
        })
        .fold(CENTRE_COLUMN_OUTWARD.min(limit), |extent, bounds| extent.min(if left { at - right(bounds) } else { bounds.origin.x - at }))
        .max(COLUMN_OUTWARD)
}

/// Extend only through captured, actually presented clear space. Painted bounds
/// include collapsed/floating cards and sashes; hidden allocation is irrelevant.
fn clear_below_end(layout: &HostLayout, card: LogicalRect) -> f32 {
    let start = bottom(card);
    let Some(span) = intersect_rect(card, layout.capacity) else {
        return start;
    };
    layout
        .painted
        .iter()
        .chain(&layout.chrome_obstacles)
        .filter_map(|bounds| intersect_rect(*bounds, layout.capacity))
        .filter(|bounds| bounds.origin.x < right(span) && right(*bounds) > span.origin.x && bottom(*bounds) > start)
        .fold(bottom(layout.capacity).max(start), |end, bounds| end.min(bounds.origin.y.max(start)))
}

fn selected_target<'a>(targets: &'a [HostDropTarget], layout: &HostLayout, at: Point) -> Option<&'a HostDropTarget> {
    if !layout.render_domain.contains(at) || layout.chrome_obstacles.iter().any(|bounds| bounds.contains(at)) {
        return None;
    }
    let distance = |target: &HostDropTarget| target.edge.map_or(0.0, |(horizontal, edge)| ((if horizontal { at.x } else { at.y }) - edge).abs());
    let visible = |target: &HostDropTarget| match &target.id {
        HostDropTargetId::Dock { .. } => true,
        HostDropTargetId::Relative { section, placement: RelativePlacement::BeforeColumn | RelativePlacement::AfterColumn } => {
            at.y >= layout.capacity.origin.y
                && at.y <= bottom(layout.capacity)
                && layout.sections.iter().find(|card| card.section == *section).is_some_and(|card|
                    // Preserve inclusive old bands; the added strip stops
                    // before its far boundary, including obstacle paint.
                    (at.x >= card.outer.origin.x - COLUMN_OUTWARD && at.x <= right(card.outer) + COLUMN_OUTWARD)
                        || (at.x > target.bounds.origin.x && at.x < right(target.bounds)))
        }
        HostDropTargetId::Relative { section, placement: RelativePlacement::Below } => {
            layout.capacity.contains(at)
                && layout
                    .sections
                    .iter()
                    .find(|card| card.section == *section)
                    .is_some_and(|card| card.outer.contains(at) || at.y < clear_below_end(layout, card.outer))
        }
        _ => layout.capacity.contains(at),
    };
    // Enlarging clear-space regions must not change original edge-band ties.
    let area = |target: &HostDropTarget| match &target.id {
        HostDropTargetId::Relative { section, placement: RelativePlacement::Below } => layout
            .sections
            .iter()
            .find(|card| card.section == *section)
            .map_or(0.0, |card| card.outer.size.width * 56.0_f32.min(card.outer.size.height * 0.25)),
        HostDropTargetId::Relative { section, placement: RelativePlacement::BeforeColumn | RelativePlacement::AfterColumn } => layout
            .sections
            .iter()
            .find(|card| card.section == *section)
            .map_or(0.0, |card| (64.0_f32.min(card.outer.size.width * 0.28) + COLUMN_OUTWARD) * card.outer.size.height),
        _ => target.bounds.size.width * target.bounds.size.height,
    };
    targets.iter().filter(|target| target.bounds.contains(at) && visible(target)).min_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| distance(a).total_cmp(&distance(b)))
            .then_with(|| area(a).total_cmp(&area(b)))
            .then_with(|| a.stable_key.cmp(&b.stable_key))
    })
}

fn resize_preview(
    service: &LayoutService,
    layout: &HostLayout,
    section: &SectionId,
    edge: HostResizeEdge,
    initial: Size,
    outer: LogicalRect,
    dx: f32,
    dy: f32,
) -> HostPreview {
    let result = (|| -> PanelResult<(LayoutAction, LogicalRect)> {
        let source = service.layout().section(section)?;
        let (min, max) = effective_allocation_bounds(service.layout(), service.registry(), source, &layout.allocation_environment)?;
        if min.width > max.width || min.height > max.height {
            return Err(error(Rule::IncompatibleTabs, "resize has incompatible composite limits"));
        }
        if matches!(&source.location, SectionLocation::Floating { .. }) {
            // A user's resize starts at the displayed rect, including both
            // dimensions when the saved rect was larger than local capacity.
            // Retaining an off-screen saved origin would move the opposite edge
            // after drop; SetSize alone cannot express this group operation.
            let width_cap = max.width.min(right(layout.capacity) - outer.origin.x);
            let height_cap = max.height.min(bottom(layout.capacity) - outer.origin.y);
            if width_cap < min.width || height_cap < min.height {
                return Err(error(Rule::Bounds, "visible floating anchor cannot fit panel minimum"));
            }
            if edge != HostResizeEdge::BottomRight {
                return Err(error(Rule::Placement, "floating resize requires its free corner"));
            }
            let size =
                Size { width: (outer.size.width + dx).clamp(min.width, width_cap), height: (outer.size.height + dy).clamp(min.height, height_cap) };
            let candidate = action(layout.revision, vec![Mutation::SetFloatingRect { section: section.clone(), origin: outer.origin, size }]);
            let projected = project_resize(service, layout, &candidate)?;
            let bounds = projected
                .sections
                .iter()
                .find(|card| card.section == *section)
                .ok_or_else(|| error(Rule::Bounds, "floating resize cannot be presented"))?
                .outer;
            if bounds.origin != outer.origin || bounds.size != size {
                return Err(error(Rule::Bounds, "floating resize cannot retain its visible anchor"));
            }
            return Ok((candidate, bounds));
        }
        let mut size = initial;
        match edge {
            HostResizeEdge::Left => size.width = (outer.size.width - dx).clamp(min.width, max.width),
            HostResizeEdge::Right => size.width = (outer.size.width + dx).clamp(min.width, max.width),
            HostResizeEdge::Top if source.location.zone() == Zone::Bottom => {
                size.height = bottom_resize_height(service.layout(), service.registry(), &layout.allocation_environment, outer.size.height - dy)?;
            }
            HostResizeEdge::Top => size.height = (outer.size.height - dy).clamp(min.height, max.height),
            HostResizeEdge::Bottom => size.height = (outer.size.height + dy).clamp(min.height, max.height),
            HostResizeEdge::BottomRight => {
                return Err(error(Rule::Placement, "dock resize requires a dock edge"));
            }
        }
        let mutation = if matches!(edge, HostResizeEdge::Left | HostResizeEdge::Right) && source.location.zone().side() {
            Mutation::SetColumnWidth {
                column: source.column.clone().ok_or_else(|| error(Rule::MissingTarget, "side resize has no column"))?,
                width: size.width,
            }
        } else if source.location.zone() == Zone::Bottom && edge == HostResizeEdge::Top {
            Mutation::SetBottomHeight { section: section.clone(), height: size.height }
        } else {
            Mutation::SetSize { section: section.clone(), size }
        };
        let candidate = action(layout.revision, vec![mutation]);
        let projected = project_resize(service, layout, &candidate)?;
        let bounds = projected
            .sections
            .iter()
            .find(|card| card.section == *section)
            .ok_or_else(|| error(Rule::Bounds, "dock resize cannot be presented"))?
            .outer;
        if source.location.zone() == Zone::Bottom
            && (outer.size.width - bounds.size.width).abs() > geometry_tolerance(&[outer.size.width, bounds.size.width])
        {
            return Err(error(Rule::Bounds, "standalone resize changes an unrequested axis"));
        }
        if source.location.zone() == Zone::Bottom
            && edge == HostResizeEdge::Top
            && projected
                .sections
                .iter()
                .filter(|card| card.zone == Zone::Bottom && !card.collapsed)
                .any(|card| (card.outer.size.height - size.height).abs() > geometry_tolerance(&[size.height]))
        {
            return Err(error(Rule::Bounds, "Bottom allocation changed proposed shared thickness"));
        }
        Ok((candidate, bounds))
    })();
    match result {
        Ok((candidate, bounds)) => resized_preview(candidate, bounds, outer),
        Err(reason) => HostPreview { rejection: Some(reason), ..HostPreview::empty(outer) },
    }
}

fn project_resize(service: &LayoutService, reference: &HostLayout, candidate: &LayoutAction) -> PanelResult<HostLayout> {
    let proposed = project_reserved(service, reference, candidate.clone())?;
    resolve_host_projected(service, &proposed, reference)
}

fn resized_preview(candidate: LayoutAction, bounds: LogicalRect, original: LogicalRect) -> HostPreview {
    HostPreview { action: if bounds == original { None } else { Some(candidate) }, ..HostPreview::empty(bounds) }
}

fn pair_preview(
    service: &LayoutService,
    layout: &HostLayout,
    first: &SectionId,
    second: &SectionId,
    _initial_a: Size,
    _initial_b: Size,
    _zone: Zone,
    outer: LogicalRect,
    delta: f32,
) -> HostPreview {
    let result = (|| -> PanelResult<(LayoutAction, LogicalRect)> {
        let pair = allocated_pair(service.layout(), service, layout, first, second)?;
        let split = (axis(pair.zone, pair.first.outer.size) + delta).clamp(pair.lower, pair.upper);
        if split == axis(pair.zone, pair.first.outer.size) {
            return Ok((action(layout.revision, Vec::new()), outer));
        }
        let original = action(
            layout.revision,
            vec![Mutation::ResizePair {
                first: first.clone(),
                second: second.clone(),
                first_size: with_axis(pair.zone, pair.first.outer.size, split),
                second_size: with_axis(pair.zone, pair.second.outer.size, pair.total - split),
            }],
        );
        let owners = project_host_action(service, layout, &original)?;
        let snapshot = current_owner_snapshot(service, layout, &owners);
        let normalized = normalize_host_action(service, layout, &original, &snapshot)?;
        let proposed = project_captured(service, layout, normalized.action, &snapshot)?;
        let projected = resolve_host_projected_with_snapshot(service, &proposed, layout, &snapshot)?;
        let bounds = projected
            .sashes
            .iter()
            .find(|sash| sash.first == *first && sash.second == *second)
            .map(|sash| sash.bounds)
            .ok_or_else(|| error(Rule::MissingTarget, "allocated sash missing after resize"))?;
        Ok((original, bounds))
    })();
    match result {
        Ok((candidate, bounds)) => resized_preview(candidate, bounds, outer),
        Err(reason) => HostPreview { rejection: Some(reason), ..HostPreview::empty(outer) },
    }
}

fn prepared_update(
    service: &LayoutService,
    scope: &LocalScope,
    reference: &HostLayout,
    mutations: Vec<Mutation>,
    consumed: bool,
) -> HostInteractionUpdate {
    match prepare_host_layout(service, scope, reference, action(service.inspect().revision, mutations)) {
        Ok(candidate) => HostInteractionUpdate { candidate: Some(candidate), consumed, ..Default::default() },
        Err(reason) => rejected(reason, consumed),
    }
}

fn action(revision: u64, mutations: Vec<Mutation>) -> LayoutAction {
    LayoutAction::Apply { expected_revision: Some(revision), mutations }
}

fn rejected(reason: PanelError, consumed: bool) -> HostInteractionUpdate {
    HostInteractionUpdate { rejection: Some(reason), consumed, ..Default::default() }
}

fn distance_squared(a: Point, b: Point) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    dx * dx + dy * dy
}
