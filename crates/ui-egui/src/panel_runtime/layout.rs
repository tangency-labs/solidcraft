//! Pure, logical-unit geometry for the opt-in workspace host.
//! Desired sizes and collapse state are never changed by capacity fitting.

use std::collections::{BTreeMap, BTreeSet};

use solidcraft_engine::panels::*;

use super::HostSurfaceStyle;
use super::interaction::{HostPointerTarget, HostProposalSnapshot, HostResizeEdge};

/// Compatibility constant only; the host never allocates rails.
pub const HOST_RAIL_THICKNESS: f32 = 32.0;

pub const MAX_USABLE_OBSTACLES: usize = 256;
const MAX_USABLE_PIECES: usize = 16384;
// Per section: declared actions, surface/title/resize, three system actions,
// tab overflow and at most one sash; tabs are bounded globally by instances.
const MAX_HOST_CONTROLS: usize = MAX_SECTIONS * (MAX_HEADER_ACTIONS + 8) + MAX_INSTANCES;

/// The full canvas minus actual occupied footprints. Free rectangles are a
/// disjoint decomposition of the region, not a replacement central viewport.
#[derive(Clone, Debug, PartialEq)]
pub struct HostUsableRegion {
    pub domain: LogicalRect,
    pub obstacles: Vec<LogicalRect>,
    pub free: Vec<LogicalRect>,
}

pub fn resolve_usable_region(domain: LogicalRect, obstacles: &[LogicalRect]) -> PanelResult<HostUsableRegion> {
    validate_capacity(domain)?;
    if obstacles.len() > MAX_USABLE_OBSTACLES {
        return Err(error(Rule::CollectionLimit, "usable-space obstacle limit"));
    }
    let mut clipped = Vec::new();
    let mut rows = vec![domain.origin.y, bottom(domain)];
    for obstacle in obstacles {
        obstacle.validate()?;
        if overlaps(domain, *obstacle) {
            rows.extend([domain.origin.y.max(obstacle.origin.y), bottom(domain).min(bottom(*obstacle))]);
            // Preserve the supplied obstacle's edges: rounding an intersection
            // inward would advertise a tiny occupied strip as usable space.
            clipped.push(*obstacle);
        }
    }
    rows.sort_by(f32::total_cmp);
    rows.dedup();
    let mut free: Vec<LogicalRect> = Vec::new();
    let mut previous_row = BTreeMap::new();
    for row in rows.windows(2) {
        let (Some(&top), Some(&end)) = (row.first(), row.get(1)) else {
            continue;
        };
        if top >= end {
            continue;
        }
        let mut current_row = BTreeMap::new();
        for (left, right) in free_intervals(domain, &clipped, top, end) {
            let Some(bounds) = inward_rect(left, top, right, end) else {
                continue;
            };
            if !domain.contains_rect(bounds) {
                continue;
            }
            let key = (left.to_bits(), bounds.size.width.to_bits());
            if let Some(index) = previous_row.get(&key).copied()
                && let Some(previous) = free.get_mut(index)
                && bottom(*previous) == top
                && let Some(merged) = inward_rect(left, previous.origin.y, right, end)
            {
                *previous = merged;
                current_row.insert(key, index);
            } else {
                if free.len() >= MAX_USABLE_PIECES {
                    return Err(error(Rule::CollectionLimit, "usable-space contour limit"));
                }
                current_row.insert(key, free.len());
                free.push(bounds);
            }
        }
        previous_row = current_row;
    }
    Ok(HostUsableRegion { domain, obstacles: clipped, free })
}

fn inward_rect(left: f32, top: f32, end_x: f32, end_y: f32) -> Option<LogicalRect> {
    let mut width = end_x - left;
    let mut height = end_y - top;
    if left + width > end_x && width > 0.0 {
        width = f32::from_bits(width.to_bits().saturating_sub(1));
    }
    if top + height > end_y && height > 0.0 {
        height = f32::from_bits(height.to_bits().saturating_sub(1));
    }
    positive_rect(left, top, width, height)
}

fn overlaps(a: LogicalRect, b: LogicalRect) -> bool {
    a.origin.x < right(b) && b.origin.x < right(a) && a.origin.y < bottom(b) && b.origin.y < bottom(a)
}

fn free_intervals(domain: LogicalRect, obstacles: &[LogicalRect], top: f32, end: f32) -> Vec<(f32, f32)> {
    let mut blocked: Vec<_> = obstacles
        .iter()
        .filter(|bounds| bounds.origin.y < end && bottom(**bounds) > top)
        .map(|bounds| (bounds.origin.x.max(domain.origin.x), right(*bounds).min(right(domain))))
        .collect();
    blocked.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut free = Vec::new();
    let mut cursor = domain.origin.x;
    for (left, end) in blocked {
        if cursor < left {
            free.push((cursor, left));
        }
        cursor = cursor.max(end);
    }
    if cursor < right(domain) {
        free.push((cursor, right(domain)));
    }
    free
}

impl HostUsableRegion {
    fn bounded(&self) -> bool {
        validate_capacity(self.domain).is_ok()
            && self.obstacles.len() <= MAX_USABLE_OBSTACLES
            && self.free.len() <= MAX_USABLE_PIECES
            && self.obstacles.iter().all(|bounds| bounds.validate().is_ok())
    }

    fn contains_bounded_rect(&self, bounds: LogicalRect) -> bool {
        bounds.validate().is_ok() && self.domain.contains_rect(bounds) && !self.obstacles.iter().any(|obstacle| overlaps(bounds, *obstacle))
    }

    pub fn contains_rect(&self, bounds: LogicalRect) -> bool {
        self.bounded() && self.contains_bounded_rect(bounds)
    }

    /// Place a measured footprint nearest its preferred centre. Candidate rows
    /// include every obstacle boundary, so placement can cross free-strip seams.
    pub fn place_footprint(&self, size: Size, preferred_center: Point, clearance: f32) -> Option<LogicalRect> {
        if !self.bounded() || size.validate().is_err() || preferred_center.validate().is_err() || !clearance.is_finite() || clearance < 0.0 {
            return None;
        }
        let padded = Size { width: size.width + 2.0 * clearance, height: size.height + 2.0 * clearance };
        if padded.validate().is_err() || padded.width > self.domain.size.width || padded.height > self.domain.size.height {
            return None;
        }
        let min_y = self.domain.origin.y;
        let max_y = bottom(self.domain) - padded.height;
        if max_y < min_y {
            return None;
        }
        let wanted = Point { x: preferred_center.x - padded.width / 2.0, y: preferred_center.y - padded.height / 2.0 };
        let mut rows = vec![min_y, max_y, wanted.y.clamp(min_y, max_y)];
        for obstacle in &self.obstacles {
            rows.extend([(obstacle.origin.y - padded.height).clamp(min_y, max_y), bottom(*obstacle).clamp(min_y, max_y)]);
        }
        rows.sort_by(f32::total_cmp);
        rows.dedup();
        let mut best: Option<(f64, LogicalRect)> = None;
        for top in rows {
            for (left, end) in free_intervals(self.domain, &self.obstacles, top, top + padded.height) {
                let max_x = end - padded.width;
                if max_x < left {
                    continue;
                }
                let outer = rect(wanted.x.clamp(left, max_x), top, padded.width, padded.height);
                if !self.contains_bounded_rect(outer) {
                    continue;
                }
                let dx = f64::from(outer.origin.x) - f64::from(wanted.x);
                let dy = f64::from(outer.origin.y) - f64::from(wanted.y);
                let distance = dx * dx + dy * dy;
                if best.as_ref().is_none_or(|(old, _)| distance < *old) {
                    best = Some((distance, outer));
                }
            }
        }
        best.map(|(_, outer)| rect(outer.origin.x + clearance, outer.origin.y + clearance, size.width, size.height))
            .filter(|bounds| self.contains_bounded_rect(*bounds))
    }

    /// Largest safe rectangle at the requested aspect, for explicit Fit/Centre.
    /// Model extents determine the aspect; layout gestures never call this.
    pub fn fit_rect(&self, aspect: f32, preferred_center: Point) -> Option<LogicalRect> {
        self.fit_rect_with_clearance(aspect, preferred_center, 0.0)
    }

    /// Search the inner model rectangle with its clearance already reserved.
    /// Shrinking an aspect-fitted result afterwards can erase thin selections.
    pub fn fit_rect_with_clearance(&self, aspect: f32, preferred_center: Point, clearance: f32) -> Option<LogicalRect> {
        if !self.bounded()
            || !aspect.is_finite()
            || aspect <= 0.0
            || preferred_center.validate().is_err()
            || !clearance.is_finite()
            || clearance < 0.0
        {
            return None;
        }
        let padding = 2.0 * clearance;
        if !padding.is_finite() {
            return None;
        }
        let available = Size { width: self.domain.size.width - padding, height: self.domain.size.height - padding };
        if available.validate().is_err() {
            return None;
        }
        let mut low = 0.0_f32;
        let mut high = available.height.min(available.width / aspect);
        if !high.is_finite() || high <= 0.0 {
            return None;
        }
        let mut best = None;
        for _ in 0..32 {
            let height = low + (high - low) / 2.0;
            if height <= low || height >= high {
                break;
            }
            if let Some(bounds) = self.place_footprint(Size { width: height * aspect, height }, preferred_center, clearance) {
                low = height;
                best = Some(bounds);
            } else {
                high = height;
            }
        }
        if let Some(bounds) = self.place_footprint(Size { width: high * aspect, height: high }, preferred_center, clearance) {
            best = Some(bounds);
        }
        best
    }

    pub fn largest_rect(&self) -> Option<LogicalRect> {
        if !self.bounded() {
            return None;
        }
        let mut rows = vec![self.domain.origin.y, bottom(self.domain)];
        for obstacle in &self.obstacles {
            rows.extend([obstacle.origin.y.max(self.domain.origin.y), bottom(*obstacle).min(bottom(self.domain))]);
        }
        rows.sort_by(f32::total_cmp);
        rows.dedup();
        let mut best: Option<LogicalRect> = None;
        for (index, &top) in rows.iter().enumerate() {
            for &end in rows.iter().skip(index + 1) {
                for (left, right) in free_intervals(self.domain, &self.obstacles, top, end) {
                    let Some(bounds) = inward_rect(left, top, right, end) else {
                        continue;
                    };
                    if self.contains_bounded_rect(bounds)
                        && best.is_none_or(|old| {
                            f64::from(bounds.size.width) * f64::from(bounds.size.height) > f64::from(old.size.width) * f64::from(old.size.height)
                        })
                    {
                        best = Some(bounds);
                    }
                }
            }
        }
        best
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HostMetrics {
    pub header_height: f32,
    pub content_header_height: f32,
    pub footer_height: f32,
    /// Separation supplied by the workspace's shared Container policy. Zero
    /// joins compact cards; resize handles retain their own positive width.
    pub gap: f32,
    pub sash_width: f32,
    pub control_extent: f32,
    pub tab_min_width: f32,
    /// Single-row Wide composition threshold, bounded by the shared contract.
    pub inline_max_height: f32,
    pub inline_title_width: f32,
    pub min_viewport: Size,
}

impl Default for HostMetrics {
    fn default() -> Self {
        let t = solidcraft_theme::Tokens::get();
        Self {
            header_height: t.form_header_height - t.space_8,
            content_header_height: 0.0,
            footer_height: t.form_footer_height,
            gap: t.space_4,
            sash_width: t.space_4,
            control_extent: t.control_height,
            tab_min_width: t.control_height * 2.0,
            inline_max_height: (t.control_height * 2.0 + t.space_4).min(88.0),
            inline_title_width: (t.control_height * 4.0).min(256.0),
            min_viewport: Size { width: t.space_2, height: t.space_2 },
        }
    }
}

impl HostMetrics {
    pub fn validate(self) -> PanelResult<()> {
        for value in [self.header_height, self.sash_width, self.control_extent, self.tab_min_width] {
            if !value.is_finite() || value <= 0.0 || value > 256.0 {
                return Err(error(Rule::Bounds, "host chrome extent must be finite and in (0, 256]"));
            }
        }
        if !self.footer_height.is_finite()
            || self.footer_height < 0.0
            || self.footer_height > 256.0
            || !self.content_header_height.is_finite()
            || self.content_header_height < 0.0
            || self.content_header_height > 256.0
        {
            return Err(error(Rule::Bounds, "invalid host footer extent"));
        }
        if !self.gap.is_finite() || !(0.0..=256.0).contains(&self.gap) {
            return Err(error(Rule::Bounds, "invalid host gap"));
        }
        if !self.inline_max_height.is_finite() || !(0.0..=88.0).contains(&self.inline_max_height) {
            return Err(error(Rule::Bounds, "invalid inline row threshold"));
        }
        if !self.inline_title_width.is_finite() || self.inline_title_width <= 0.0 || self.inline_title_width > 512.0 {
            return Err(error(Rule::Bounds, "invalid inline title width"));
        }
        self.min_viewport.validate()
    }
}

/// Immutable bounded content metric snapshot. Never render content or inspect
/// mutable controller/egui state here. Missing widths must return Revision.
pub trait HostContentMetrics: std::fmt::Debug + Send + Sync {
    fn measure(&self, request: &ContentMeasureRequest) -> PanelResult<SideContentMeasure>;
    fn revision(&self) -> u64;
}

const MAX_HOST_RESERVED_IDS: usize = MAX_RESERVED_IDENTITIES;

/// Immutable identity/recovery-priority environment. No registry entries,
/// controllers or bodies are created for quarantined placements.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostIdentityReservations {
    pub sections: BTreeSet<SectionId>,
    pub columns: BTreeSet<ColumnId>,
    pub extent_priorities: ExtentPriorityContext,
    /// Typed incoming provenance, independent of additional live reservations.
    pub replacement_ownership: Option<ReplacementIdentityOwnership>,
}
impl HostIdentityReservations {
    pub fn validate(&self) -> PanelResult<()> {
        if self.sections.len() > MAX_HOST_RESERVED_IDS || self.columns.len() > MAX_HOST_RESERVED_IDS {
            return Err(error(Rule::CollectionLimit, "host identity reservation cap"));
        }
        for id in &self.sections {
            valid_id(&id.0)?;
        }
        for id in &self.columns {
            valid_id(&id.0)?;
        }
        self.extent_priorities.validate()?;
        if let Some(ownership) = &self.replacement_ownership {
            ownership.validate()?;
        }
        Ok(())
    }
    pub(super) fn preparation_context(&self) -> PanelResult<LayoutPreparationContext> {
        self.validate()?;
        Ok(LayoutPreparationContext {
            sections: self.sections.clone(),
            columns: self.columns.clone(),
            extent_priorities: self.extent_priorities.clone(),
            replacement_ownership: self.replacement_ownership.clone(),
        })
    }
    pub(super) fn include_layout(&mut self, layout: &Layout) -> PanelResult<()> {
        if layout.sections.len() > MAX_HOST_RESERVED_IDS
            || layout.instances.len() > MAX_HOST_RESERVED_IDS
            || layout.columns.len() > MAX_HOST_RESERVED_IDS
        {
            return Err(error(Rule::CollectionLimit, "identity source cap"));
        }
        for section in &layout.sections {
            valid_id(&section.id.0)?;
            if let Some(column) = &section.column {
                valid_id(&column.0)?;
            }
            self.sections.insert(section.id.clone());
            self.columns.extend(section.column.clone());
        }
        for column in &layout.columns {
            valid_id(&column.id.0)?;
            self.columns.insert(column.id.clone());
        }
        for instance in &layout.instances {
            valid_id(&instance.memory.section.0)?;
            if let Some(column) = &instance.memory.column {
                valid_id(&column.0)?;
            }
            self.sections.insert(instance.memory.section.clone());
            self.columns.extend(instance.memory.column.clone());
        }
        for location in
            layout.sections.iter().map(|section| &section.location).chain(layout.instances.iter().map(|instance| &instance.memory.location))
        {
            if let SectionLocation::Window { return_to, .. } = location {
                if let Some(section) = &return_to.section {
                    valid_id(&section.0)?;
                }
                if let Some(join) = &return_to.join {
                    valid_id(&join.section.0)?;
                }
                if let Some(column) = &return_to.column {
                    valid_id(&column.0)?;
                }
                self.sections.extend(return_to.section.clone());
                self.sections.extend(return_to.join.as_ref().map(|join| join.section.clone()));
                self.columns.extend(return_to.column.clone());
            }
        }
        self.validate()
    }
}

/// Capture recognized full preparation metadata and fixed identity paths: no
/// migrated-envelope encoding or materialized reservation placements/controllers.
pub fn host_identity_reservations(envelope: &LayoutEnvelope) -> PanelResult<HostIdentityReservations> {
    if envelope.quarantine.len().saturating_add(envelope.layout.instances.len()) > MAX_INSTANCES
        || envelope.section_snapshots.len() > MAX_SECTIONS
        || envelope.section_order.len() > MAX_SECTIONS
        || envelope.retained_data.len() > MAX_INSTANCES
    {
        return Err(error(Rule::CollectionLimit, "envelope identity collection cap"));
    }
    let context = preparation_context(envelope)?;
    let mut reservations = HostIdentityReservations {
        sections: context.sections,
        columns: context.columns,
        extent_priorities: context.extent_priorities,
        replacement_ownership: context.replacement_ownership,
    };
    reservations.include_layout(&envelope.layout)?;
    for id in &envelope.section_order {
        valid_id(&id.0)?;
        reservations.sections.insert(id.clone());
    }
    for item in &envelope.quarantine {
        valid_id(&item.instance.0)?;
        valid_id(&item.kind.0)?;
        if let Some(id) = &item.section {
            valid_id(&id.0)?;
            reservations.sections.insert(id.clone());
        }
        reserve_wire_instance(&mut reservations, &item.instance_data)?;
    }
    for (id, snapshot) in &envelope.section_snapshots {
        valid_id(&id.0)?;
        reservations.sections.insert(id.clone());
        reserve_wire_section(&mut reservations, &snapshot.data)?;
        if let Some(section) = &snapshot.projected {
            valid_id(&section.id.0)?;
            if let Some(column) = &section.column {
                valid_id(&column.0)?;
            }
            reservations.sections.insert(section.id.clone());
            reservations.columns.extend(section.column.clone());
            reserve_location(&mut reservations, &section.location)?;
        }
    }
    for data in envelope.retained_data.values() {
        // Retained extension payloads may have opaque unrelated fields. Inspect
        // only the known host wrapper shape; never reinterpret arbitrary data.
        if let Some(instance) =
            data.get("instance").filter(|instance| instance.get("memory").and_then(|memory| memory.get("section")).is_some_and(|id| id.is_string()))
        {
            reserve_wire_instance(&mut reservations, instance)?;
            if let Some(id) = data.get("section").and_then(|id| id.as_str()) {
                valid_id(id)?;
                reservations.sections.insert(SectionId(id.to_owned()));
            }
        }
    }
    reservations.validate()?;
    Ok(reservations)
}

fn wire_identity<'a>(value: &'a serde_json::Value, key: &str) -> PanelResult<Option<&'a str>> {
    let Some(raw) = value.get(key).filter(|raw| !raw.is_null()) else {
        return Ok(None);
    };
    let id = raw.as_str().ok_or_else(|| error(Rule::Schema, "reserved identity must be a string"))?;
    valid_id(id)?;
    Ok(Some(id))
}
fn reserve_wire_instance(out: &mut HostIdentityReservations, value: &serde_json::Value) -> PanelResult<()> {
    let memory = value.get("memory").filter(|memory| memory.is_object()).ok_or_else(|| error(Rule::Schema, "reserved instance memory missing"))?;
    if let Some(id) = wire_identity(memory, "section")? {
        out.sections.insert(SectionId(id.to_owned()));
    }
    if let Some(id) = wire_identity(memory, "column")? {
        out.columns.insert(ColumnId(id.to_owned()));
    }
    if let Some(location) = memory.get("location") {
        reserve_wire_location(out, location)?;
    }
    Ok(())
}
fn reserve_wire_section(out: &mut HostIdentityReservations, value: &serde_json::Value) -> PanelResult<()> {
    if let Some(id) = wire_identity(value, "id")? {
        out.sections.insert(SectionId(id.to_owned()));
    }
    if let Some(id) = wire_identity(value, "column")? {
        out.columns.insert(ColumnId(id.to_owned()));
    }
    if let Some(location) = value.get("location") {
        reserve_wire_location(out, location)?;
    }
    Ok(())
}
fn reserve_wire_location(out: &mut HostIdentityReservations, location: &serde_json::Value) -> PanelResult<()> {
    if location.get("type").and_then(|value| value.as_str()) != Some("window") {
        return Ok(());
    }
    let target = location.get("return_to").filter(|value| value.is_object()).ok_or_else(|| error(Rule::Schema, "reserved native return missing"))?;
    if let Some(id) = wire_identity(target, "section")? {
        out.sections.insert(SectionId(id.to_owned()));
    }
    if let Some(id) = wire_identity(target, "column")? {
        out.columns.insert(ColumnId(id.to_owned()));
    }
    if let Some(join) = target.get("join").filter(|value| !value.is_null())
        && let Some(id) = wire_identity(join, "section")?
    {
        out.sections.insert(SectionId(id.to_owned()));
    }
    Ok(())
}
fn reserve_location(out: &mut HostIdentityReservations, location: &SectionLocation) -> PanelResult<()> {
    if let SectionLocation::Window { return_to, .. } = location {
        if let Some(section) = &return_to.section {
            valid_id(&section.0)?;
        }
        if let Some(join) = &return_to.join {
            valid_id(&join.section.0)?;
        }
        if let Some(column) = &return_to.column {
            valid_id(&column.0)?;
        }
        out.sections.extend(return_to.section.clone());
        out.sections.extend(return_to.join.as_ref().map(|join| join.section.clone()));
        out.columns.extend(return_to.column.clone());
    }
    Ok(())
}

pub fn next_host_section_id(layout: &Layout, external: &HostIdentityReservations) -> PanelResult<SectionId> {
    external.validate()?;
    next_section_id_with_reservations(layout, &external.sections)
}
pub fn next_host_column_id(layout: &Layout, external: &HostIdentityReservations) -> PanelResult<ColumnId> {
    external.validate()?;
    next_column_id_with_reservations(layout, &external.columns)
}

/// Captured renderer fill alpha, separate from declared surface policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostSurfaceAlpha {
    pub docked: u8,
    pub floating: u8,
}

#[derive(Clone, Debug)]
pub struct HostGeometryPolicy {
    pub surface_alpha: Option<HostSurfaceAlpha>,
    /// Captured renderer state; never written into a layout envelope.
    pub surfaces: Option<HostSurfaceStyle>,
    /// Runtime normalized opacity override; None retains authored theme opacity.
    pub requested_opacity: Option<f32>,
    pub default_opaque: bool,
    pub effective_surfaces: BTreeMap<SectionId, PanelSurface>,
    pub(crate) hidden_header_actions: BTreeMap<InstanceId, BTreeSet<ControlId>>,
    pub(crate) header_title_widths: BTreeMap<InstanceId, f32>,
    pub(crate) tab_title_widths: BTreeMap<InstanceId, f32>,
    pub content: Vec<SideContentMeasure>,
    pub content_revision: u64,
    pub scroll: Point,
    pub measure: Option<std::sync::Arc<dyn HostContentMetrics>>,
    pub identity_reservations: HostIdentityReservations,
}

impl PartialEq for HostGeometryPolicy {
    fn eq(&self, other: &Self) -> bool {
        self.surfaces == other.surfaces
            && self.requested_opacity.map(f32::to_bits) == other.requested_opacity.map(f32::to_bits)
            && self.surface_alpha == other.surface_alpha
            && self.default_opaque == other.default_opaque
            && self.effective_surfaces == other.effective_surfaces
            && self.hidden_header_actions == other.hidden_header_actions
            && self.header_title_widths == other.header_title_widths
            && self.tab_title_widths == other.tab_title_widths
            && self.identity_reservations == other.identity_reservations
            && self.content == other.content
            && self.content_revision == other.content_revision
            && self.scroll == other.scroll
            && match (&self.measure, &other.measure) {
                (None, None) => true,
                (Some(a), Some(b)) => std::sync::Arc::ptr_eq(a, b) && a.revision() == b.revision(),
                _ => false,
            }
    }
}

impl Default for HostGeometryPolicy {
    fn default() -> Self {
        Self {
            surface_alpha: None,
            surfaces: None,
            requested_opacity: None,
            default_opaque: true,
            effective_surfaces: BTreeMap::new(),
            hidden_header_actions: BTreeMap::new(),
            header_title_widths: BTreeMap::new(),
            tab_title_widths: BTreeMap::new(),
            content: Vec::new(),
            content_revision: 0,
            scroll: Point { x: 0.0, y: 0.0 },
            measure: None,
            identity_reservations: HostIdentityReservations::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct HostControl {
    pub target: HostPointerTarget,
    pub bounds: LogicalRect,
    pub role: InputRole,
    pub accessible_name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HostTab {
    pub instance: InstanceId,
    pub bounds: LogicalRect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HostSection {
    pub contrast: Option<ContrastOutcome>,
    pub paint_alpha: Option<u8>,
    pub section: SectionId,
    pub instance: InstanceId,
    pub zone: Zone,
    pub outer: LogicalRect,
    pub header: LogicalRect,
    /// Shared title/grip drag region; grip only for tab groups. Disjoint from tabs,
    /// actions and editable inline content.
    pub title: Option<LogicalRect>,
    pub menu: LogicalRect,
    pub action_bar: Option<LogicalRect>,
    /// Tab headings within the shared header, after the section grip.
    pub tab_strip: Option<LogicalRect>,
    pub tabs: Vec<HostTab>,
    pub hidden_tabs: Vec<InstanceId>,
    pub tab_overflow: Option<LogicalRect>,
    pub body: Option<LogicalRect>,
    pub collapsed: bool,
    pub inline: bool,
    /// Editable inline content is separate from the shared title drag region.
    pub inline_content: Option<LogicalRect>,
    pub collapse: Option<LogicalRect>,
    pub close: Option<LogicalRect>,
    pub actions: Vec<(ControlId, LogicalRect)>,
    pub footer: Option<LogicalRect>,
    pub peek: bool,
    pub surface: PanelSurface,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HostSash {
    pub first: SectionId,
    pub second: SectionId,
    pub zone: Zone,
    pub bounds: LogicalRect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HostRail {
    pub section: SectionId,
    pub instance: InstanceId,
    pub zone: Zone,
    pub bounds: LogicalRect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HostOverflow {
    pub zone: Zone,
    /// Complete hidden set, including entries that cannot have any physical chrome.
    pub sections: Vec<SectionId>,
    pub tabs: Vec<InstanceId>,
    pub bounds: Option<LogicalRect>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HostLayout {
    pub resolved: ResolvedLayout,
    /// Paint order: docks, then floating cards.
    pub sections: Vec<HostSection>,
    pub sashes: Vec<HostSash>,
    pub rails: Vec<HostRail>,
    pub overflow: Vec<HostOverflow>,
    /// A single bounded menu for every hidden entry, including groups whose
    /// own rail cannot fit. None means even one control cannot be presented.
    pub overflow_launcher: Option<LogicalRect>,
    /// Typed routes are authoritative; never parse a foundation ControlId.
    pub controls: Vec<HostControl>,
    pub render_domain: LogicalRect,
    pub capacity: LogicalRect,
    pub chrome_obstacles: Vec<LogicalRect>,
    pub usable: HostUsableRegion,
    pub viewport: Option<LogicalRect>,
    pub painted: Vec<LogicalRect>,
    pub input: Vec<LogicalRect>,
    pub occlusion: Vec<LogicalRect>,
    pub reservations: Vec<LogicalRect>,
    pub revision: u64,
    pub direction: Direction,
    pub metrics: HostMetrics,
    pub geometry_policy: HostGeometryPolicy,
    pub extent: Size,
    pub allocation_environment: AllocationEnvironment,
    pub source_state: LayoutReply,
    pub registry_specs: Vec<PanelSpec>,
}

impl HostLayout {
    /// Public geometry remains bounded even when a direct adapter constructs or
    /// modifies it. In particular, pointer code must not clamp against NaN.
    pub fn validate(&self) -> PanelResult<()> {
        self.allocation_environment.validate()?;
        self.geometry_policy.identity_reservations.validate()?;
        if self.registry_specs.len() > MAX_INSTANCES {
            return Err(error(Rule::CollectionLimit, "host registry signature bound"));
        }
        let env = &self.allocation_environment;
        if env.capacity != self.capacity
            || env.direction != self.direction
            || env.header_height != self.metrics.header_height
            || env.footer_height != self.metrics.footer_height + self.metrics.content_header_height
            || env.gap != self.metrics.gap
            || env.min_viewport != self.metrics.min_viewport
            || env.inline_max_height != self.metrics.inline_max_height
            || env.scroll != self.geometry_policy.scroll
            || env.content_revision != self.geometry_policy.content_revision
        {
            return Err(error(Rule::Bounds, "host allocation environment differs from presentation"));
        }
        if !self.extent.width.is_finite()
            || !self.extent.height.is_finite()
            || self.extent.width < self.capacity.size.width
            || self.extent.height < self.capacity.size.height
            || self.extent.width > MAX_LOGICAL
            || self.extent.height > MAX_LOGICAL
        {
            return Err(error(Rule::Bounds, "invalid host scroll extent"));
        }
        if self.geometry_policy.measure.as_ref().is_some_and(|provider| provider.revision() != self.allocation_environment.content_revision) {
            return Err(error(Rule::Revision, "content metric snapshot changed"));
        }
        validate_capacity(self.capacity)?;
        validate_capacity(self.render_domain)?;
        if !self.render_domain.contains_rect(self.capacity) || self.usable.domain != self.render_domain {
            return Err(error(Rule::Bounds, "placement/usable domain differs from render domain"));
        }
        if self.chrome_obstacles.len() > MAX_USABLE_OBSTACLES
            || self.usable.obstacles.len() > MAX_USABLE_OBSTACLES
            || self.usable.free.len() > MAX_USABLE_PIECES
        {
            return Err(error(Rule::CollectionLimit, "usable-space collection bound"));
        }
        for bounds in self.chrome_obstacles.iter().chain(&self.usable.obstacles) {
            bounds.validate()?;
        }
        for bounds in &self.usable.free {
            bounds.validate()?;
            if !self.render_domain.contains_rect(*bounds) || self.usable.obstacles.iter().any(|obstacle| overlaps(*bounds, *obstacle)) {
                return Err(error(Rule::Bounds, "usable-space geometry crosses occupied bounds"));
            }
        }
        self.metrics.validate()?;
        if self.geometry_policy.requested_opacity.is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value)) {
            return Err(error(Rule::Bounds, "invalid requested panel opacity"));
        }
        if self.geometry_policy.effective_surfaces.len() > MAX_SECTIONS {
            return Err(error(Rule::CollectionLimit, "host geometry policy bound"));
        }
        for section in self.geometry_policy.effective_surfaces.keys() {
            valid_id(&section.0)?;
        }
        self.resolved.validate()?;
        if self.sections.len() > MAX_SECTIONS
            || self.sashes.len() > MAX_SECTIONS
            || self.rails.len() > MAX_INSTANCES
            || self.overflow.len() > 5
            || self.controls.len() > MAX_HOST_CONTROLS
            || self.painted.len() > MAX_INSTANCES * 8
            || self.input.len() > MAX_INSTANCES * 8
            || self.occlusion.len() > MAX_INSTANCES * 8
            || self.reservations.len() > MAX_INSTANCES * 8
        {
            return Err(error(Rule::CollectionLimit, "host geometry collection bound"));
        }
        for bounds in self.painted.iter().chain(&self.input).chain(&self.occlusion).chain(&self.reservations).chain(self.overflow_launcher.iter()) {
            bounds.validate()?;
            if !self.capacity.contains_rect(*bounds) {
                return Err(error(Rule::Bounds, "host geometry outside capacity"));
            }
        }
        if self.viewport
            != positive_rect(self.render_domain.origin.x, self.render_domain.origin.y, self.render_domain.size.width, self.render_domain.size.height)
        {
            return Err(error(Rule::Bounds, "render viewport must fill its domain"));
        }
        for bounds in &self.input {
            if !self.painted.iter().any(|paint| paint.contains_rect(*bounds)) {
                return Err(error(Rule::Bounds, "host input outside paint"));
            }
        }
        let mut targets = BTreeSet::new();
        for control in &self.controls {
            control.target.validate()?;
            control.bounds.validate()?;
            if control.accessible_name.is_empty() || control.accessible_name.len() > 256 {
                return Err(error(Rule::Bounds, "host control semantic name bound"));
            }
            if !targets.insert(&control.target) {
                return Err(error(Rule::DuplicateId, "duplicate typed host target"));
            }
            if !self.painted.iter().any(|paint| paint.contains_rect(control.bounds)) {
                return Err(error(Rule::Bounds, "host control outside paint"));
            }
        }
        for section in &self.sections {
            valid_id(&section.section.0)?;
            valid_id(&section.instance.0)?;
            if (section.collapsed || section.inline) && (section.body.is_some() || section.footer.is_some())
                || section.inline_content.is_some() && (!section.inline || section.collapsed)
                || section.inline && section.header != section.outer
            {
                return Err(error(Rule::Bounds, "collapsed/inline host has inconsistent content roles"));
            }
            if section.actions.len() > MAX_HEADER_ACTIONS {
                return Err(error(Rule::CollectionLimit, "host header action cap"));
            }
            let mut actions = BTreeSet::new();
            for (action, _) in &section.actions {
                valid_id(&action.0)?;
                if !actions.insert(action) {
                    return Err(error(Rule::DuplicateId, "duplicate host header action"));
                }
            }
            if section.tabs.len() > MAX_TABS || section.hidden_tabs.len() > MAX_TABS || section.tabs.len() + section.hidden_tabs.len() > MAX_TABS {
                return Err(error(Rule::CollectionLimit, "host tab geometry bound"));
            }
            if !self.resolved.panels.iter().any(|panel| {
                panel.section == section.section
                    && panel.instance == section.instance
                    && panel.zone == section.zone
                    && panel.outer == section.outer
                    && panel.body == section.body
            }) {
                return Err(error(Rule::Placement, "host section differs from mounted geometry"));
            }
            for bounds in [
                Some(section.header),
                section.title,
                Some(section.menu),
                section.action_bar,
                section.collapse,
                section.close,
                section.tab_strip,
                section.body,
                section.footer,
                section.tab_overflow,
                section.inline_content,
            ]
            .into_iter()
            .flatten()
            .chain(section.actions.iter().map(|(_, bounds)| *bounds))
            {
                bounds.validate()?;
                if !section.outer.contains_rect(bounds) {
                    return Err(error(Rule::Bounds, "host slot outside card"));
                }
            }
            for tab in &section.tabs {
                valid_id(&tab.instance.0)?;
                tab.bounds.validate()?;
                if !section.tab_strip.is_some_and(|strip| strip.contains_rect(tab.bounds)) {
                    return Err(error(Rule::Bounds, "tab outside strip"));
                }
            }
            for instance in &section.hidden_tabs {
                valid_id(&instance.0)?;
            }
        }
        for rail in &self.rails {
            valid_id(&rail.section.0)?;
            valid_id(&rail.instance.0)?;
            rail.bounds.validate()?;
            if !self.capacity.contains_rect(rail.bounds) {
                return Err(error(Rule::Bounds, "rail outside capacity"));
            }
        }
        for group in &self.overflow {
            if group.sections.len() > MAX_SECTIONS || group.tabs.len() > MAX_INSTANCES {
                return Err(error(Rule::CollectionLimit, "host overflow collection bound"));
            }
            for section in &group.sections {
                valid_id(&section.0)?;
            }
            for instance in &group.tabs {
                valid_id(&instance.0)?;
            }
            if let Some(bounds) = group.bounds {
                bounds.validate()?;
                if !self.capacity.contains_rect(bounds) {
                    return Err(error(Rule::Bounds, "overflow outside capacity"));
                }
            }
        }
        Ok(())
    }

    pub fn control(&self, target: &HostPointerTarget) -> Option<&HostControl> {
        self.controls.iter().find(|control| control.target == *target)
    }

    /// Reverse traversal follows the actual topmost painted chrome.
    pub fn hit_test(&self, at: Point) -> Option<HostPointerTarget> {
        if at.validate().is_err() {
            return None;
        }
        self.controls.iter().rev().find(|control| control.bounds.contains(at)).map(|control| control.target.clone())
    }

    pub fn consumes_pointer(&self, at: Point) -> bool {
        at.validate().is_ok() && self.input.iter().any(|bounds| bounds.contains(at))
    }
}

/// Construct allocation environment from captured service state. No content code
/// runs here; availability is a bounded snapshot, not a callback in the allocator.
fn allocation_environment(
    service: &LayoutService,
    proposed: &Layout,
    capacity: LogicalRect,
    direction: Direction,
    metrics: HostMetrics,
    policy: &HostGeometryPolicy,
    availability: Option<&AvailabilityMap>,
) -> PanelResult<AllocationEnvironment> {
    metrics.validate()?;
    policy.identity_reservations.validate()?;
    let env = AllocationEnvironment {
        capacity,
        direction,
        header_height: metrics.header_height,
        footer_height: metrics.footer_height + metrics.content_header_height,
        gap: metrics.gap,
        inline_max_height: metrics.inline_max_height,
        min_viewport: metrics.min_viewport,
        scroll: policy.scroll,
        available: proposed
            .instances
            .iter()
            .filter(|instance| {
                availability.map_or_else(
                    || service.availability(&instance.id) == Availability::Available,
                    |captured| captured.get(&instance.id).is_none_or(|state| *state == Availability::Available),
                )
            })
            .map(|instance| instance.id.clone())
            .collect(),
        content: policy.content.clone(),
        content_revision: policy.content_revision,
    };
    env.validate()?;
    if policy.measure.as_ref().is_some_and(|provider| provider.revision() != env.content_revision) {
        return Err(error(Rule::Revision, "content metric snapshot revision differs"));
    }
    Ok(env)
}

pub fn host_measure_requests(
    service: &LayoutService,
    proposed: &Layout,
    capacity: LogicalRect,
    direction: Direction,
    metrics: HostMetrics,
    policy: &HostGeometryPolicy,
) -> PanelResult<Vec<ContentMeasureRequest>> {
    allocation_measure_requests(proposed, service.registry(), &allocation_environment(service, proposed, capacity, direction, metrics, policy, None)?)
}

pub fn resolve_host_layout(
    service: &LayoutService,
    capacity: LogicalRect,
    direction: Direction,
    metrics: HostMetrics,
    policy: &HostGeometryPolicy,
) -> PanelResult<HostLayout> {
    resolve_host_layout_in_domain(service, capacity, capacity, &[], direction, metrics, policy)
}

pub fn resolve_host_layout_in_domain(
    service: &LayoutService,
    render_domain: LogicalRect,
    capacity: LogicalRect,
    chrome_obstacles: &[LogicalRect],
    direction: Direction,
    metrics: HostMetrics,
    policy: &HostGeometryPolicy,
) -> PanelResult<HostLayout> {
    resolve_layout_source(service, service.layout(), render_domain, capacity, chrome_obstacles, direction, metrics, policy, None)
}

/// Noncommittable current-owner geometry probe over structural projection data.
pub(super) fn resolve_host_projected(service: &LayoutService, proposed: &Layout, reference: &HostLayout) -> PanelResult<HostLayout> {
    resolve_layout_source(
        service,
        proposed,
        reference.render_domain,
        reference.capacity,
        &reference.chrome_obstacles,
        reference.direction,
        reference.metrics,
        &reference.geometry_policy,
        None,
    )
}

/// Final proposal geometry uses proposed-owner availability and one immutable
/// metric snapshot. The outgoing reference still anchors placement and freshness.
pub(super) fn resolve_host_prepared_with_snapshot(
    service: &LayoutService,
    prepared: &PreparedLayout,
    reference: &HostLayout,
    snapshot: &HostProposalSnapshot,
) -> PanelResult<HostLayout> {
    resolve_host_projected_with_snapshot(service, prepared.layout(), reference, snapshot)
}

/// All numeric prefix probes share the final owner's immutable environment.
pub(super) fn resolve_host_projected_with_snapshot(
    service: &LayoutService,
    proposed: &Layout,
    reference: &HostLayout,
    snapshot: &HostProposalSnapshot,
) -> PanelResult<HostLayout> {
    let mut policy = reference.geometry_policy.clone();
    policy.content.clear();
    policy.content_revision = snapshot.metrics.revision();
    policy.measure = Some(snapshot.metrics.clone());
    resolve_layout_source(
        service,
        proposed,
        reference.render_domain,
        reference.capacity,
        &reference.chrome_obstacles,
        reference.direction,
        reference.metrics,
        &policy,
        Some(&snapshot.availability),
    )
}

fn resolve_layout_source(
    service: &LayoutService,
    proposed: &Layout,
    render_domain: LogicalRect,
    capacity: LogicalRect,
    chrome_obstacles: &[LogicalRect],
    direction: Direction,
    metrics: HostMetrics,
    policy: &HostGeometryPolicy,
    availability: Option<&AvailabilityMap>,
) -> PanelResult<HostLayout> {
    validate_capacity(capacity)?;
    validate_capacity(render_domain)?;
    if !render_domain.contains_rect(capacity) {
        return Err(error(Rule::Bounds, "placement outside render domain"));
    }
    let usable = resolve_usable_region(render_domain, chrome_obstacles)?;
    let mut env = allocation_environment(service, proposed, capacity, direction, metrics, policy, availability)?;
    for request in allocation_measure_requests(proposed, service.registry(), &env)? {
        if env.content.iter().any(|measure| measure.request == request) {
            continue;
        }
        let provider = policy.measure.as_ref().ok_or_else(|| error(Rule::Revision, "assigned-width content metrics missing"))?;
        let measure = provider.measure(&request)?;
        if measure.request != request || provider.revision() != env.content_revision {
            return Err(error(Rule::Revision, "assigned-width content metrics changed"));
        }
        env.content.push(measure);
    }
    // A content-sized side card still gets its full body when the actions
    // need their own row. Requests retain their original measurement keys.
    for measure in &mut env.content {
        let instance = &measure.request.instance;
        if measure.policy == SideHeightPolicy::Content
            && let Ok(section) = proposed.containing(instance)
        {
            let spec = service.registry().get(&proposed.instance(instance)?.kind)?;
            let collapsible = section_collapsible(proposed, service.registry(), section)?;
            if header_actions_below(policy, metrics, section, instance, spec, collapsible, measure.request.width) {
                measure.needed_height = (measure.needed_height + metrics.control_extent).min(MAX_LOGICAL);
            }
        }
    }
    let allocation = allocate_layout(proposed, service.registry(), &env)?;
    let source_state = service.inspect();
    let mut result = HostLayout {
        resolved: ResolvedLayout::default(),
        sections: Vec::new(),
        sashes: Vec::new(),
        rails: Vec::new(),
        overflow: Vec::new(),
        overflow_launcher: None,
        controls: Vec::new(),
        render_domain,
        capacity,
        chrome_obstacles: usable.obstacles.clone(),
        usable,
        viewport: positive_rect(render_domain.origin.x, render_domain.origin.y, render_domain.size.width, render_domain.size.height),
        painted: Vec::new(),
        input: Vec::new(),
        occlusion: Vec::new(),
        reservations: Vec::new(),
        revision: source_state.revision,
        direction,
        metrics,
        geometry_policy: policy.clone(),
        extent: allocation.extent,
        allocation_environment: env,
        source_state,
        registry_specs: service.registry().specs().cloned().collect(),
    };
    for card in allocation.sections {
        let section = proposed.section(&card.section)?;
        append_section(&mut result, service, proposed, section, &card.instance, card.outer, policy)?;
    }
    append_sashes(&mut result, proposed)?;
    for section in &proposed.sections {
        if !result.sections.iter().any(|shown| shown.section == section.id) {
            result.resolved.unpresented.push(UnpresentedSection {
                section: section.id.clone(),
                reason: if section.location.zone() == Zone::Window && !service.capabilities().native_windows {
                    UnpresentedReason::NativeUnsupported
                } else {
                    UnpresentedReason::Unavailable
                },
            });
        }
    }
    result.reservations = result.painted.clone();
    let obstacles: Vec<_> = result.chrome_obstacles.iter().chain(&result.reservations).copied().collect();
    result.usable = resolve_usable_region(render_domain, &obstacles)?;
    result.validate()?;
    Ok(result)
}

fn validate_capacity(capacity: LogicalRect) -> PanelResult<()> {
    validate_allocation_capacity(capacity)
}

fn effective_surface(service: &LayoutService, proposed: &Layout, section: &Section, policy: &HostGeometryPolicy) -> PanelResult<PanelSurface> {
    let mut specs = Vec::new();
    for instance in &section.tabs {
        specs.push(service.registry().get(&proposed.instance(instance)?.kind)?);
    }
    let declared = group_surface(section.location.zone(), specs);
    if declared == PanelSurface::Opaque {
        return Ok(PanelSurface::Opaque);
    }
    if let Some(surface) = policy.effective_surfaces.get(&section.id)
        && *surface != PanelSurface::FollowZone
    {
        return Ok(*surface);
    }
    Ok(if policy.default_opaque { PanelSurface::Opaque } else { declared })
}

fn header_action_visible(policy: &HostGeometryPolicy, instance: &InstanceId, action: &ControlId) -> bool {
    !policy.hidden_header_actions.get(instance).is_some_and(|hidden| hidden.contains(action))
}

fn header_actions_below(
    policy: &HostGeometryPolicy,
    metrics: HostMetrics,
    section: &Section,
    instance: &InstanceId,
    spec: &PanelSpec,
    collapsible: bool,
    width: f32,
) -> bool {
    let count = spec.header_actions.iter().filter(|action| header_action_visible(policy, instance, &action.id)).count();
    let primary = 1 + usize::from(collapsible) + usize::from(spec.features.hot_closable);
    count > 0
        && (section.tabs.len() > 1
            || policy
                .header_title_widths
                .get(instance)
                .is_some_and(|title| width < title + metrics.control_extent * (primary + count) as f32 + solidcraft_theme::Tokens::get().space_2))
}

fn append_section(
    out: &mut HostLayout,
    service: &LayoutService,
    proposed: &Layout,
    section: &Section,
    instance: &InstanceId,
    outer: LogicalRect,
    policy: &HostGeometryPolicy,
) -> PanelResult<()> {
    outer.validate()?;
    let metrics = out.metrics;
    let spec = service.registry().get(&proposed.instance(instance)?.kind)?;
    let collapsible = section_collapsible(proposed, service.registry(), section)?;
    let (flow, _) = responsive(section.location.zone(), outer.size, None)?;
    // A single row owns inline controls AND content; no hidden second body.
    let inline = !section.collapsed && flow == Flow::Wide && outer.size.height <= metrics.inline_max_height;
    let header =
        rect(outer.origin.x, outer.origin.y, outer.size.width, if inline { outer.size.height } else { metrics.header_height.min(outer.size.height) });
    let visible_actions = spec.header_actions.iter().filter(|action| header_action_visible(policy, instance, &action.id));
    let action_count = visible_actions.clone().count();
    let primary_count = 1 + usize::from(collapsible) + usize::from(spec.features.hot_closable);
    let control_count = action_count + primary_count;
    let split = !inline && !section.collapsed && header_actions_below(policy, metrics, section, instance, spec, collapsible, header.size.width);
    let action_bar = (split && !section.collapsed)
        .then(|| {
            positive_rect(
                header.origin.x,
                bottom(header),
                header.size.width,
                metrics.control_extent.min((bottom(outer) - bottom(header) - metrics.footer_height).max(0.0)),
            )
        })
        .flatten();
    let extent = metrics.control_extent.min(header.size.height).min(header.size.width / control_count as f32);
    let primary_extent = if split { metrics.control_extent.min(header.size.height).min(header.size.width / primary_count as f32) } else { extent };
    let header_controls = if split { primary_count } else { control_count };
    let edge_margin = ((header.size.height - primary_extent) / 2.0).min((header.size.width - header_controls as f32 * primary_extent).max(0.0));
    let mut cursor = if out.direction == Direction::Ltr { right(header) - edge_margin } else { header.origin.x + edge_margin };
    let take = |cursor: &mut f32, row: LogicalRect, extent: f32| {
        let x = if out.direction == Direction::Ltr { *cursor - extent } else { *cursor };
        *cursor += if out.direction == Direction::Ltr { -extent } else { extent };
        rect(x, row.origin.y + (row.size.height - extent) / 2.0, extent, extent)
    };
    let close = spec.features.hot_closable.then(|| take(&mut cursor, header, primary_extent));
    let collapse = collapsible.then(|| take(&mut cursor, header, primary_extent));
    let menu = take(&mut cursor, header, primary_extent);
    let mut actions = Vec::new();
    let (heading, inline_content) = if inline {
        // Shared title/grip, declared custom actions, editable inline content,
        // then shared system actions. The whole sequence mirrors in RTL.
        let available = (header.size.width - edge_margin - extent * control_count as f32).max(0.0);
        let title_width = metrics.inline_title_width.min(available * 0.4);
        let mut leading = if out.direction == Direction::Ltr { header.origin.x } else { right(header) };
        let title_x = if out.direction == Direction::Ltr { leading } else { leading - title_width };
        let title = positive_rect(title_x, header.origin.y, title_width, header.size.height);
        leading += if out.direction == Direction::Ltr { title_width } else { -title_width };
        for action in visible_actions.clone() {
            let x = if out.direction == Direction::Ltr { leading } else { leading - extent };
            actions.push((action.id.clone(), rect(x, header.origin.y + (header.size.height - extent) / 2.0, extent, extent)));
            leading += if out.direction == Direction::Ltr { extent } else { -extent };
        }
        let content = if out.direction == Direction::Ltr {
            positive_rect(leading, header.origin.y, cursor - leading, header.size.height)
        } else {
            positive_rect(cursor, header.origin.y, leading - cursor, header.size.height)
        };
        (title, content)
    } else {
        if let Some(row) = action_bar {
            let margin = edge_margin.min(row.size.width / 2.0);
            let extent = metrics.control_extent.min(row.size.height).min((row.size.width - margin) / action_count as f32);
            let mut action_cursor = if out.direction == Direction::Ltr { right(row) - margin } else { row.origin.x + margin };
            for action in visible_actions.rev() {
                actions.push((action.id.clone(), take(&mut action_cursor, row, extent)));
            }
        } else if !split {
            let remaining = if out.direction == Direction::Ltr { cursor - header.origin.x } else { right(header) - cursor };
            let extent = extent.min(header.size.height).min(remaining / action_count.max(1) as f32);
            if extent > 0.0 {
                for action in visible_actions.rev() {
                    actions.push((action.id.clone(), take(&mut cursor, header, extent)));
                }
            }
        }
        actions.reverse();
        (
            positive_rect(
                if out.direction == Direction::Ltr { header.origin.x } else { cursor },
                header.origin.y,
                if out.direction == Direction::Ltr { cursor - header.origin.x } else { right(header) - cursor },
                header.size.height,
            ),
            None,
        )
    };
    // A group replaces the single title with one grip and text-only tabs in
    // that same slot. Collapsing never removes access to individual tabs.
    let (title, tab_strip) = if section.tabs.len() > 1 {
        if let Some(heading) = heading {
            let t = solidcraft_theme::Tokens::get();
            let grip_width = metrics.control_extent.min(t.space_10 + t.space_8).min(heading.size.width);
            let grip_x = if out.direction == Direction::Ltr { heading.origin.x } else { right(heading) - grip_width };
            let tabs_x = if out.direction == Direction::Ltr { grip_x + grip_width } else { heading.origin.x };
            (
                positive_rect(grip_x, heading.origin.y, grip_width, heading.size.height),
                positive_rect(tabs_x, heading.origin.y, heading.size.width - grip_width, heading.size.height),
            )
        } else {
            (None, None)
        }
    } else {
        (heading, None)
    };
    let body_y = bottom(action_bar.unwrap_or(header)) + metrics.content_header_height;
    let body = if section.collapsed || inline {
        None
    } else {
        positive_rect(outer.origin.x, body_y, outer.size.width, bottom(outer) - metrics.footer_height - body_y)
    };
    let footer = if section.collapsed || inline {
        None
    } else {
        positive_rect(outer.origin.x, bottom(outer) - metrics.footer_height, outer.size.width, metrics.footer_height)
    };
    let mut tabs = Vec::new();
    let mut hidden_tabs = Vec::new();
    let mut tab_overflow = None;
    if let Some(strip) = tab_strip {
        let preferred = |id: &InstanceId| policy.tab_title_widths.get(id).copied().unwrap_or(metrics.tab_min_width);
        let minimum = |id: &InstanceId| preferred(id).min(metrics.tab_min_width);
        let required: f32 = section.tabs.iter().map(minimum).sum();
        let overflowed = required > strip.size.width;
        let overflow_width = if overflowed { metrics.control_extent.min(strip.size.width) } else { 0.0 };
        let room = strip.size.width - overflow_width;
        let mut visible = BTreeSet::new();
        if !overflowed {
            visible.extend(section.tabs.iter().cloned());
        } else if room >= minimum(instance) {
            visible.insert(instance.clone());
            let mut remaining = room - minimum(instance);
            for id in section.tabs.iter().filter(|id| *id != instance) {
                let width = minimum(id);
                if width > remaining {
                    break;
                }
                remaining -= width;
                visible.insert(id.clone());
            }
        }
        let required: f32 = visible.iter().map(minimum).sum();
        let desired: f32 = visible.iter().map(preferred).sum();
        let fraction = if desired > required { ((room - required) / (desired - required)).clamp(0.0, 1.0) } else { 0.0 };
        let mut cursor = if out.direction == Direction::Ltr { strip.origin.x } else { right(strip) };
        for id in &section.tabs {
            if visible.contains(id) {
                let remaining =
                    if out.direction == Direction::Ltr { right(strip) - overflow_width - cursor } else { cursor - strip.origin.x - overflow_width };
                let cell_width = (minimum(id) + (preferred(id) - minimum(id)) * fraction).min(remaining);
                let x = if out.direction == Direction::Ltr { cursor } else { cursor - cell_width };
                tabs.push(HostTab { instance: id.clone(), bounds: rect(x, strip.origin.y, cell_width, strip.size.height) });
                cursor = if out.direction == Direction::Ltr { x + cell_width } else { x };
            } else {
                hidden_tabs.push(id.clone());
            }
        }
        if overflowed {
            tab_overflow = positive_rect(
                if out.direction == Direction::Ltr { right(strip) - overflow_width } else { strip.origin.x },
                strip.origin.y,
                overflow_width,
                strip.size.height,
            );
        }
    } else if section.tabs.len() > 1 {
        hidden_tabs = section.tabs.clone();
    }
    let surface = effective_surface(service, proposed, section, policy)?;
    let painted: Vec<_> = intersect_rect(outer, out.capacity).into_iter().collect();
    let input = painted
        .iter()
        .map(|bounds| InputRegion {
            control: ControlId(instance.0.clone()),
            role: InputRole::Surface,
            bounds: *bounds,
            keyboard_focusable: true,
            accessible_name: spec.title_key.clone(),
        })
        .collect();
    out.resolved.panels.push(ResolvedPanel {
        instance: instance.clone(),
        section: section.id.clone(),
        zone: section.location.zone(),
        outer,
        body,
        painted: painted.clone(),
        input,
    });
    out.painted.extend(&painted);
    out.occlusion.extend(&painted);
    out.input.extend(&painted);
    add_control(out, HostPointerTarget::Surface { instance: instance.clone() }, outer, InputRole::Surface, "ui.panel_host.surface");
    if let Some(title) = title {
        add_control(out, HostPointerTarget::Header { instance: instance.clone() }, title, InputRole::Header, "ui.panel_host.header");
    }
    add_control(out, HostPointerTarget::Menu { instance: instance.clone() }, menu, InputRole::Action, "ui.panel_host.menu");
    if let Some(collapse) = collapse {
        add_control(out, HostPointerTarget::Collapse { instance: instance.clone() }, collapse, InputRole::Action, "ui.panel_host.collapse");
    }
    if let Some(close) = close {
        add_control(out, HostPointerTarget::Close { instance: instance.clone() }, close, InputRole::Action, "ui.panel_host.close");
    }
    for (id, bounds) in &actions {
        let name = spec.header_actions.iter().find(|action| action.id == *id).map_or("ui.panel_host.menu", |action| action.label_key.as_str());
        add_control(out, HostPointerTarget::HeaderAction { instance: instance.clone(), action: id.clone() }, *bounds, InputRole::Action, name);
    }
    for tab in &tabs {
        add_control(out, HostPointerTarget::Tab { instance: tab.instance.clone() }, tab.bounds, InputRole::Tab, "ui.panel_host.tab");
    }
    if let Some(bounds) = tab_overflow {
        add_control(out, HostPointerTarget::TabOverflow { section: section.id.clone() }, bounds, InputRole::Action, "ui.panel_host.overflow");
    }
    // Workspace paints the card's resize response last, above header/content.
    // Reverse geometric hit-testing must choose the same owner at their edges.
    let edge = match section.location.zone() {
        Zone::Floating => HostResizeEdge::BottomRight,
        Zone::Bottom => HostResizeEdge::Top,
        Zone::Left => HostResizeEdge::Right,
        _ => HostResizeEdge::Left,
    };
    let (min, max) = tab_bounds(proposed, service.registry(), section)?;
    if !section.collapsed && !(section.location.zone() == Zone::Bottom && min.height == max.height) {
        add_control(
            out,
            HostPointerTarget::Resize { section: section.id.clone(), edge },
            resize_rect(outer, edge.physical(section.location.zone(), out.direction), metrics.sash_width),
            InputRole::Sash,
            "ui.panel_host.resize",
        );
    }
    let floating = section.location.zone() == Zone::Floating;
    let appearance = policy.surfaces.as_ref().map(|style| style.appearance(surface, floating));
    let paint_alpha = appearance
        .map(|appearance| appearance.fill.a())
        .or_else(|| policy.surface_alpha.map(|alpha| if floating { alpha.floating } else { alpha.docked }));
    let contrast = appearance.and_then(|appearance| appearance.contrast.clone());
    out.sections.push(HostSection {
        contrast,
        paint_alpha,
        section: section.id.clone(),
        instance: instance.clone(),
        zone: section.location.zone(),
        outer,
        header,
        title,
        menu,
        action_bar,
        tab_strip,
        tabs,
        hidden_tabs,
        tab_overflow,
        body,
        footer,
        peek: false,
        surface,
        collapsed: section.collapsed,
        inline,
        inline_content,
        collapse,
        close,
        actions,
    });
    Ok(())
}

fn append_sashes(out: &mut HostLayout, proposed: &Layout) -> PanelResult<()> {
    let mut groups: BTreeMap<(Zone, Option<ColumnId>), Vec<SectionId>> = BTreeMap::new();
    // Allocation order is already per presented column/Bottom row. Unavailable
    // members occupy no space; collapsed headers remain intervening members.
    for shown in &out.sections {
        if shown.zone.docked() {
            let section = proposed.section(&shown.section)?;
            groups.entry((shown.zone, section.column.clone())).or_default().push(section.id.clone());
        }
    }
    for members in groups.values() {
        for pair in members.windows(2) {
            let (Some(first), Some(second)) = (pair.first(), pair.get(1)) else {
                continue;
            };
            let a = out.sections.iter().find(|shown| shown.section == *first);
            let b = out.sections.iter().find(|shown| shown.section == *second);
            if let (Some(a), Some(b)) = (a, b) {
                if a.collapsed || b.collapsed {
                    continue;
                }
                let zone = a.zone;
                let bounds = if zone == Zone::Bottom {
                    let left = if out.direction == Direction::Ltr { a.outer } else { b.outer };
                    let right_card = if out.direction == Direction::Ltr { b.outer } else { a.outer };
                    let gap = right_card.origin.x - right(left);
                    let width = if out.metrics.gap == 0.0 { out.metrics.sash_width.min(left.size.width).min(right_card.size.width) } else { gap };
                    positive_rect(
                        right(left) - if out.metrics.gap == 0.0 { width / 2.0 } else { 0.0 },
                        left.origin.y.max(right_card.origin.y),
                        width,
                        left.size.height.min(right_card.size.height),
                    )
                } else {
                    let gap = b.outer.origin.y - bottom(a.outer);
                    let height = if out.metrics.gap == 0.0 { out.metrics.sash_width.min(a.outer.size.height).min(b.outer.size.height) } else { gap };
                    positive_rect(
                        a.outer.origin.x,
                        bottom(a.outer) - if out.metrics.gap == 0.0 { height / 2.0 } else { 0.0 },
                        a.outer.size.width,
                        height,
                    )
                };
                if let Some(bounds) = bounds.and_then(|bounds| intersect_rect(bounds, out.capacity)) {
                    out.sashes.push(HostSash { first: first.clone(), second: second.clone(), zone, bounds });
                    out.painted.push(bounds);
                    out.occlusion.push(bounds);
                    out.input.push(bounds);
                    add_control(
                        out,
                        HostPointerTarget::Sash { first: first.clone(), second: second.clone() },
                        bounds,
                        InputRole::Sash,
                        "ui.panel_host.sash",
                    );
                }
            }
        }
    }
    Ok(())
}

fn add_control(out: &mut HostLayout, target: HostPointerTarget, bounds: LogicalRect, role: InputRole, name: &str) {
    if let Some(bounds) = intersect_rect(bounds, out.capacity) {
        out.controls.push(HostControl { target, bounds, role, accessible_name: name.into() });
    }
}

fn resize_rect(outer: LogicalRect, edge: HostResizeEdge, thickness: f32) -> LogicalRect {
    let width = thickness.min(outer.size.width);
    let height = thickness.min(outer.size.height);
    match edge {
        HostResizeEdge::Left => rect(outer.origin.x, outer.origin.y, width, outer.size.height),
        HostResizeEdge::Right => rect(right(outer) - width, outer.origin.y, width, outer.size.height),
        HostResizeEdge::Top => rect(outer.origin.x, outer.origin.y, outer.size.width, height),
        HostResizeEdge::Bottom => rect(outer.origin.x, bottom(outer) - height, outer.size.width, height),
        HostResizeEdge::BottomRight => rect(right(outer) - width, bottom(outer) - height, width, height),
    }
}

pub(super) fn rect(x: f32, y: f32, width: f32, height: f32) -> LogicalRect {
    LogicalRect { origin: Point { x, y }, size: Size { width, height } }
}
pub(super) fn positive_rect(x: f32, y: f32, width: f32, height: f32) -> Option<LogicalRect> {
    let bounds = rect(x, y, width, height);
    if bounds.validate().is_ok() { Some(bounds) } else { None }
}
pub(super) fn right(bounds: LogicalRect) -> f32 {
    bounds.origin.x + bounds.size.width
}
pub(super) fn bottom(bounds: LogicalRect) -> f32 {
    bounds.origin.y + bounds.size.height
}
