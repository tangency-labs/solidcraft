use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}
impl Point {
    pub fn validate(self) -> PanelResult<()> {
        if !self.x.is_finite() || !self.y.is_finite() || self.x.abs() > MAX_LOGICAL || self.y.abs() > MAX_LOGICAL {
            return Err(error(Rule::Bounds, "invalid logical position"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LogicalRect {
    pub origin: Point,
    pub size: Size,
}
impl LogicalRect {
    pub fn validate(self) -> PanelResult<()> {
        self.origin.validate()?;
        self.size.validate()?;
        if (self.origin.x + self.size.width).abs() > MAX_LOGICAL || (self.origin.y + self.size.height).abs() > MAX_LOGICAL {
            return Err(error(Rule::Bounds, "logical rectangle extent out of range"));
        }
        Ok(())
    }
    pub fn contains(self, p: Point) -> bool {
        p.x >= self.origin.x && p.y >= self.origin.y && p.x <= self.origin.x + self.size.width && p.y <= self.origin.y + self.size.height
    }
    pub fn contains_rect(self, other: Self) -> bool {
        self.contains(other.origin) && self.contains(Point { x: other.origin.x + other.size.width, y: other.origin.y + other.size.height })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Flow {
    Tall,
    Wide,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SizeClass {
    Compact,
    Regular,
    Wide,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Ltr,
    Rtl,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PhysicalEdge {
    Left,
    Right,
    Bottom,
}

impl Direction {
    pub fn edge(self, zone: Zone) -> PanelResult<PhysicalEdge> {
        match (self, zone) {
            (Self::Ltr, Zone::Left) | (Self::Rtl, Zone::Right) => Ok(PhysicalEdge::Left),
            (Self::Ltr, Zone::Right) | (Self::Rtl, Zone::Left) => Ok(PhysicalEdge::Right),
            (_, Zone::Bottom) => Ok(PhysicalEdge::Bottom),
            _ => Err(error(Rule::Placement, "free host has no logical dock edge")),
        }
    }
}

/// Usable logical units: egui applies scale exactly once, outside this contract.
/// Free/native flow follows the assigned logical aspect. The previous argument
/// remains for source compatibility; a stretched parent never determines flow.
pub fn responsive(zone: Zone, usable: Size, _previous: Option<Flow>) -> PanelResult<(Flow, SizeClass)> {
    usable.validate()?;
    let aspect = usable.width / usable.height;
    let flow = match zone {
        Zone::Left | Zone::Right => Flow::Tall,
        Zone::Bottom => Flow::Wide,
        Zone::Floating | Zone::Window => {
            if aspect >= 1.0 {
                Flow::Wide
            } else {
                Flow::Tall
            }
        }
    };
    let shorter = usable.width.min(usable.height);
    let class = if shorter < 240.0 {
        SizeClass::Compact
    } else if shorter <= 480.0 {
        SizeClass::Regular
    } else {
        SizeClass::Wide
    };
    Ok((flow, class))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InputRole {
    Surface,
    Header,
    Tab,
    Sash,
    Action,
    Rail,
    Overlay,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InputRegion {
    pub control: ControlId,
    pub role: InputRole,
    pub bounds: LogicalRect,
    pub keyboard_focusable: bool,
    /// Translation key or a runtime title resolved by the host.
    pub accessible_name: String,
}
impl InputRegion {
    pub fn validate(&self) -> PanelResult<()> {
        valid_id(&self.control.0)?;
        self.bounds.validate()?;
        if self.accessible_name.is_empty() || self.accessible_name.len() > 256 {
            return Err(error(Rule::Bounds, "accessible name requires 1..256 bytes"));
        }
        Ok(())
    }
}

/// Host-resolved geometry has separate outer, body, paint and input bounds. No
/// transparent/full-window input shield is implied by an overlay surface.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ResolvedPanel {
    pub instance: InstanceId,
    pub section: SectionId,
    pub zone: Zone,
    pub outer: LogicalRect,
    pub body: Option<LogicalRect>,
    pub painted: Vec<LogicalRect>,
    pub input: Vec<InputRegion>,
}
impl ResolvedPanel {
    pub fn validate(&self) -> PanelResult<()> {
        valid_id(&self.instance.0)?;
        valid_id(&self.section.0)?;
        self.outer.validate()?;
        if let Some(body) = self.body {
            body.validate()?;
            if !self.outer.contains_rect(body) {
                return Err(error(Rule::Bounds, "usable body outside outer host"));
            }
        }
        if self.painted.len() > MAX_TABS || self.input.len() > MAX_INSTANCES {
            return Err(error(Rule::CollectionLimit, "resolved region count"));
        }
        for rect in &self.painted {
            rect.validate()?;
            if !self.outer.contains_rect(*rect) {
                return Err(error(Rule::Bounds, "paint outside host"));
            }
        }
        let mut ids = std::collections::BTreeSet::new();
        for region in &self.input {
            region.validate()?;
            if !ids.insert(&region.control) {
                return Err(error(Rule::DuplicateId, "semantic control id repeated"));
            }
            if !self.painted.iter().any(|r| r.contains_rect(region.bounds)) {
                return Err(error(Rule::Bounds, "input must be contained in painted area"));
            }
        }
        Ok(())
    }
    pub fn consumes_pointer(&self, at: Point) -> bool {
        self.input.iter().any(|r| r.bounds.contains(at))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UnpresentedReason {
    MinimumOverflow,
    Unavailable,
    NativeUnsupported,
    Collapsed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UnpresentedSection {
    pub section: SectionId,
    pub reason: UnpresentedReason,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ResolvedLayout {
    pub panels: Vec<ResolvedPanel>,
    /// Accessible rail controls are chrome, never sub-minimum content bodies.
    pub rails: Vec<InputRegion>,
    pub unpresented: Vec<UnpresentedSection>,
}
impl ResolvedLayout {
    pub fn validate(&self) -> PanelResult<()> {
        if self.panels.len() > MAX_SECTIONS || self.rails.len() > MAX_SECTIONS || self.unpresented.len() > MAX_SECTIONS {
            return Err(error(Rule::CollectionLimit, "resolved layout cap"));
        }
        let mut instances = std::collections::BTreeSet::new();
        let mut sections = std::collections::BTreeSet::new();
        let mut rails = std::collections::BTreeSet::new();
        for panel in &self.panels {
            panel.validate()?;
            if !instances.insert(&panel.instance) || !sections.insert(&panel.section) {
                return Err(error(Rule::ExactlyOnce, "duplicate resolved instance/section"));
            }
        }
        for rail in &self.rails {
            rail.validate()?;
            if rail.role != InputRole::Rail || !rails.insert(&rail.control) {
                return Err(error(Rule::DuplicateId, "invalid or duplicate rail control"));
            }
        }
        for record in &self.unpresented {
            valid_id(&record.section.0)?;
            if !sections.insert(&record.section) {
                return Err(error(Rule::ExactlyOnce, "section both presented/unpresented or duplicated"));
            }
        }
        Ok(())
    }
}

/// Width-dependent, side-effect-free measurement request. Heights include all
/// shared chrome, body, footer, padding and spacing at this exact assigned width.
#[derive(Clone, Debug, PartialEq)]
pub struct ContentMeasureRequest {
    pub instance: InstanceId,
    pub width: f32,
    pub chrome_height: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SideContentMeasure {
    pub request: ContentMeasureRequest,
    pub needed_height: f32,
    pub policy: SideHeightPolicy,
}

/// Immutable input to the allocator. Availability and measurements are captured
/// by the adapter; allocation never calls content or changes layout preferences.
#[derive(Clone, Debug, PartialEq)]
pub struct AllocationEnvironment {
    pub capacity: LogicalRect,
    pub direction: Direction,
    pub header_height: f32,
    pub footer_height: f32,
    pub gap: f32,
    pub inline_max_height: f32,
    pub min_viewport: Size,
    pub scroll: Point,
    pub available: Vec<InstanceId>,
    pub content: Vec<SideContentMeasure>,
    pub content_revision: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AllocatedSection {
    pub section: SectionId,
    pub instance: InstanceId,
    pub outer: LogicalRect,
    pub collapsed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AllocatedGeometry {
    pub sections: Vec<AllocatedSection>,
    pub extent: Size,
    pub scroll: Point,
}

impl AllocationEnvironment {
    pub fn validate(&self) -> PanelResult<()> {
        validate_allocation_capacity(self.capacity)?;
        self.scroll.validate()?;
        self.min_viewport.validate()?;
        if self.scroll.x < 0.0 || self.scroll.y < 0.0 {
            return Err(error(Rule::Bounds, "negative panel scroll"));
        }
        for value in [self.header_height, self.footer_height, self.gap] {
            if !value.is_finite() || !(0.0..=256.0).contains(&value) {
                return Err(error(Rule::Bounds, "invalid allocation chrome"));
            }
        }
        if self.header_height <= 0.0 {
            return Err(error(Rule::Bounds, "header height must be positive"));
        }
        if self.available.len() > MAX_INSTANCES || self.content.len() > MAX_INSTANCES {
            return Err(error(Rule::CollectionLimit, "allocation environment bound"));
        }
        if !self.inline_max_height.is_finite() || !(0.0..=88.0).contains(&self.inline_max_height) {
            return Err(error(Rule::Bounds, "invalid inline allocation threshold"));
        }
        let mut available = std::collections::BTreeSet::new();
        for id in &self.available {
            valid_id(&id.0)?;
            if !available.insert(id) {
                return Err(error(Rule::DuplicateId, "duplicate availability"));
            }
        }
        let mut measures = std::collections::BTreeSet::new();
        for measure in &self.content {
            valid_id(&measure.request.instance.0)?;
            if !measure.request.width.is_finite()
                || measure.request.width <= 0.0
                || measure.request.width > MAX_LOGICAL
                || !measure.request.chrome_height.is_finite()
                || measure.request.chrome_height < 0.0
                || !measure.needed_height.is_finite()
                || measure.needed_height < measure.request.chrome_height
                || measure.needed_height > MAX_LOGICAL
            {
                return Err(error(Rule::Bounds, "invalid intrinsic content measure"));
            }
            if !measures.insert((&measure.request.instance, measure.request.width.to_bits(), measure.request.chrome_height.to_bits())) {
                return Err(error(Rule::DuplicateId, "duplicate content measure"));
            }
        }
        Ok(())
    }
}

/// Capacity may be empty, unlike a positive panel/body rectangle.
pub fn validate_allocation_capacity(bounds: LogicalRect) -> PanelResult<()> {
    bounds.origin.validate()?;
    if !bounds.size.width.is_finite()
        || !bounds.size.height.is_finite()
        || bounds.size.width < 0.0
        || bounds.size.height < 0.0
        || bounds.size.width > MAX_LOGICAL
        || bounds.size.height > MAX_LOGICAL
        || (bounds.origin.x + bounds.size.width).abs() > MAX_LOGICAL
        || (bounds.origin.y + bounds.size.height).abs() > MAX_LOGICAL
    {
        return Err(error(Rule::Bounds, "invalid allocation capacity"));
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct AxisLimit {
    min: f32,
    max: f32,
    weight: f32,
    preferred: Option<f32>,
    priority: u16,
}

/// Bounded water filling starts at minima, redistributes only available surplus,
/// and leaves a real gap when every member reaches its maximum.
fn distribute(items: &[AxisLimit], available: f32) -> Vec<f32> {
    let mut sizes: Vec<_> = items.iter().map(|item| item.min).collect();
    let mut remaining = (f64::from(available) - sizes.iter().map(|v| f64::from(*v)).sum::<f64>()).max(0.0);
    let tiers: std::collections::BTreeSet<_> = items.iter().filter(|item| item.preferred.is_some()).map(|item| item.priority).collect();
    for tier in tiers.iter().rev() {
        let requested: f64 = items
            .iter()
            .filter(|item| item.priority == *tier)
            .map(|item| item.preferred.map_or(0.0, |wanted| f64::from(wanted.clamp(item.min, item.max)) - f64::from(item.min)))
            .sum();
        let factor = if requested > 0.0 { (remaining / requested).min(1.0) } else { 0.0 };
        for (item, size) in items.iter().zip(&mut sizes) {
            if item.priority == *tier
                && let Some(wanted) = item.preferred
            {
                let increment = (f64::from(wanted.clamp(item.min, item.max)) - f64::from(item.min)) * factor;
                let mut next = (f64::from(*size) + increment) as f32;
                if f64::from(next) > f64::from(*size) + increment {
                    next = f32::from_bits(next.to_bits().saturating_sub(1));
                }
                remaining = (remaining - (f64::from(next) - f64::from(*size))).max(0.0);
                *size = next;
            }
        }
        // This tier exhausted the mathematical budget. Inward f32 rounding
        // leaves real uncovered space, never priority for a lower tier/weight.
        if factor < 1.0 && requested > 0.0 {
            return sizes;
        }
    }
    for _ in 0..items.len() {
        let total: f64 = items
            .iter()
            .zip(&sizes)
            .filter(|(item, size)| item.preferred.is_none() && item.max > **size && item.weight > 0.0)
            .map(|(item, _)| f64::from(item.weight))
            .sum();
        if remaining <= 0.0 || total <= 0.0 {
            break;
        }
        let mut consumed = 0.0;
        for (item, size) in items.iter().zip(&mut sizes) {
            if item.preferred.is_none() && item.max > *size && item.weight > 0.0 {
                let increment = (remaining * f64::from(item.weight) / total).min(f64::from(item.max) - f64::from(*size));
                let mut next = (f64::from(*size) + increment) as f32;
                // Round surplus inward: a collection of f32 round-ups must not
                // advertise a capacity fit which actually crosses its boundary.
                if f64::from(next) > f64::from(*size) + increment {
                    next = f32::from_bits(next.to_bits().saturating_sub(1));
                }
                consumed += f64::from(next) - f64::from(*size);
                *size = next.min(item.max);
            }
        }
        if consumed <= 0.0 {
            break;
        }
        remaining = (remaining - consumed).max(0.0);
    }
    sizes
}

/// Deterministic presentation identity, without changing remembered activation.
pub fn presented_instance<'a>(section: &'a Section, available: &[InstanceId]) -> Option<&'a InstanceId> {
    if section.tabs.is_empty() || section.tabs.len() > MAX_TABS || available.len() > MAX_INSTANCES {
        return None;
    }
    if section.tabs.contains(&section.active) && available.contains(&section.active) {
        Some(&section.active)
    } else {
        section.tabs.iter().find(|id| available.contains(id))
    }
}

fn presented<'a>(section: &'a Section, env: &AllocationEnvironment) -> Option<&'a InstanceId> {
    presented_instance(section, &env.available)
}

struct AllocationColumn<'a> {
    column: &'a Column,
    members: Vec<&'a Section>,
    width: f32,
}

fn allocation_columns<'a>(layout: &'a Layout, registry: &PanelRegistry, env: &AllocationEnvironment) -> PanelResult<Vec<AllocationColumn<'a>>> {
    let mut columns = Vec::new();
    let mut limits = Vec::new();
    for zone in [Zone::Left, Zone::Right] {
        for column in layout.columns.iter().filter(|column| column.zone == zone) {
            let members: Vec<_> =
                layout.sections.iter().filter(|section| section.column.as_ref() == Some(&column.id) && presented(section, env).is_some()).collect();
            if members.is_empty() {
                continue;
            }
            // Unavailable members still constrain widths, just like inactive tabs.
            let (min, max) = column_bounds(layout, registry, &column.id)?;
            let preferred = column.width.clamp(min, max);
            limits.push(AxisLimit { min, max: preferred, weight: (preferred - min).max(0.001), preferred: None, priority: 0 });
            columns.push(AllocationColumn { column, members, width: 0.0 });
        }
    }
    let gaps = env.gap * columns.len().saturating_sub(1) as f32;
    for (column, width) in columns.iter_mut().zip(distribute(&limits, (env.capacity.size.width - gaps).max(0.0))) {
        column.width = width;
    }
    Ok(columns)
}

pub fn allocation_measure_requests(
    layout: &Layout,
    registry: &PanelRegistry,
    env: &AllocationEnvironment,
) -> PanelResult<Vec<ContentMeasureRequest>> {
    validate(layout, registry)?;
    env.validate()?;
    let mut requests = Vec::new();
    for column in allocation_columns(layout, registry, env)? {
        if let Some(section) = column.members.first().filter(|_| column.members.len() == 1)
            && !section.collapsed
            && let Some(instance) = presented(section, env)
        {
            requests.push(ContentMeasureRequest {
                instance: instance.clone(),
                width: column.width,
                chrome_height: env.header_height + env.footer_height,
            });
        }
    }
    Ok(requests)
}

fn section_height_limit(layout: &Layout, registry: &PanelRegistry, section: &Section, env: &AllocationEnvironment) -> PanelResult<AxisLimit> {
    if section.collapsed {
        return Ok(AxisLimit { min: env.header_height, max: env.header_height, weight: 0.0, preferred: None, priority: 0 });
    }
    let (min, max) = tab_bounds(layout, registry, section)?;
    let free_width = section.size.width.min(env.capacity.size.width).clamp(min.width, max.width);
    let free_height = section.size.height.min(env.capacity.size.height).clamp(min.height, max.height);
    let inline = (section.location.zone() == Zone::Bottom && section.size.height <= env.inline_max_height)
        || (matches!(section.location.zone(), Zone::Floating | Zone::Window) && free_height <= env.inline_max_height && free_width >= free_height);
    let chrome = if inline { env.header_height } else { env.header_height + env.footer_height };
    let min = min.height.max(chrome);
    if min > max.height {
        return Err(error(Rule::IncompatibleTabs, "chrome exceeds composite height"));
    }
    Ok(AxisLimit {
        min,
        max: max.height,
        weight: section.weight,
        preferred: if section.location.zone().side() { section.preferred_extent } else { None },
        priority: section.extent_priority,
    })
}

fn measure_for<'a>(env: &'a AllocationEnvironment, request: &ContentMeasureRequest) -> PanelResult<&'a SideContentMeasure> {
    env.content
        .iter()
        .find(|measure| measure.request == *request)
        .ok_or_else(|| error(Rule::Revision, "missing or stale assigned-width content metrics"))
}

/// The exact bounds used for allocation and host command/gesture admission.
pub fn effective_allocation_bounds(
    layout: &Layout,
    registry: &PanelRegistry,
    section: &Section,
    env: &AllocationEnvironment,
) -> PanelResult<(Size, Size)> {
    env.validate()?;
    if layout.section(&section.id)? != section {
        return Err(error(Rule::Revision, "allocation section became stale"));
    }
    let (mut min, mut max) = tab_bounds(layout, registry, section)?;
    if let Some(column) = &section.column {
        let (low, high) = column_bounds(layout, registry, column)?;
        min.width = low;
        max.width = high;
    }
    let height = section_height_limit(layout, registry, section, env)?;
    min.height = height.min;
    max.height = height.max;
    Ok((min, max))
}

/// Closest feasible shared Bottom thickness using proposed presentation bounds.
/// Both regimes delegate chrome/composite constraints to the same allocator.
pub fn bottom_resize_height(layout: &Layout, registry: &PanelRegistry, env: &AllocationEnvironment, requested: f32) -> PanelResult<f32> {
    validate(layout, registry)?;
    env.validate()?;
    if !requested.is_finite() {
        return Err(error(Rule::Bounds, "nonfinite Bottom thickness"));
    }
    let (registered_min, registered_max) = bottom_height_bounds(layout, registry)?;
    let expanded_min = f32::from_bits(env.inline_max_height.to_bits().saturating_add(1));
    let regions = [(registered_min, registered_max.min(env.inline_max_height)), (registered_min.max(expanded_min), registered_max)];
    let mut proposed = layout.clone();
    let mut best: Option<f32> = None;
    for (mut low, mut high) in regions {
        if low > high {
            continue;
        }
        let probe = requested.clamp(low, high);
        for section in &mut proposed.sections {
            if section.location.zone() == Zone::Bottom {
                section.size.height = probe;
            }
        }
        let mut compatible = true;
        for section in proposed
            .sections
            .iter()
            .filter(|section| section.location.zone() == Zone::Bottom && !section.collapsed && presented(section, env).is_some())
        {
            match effective_allocation_bounds(&proposed, registry, section, env) {
                Ok((min, max)) => {
                    low = low.max(min.height);
                    high = high.min(max.height);
                }
                Err(failure) if failure.rule == Rule::IncompatibleTabs => {
                    compatible = false;
                    break;
                }
                Err(failure) => return Err(failure),
            }
        }
        if !compatible || low > high {
            continue;
        }
        let next = requested.clamp(low, high);
        if best.is_none_or(|old| (f64::from(next) - f64::from(requested)).abs() < (f64::from(old) - f64::from(requested)).abs()) {
            best = Some(next);
        }
    }
    best.ok_or_else(|| error(Rule::IncompatibleTabs, "Bottom row has no feasible proposed thickness"))
}

/// Authoritative allocation for committed presentation and every proposal.
/// Extent is independent of clipping. Right and Bottom anchor to that extent,
/// so minima remain nonoverlapping even in an empty/tiny visible capacity.
pub fn allocate_layout(layout: &Layout, registry: &PanelRegistry, env: &AllocationEnvironment) -> PanelResult<AllocatedGeometry> {
    validate(layout, registry)?;
    env.validate()?;
    let columns = allocation_columns(layout, registry, env)?;
    let bottoms: Vec<_> =
        layout.sections.iter().filter(|section| section.location.zone() == Zone::Bottom && presented(section, env).is_some()).collect();
    let mut bottom_height = 0.0_f32;
    let mut bottom_limits = Vec::new();
    for section in &bottoms {
        let (min, max) = tab_bounds(layout, registry, section)?;
        let h = section_height_limit(layout, registry, section, env)?;
        bottom_height = bottom_height.max(section.size.height.clamp(h.min, h.max));
        bottom_limits.push(AxisLimit {
            min: min.width,
            max: max.width,
            weight: section.weight,
            preferred: if section.collapsed { None } else { section.preferred_extent },
            priority: section.extent_priority,
        });
    }
    let bottom_gap = if bottoms.is_empty() || columns.is_empty() { 0.0 } else { env.gap };
    let side_available = (env.capacity.size.height - bottom_height - bottom_gap).max(0.0);
    let mut heights = Vec::new();
    let mut side_height = 0.0_f32;
    for column in &columns {
        let mut limits = Vec::new();
        for section in &column.members {
            limits.push(section_height_limit(layout, registry, section, env)?);
        }
        if column.members.len() == 1
            && let (Some(section), Some(limit)) = (column.members.first(), limits.first_mut())
            && !section.collapsed
            && limit.preferred.is_none()
            && let Some(instance) = presented(section, env)
        {
            let request =
                ContentMeasureRequest { instance: instance.clone(), width: column.width, chrome_height: env.header_height + env.footer_height };
            let measure = measure_for(env, &request)?;
            // Without an explicit height, a sole panel follows content or fills the column.
            limit.weight = 1.0;
            if measure.policy == SideHeightPolicy::Content {
                limit.max = measure.needed_height.clamp(limit.min, limit.max);
            }
        }
        let gaps = env.gap * column.members.len().saturating_sub(1) as f32;
        let sizes = distribute(&limits, (side_available - gaps).max(0.0));
        side_height = side_height.max((sizes.iter().map(|size| f64::from(*size)).sum::<f64>() + f64::from(gaps)) as f32);
        heights.push(sizes);
    }
    let widths: Vec<_> = distribute(&bottom_limits, (env.capacity.size.width - env.gap * bottoms.len().saturating_sub(1) as f32).max(0.0));
    let columns_width =
        (columns.iter().map(|column| f64::from(column.width)).sum::<f64>() + f64::from(env.gap) * columns.len().saturating_sub(1) as f64) as f32;
    let bottom_width =
        (widths.iter().map(|width| f64::from(*width)).sum::<f64>() + f64::from(env.gap) * bottoms.len().saturating_sub(1) as f64) as f32;
    let mut free_min = Size { width: 0.0, height: 0.0 };
    for section in &layout.sections {
        if section.location.zone() == Zone::Floating && presented(section, env).is_some() {
            let (min, _) = tab_bounds(layout, registry, section)?;
            free_min.width = free_min.width.max(min.width);
            free_min.height = free_min.height.max(section_height_limit(layout, registry, section, env)?.min);
        }
    }
    let extent = Size {
        width: env.capacity.size.width.max(columns_width).max(bottom_width).max(free_min.width),
        height: env.capacity.size.height.max(side_height + bottom_height + bottom_gap).max(free_min.height),
    };
    if !extent.width.is_finite() || !extent.height.is_finite() || extent.width > MAX_LOGICAL || extent.height > MAX_LOGICAL {
        return Err(error(Rule::Bounds, "panel scroll extent exceeds bound"));
    }
    let scroll = Point {
        x: env.scroll.x.min((extent.width - env.capacity.size.width).max(0.0)),
        y: env.scroll.y.min((extent.height - env.capacity.size.height).max(0.0)),
    };
    let origin = Point { x: env.capacity.origin.x - scroll.x, y: env.capacity.origin.y - scroll.y };
    let mut result = AllocatedGeometry { sections: Vec::new(), extent, scroll };
    let mut offsets = [0.0_f64; 2];
    for (column, sizes) in columns.iter().zip(heights) {
        let side = usize::from(column.column.zone == Zone::Right);
        let offset = offsets.get(side).copied().unwrap_or(0.0);
        let x = (if env.direction.edge(column.column.zone)? == PhysicalEdge::Left {
            f64::from(origin.x) + offset
        } else {
            f64::from(origin.x) + f64::from(extent.width) - offset - f64::from(column.width)
        }) as f32;
        let mut y = f64::from(origin.y);
        for (section, height) in column.members.iter().zip(sizes) {
            if let Some(instance) = presented(section, env) {
                push_allocation(&mut result, section, instance, x, y as f32, column.width, height)?;
                y += f64::from(height) + f64::from(env.gap);
            }
        }
        if let Some(offset) = offsets.get_mut(side) {
            *offset += f64::from(column.width) + f64::from(env.gap);
        }
    }
    let mut cursor = 0.0_f64;
    for (section, width) in bottoms.iter().zip(widths) {
        let height_limit = section_height_limit(layout, registry, section, env)?;
        let height = bottom_height.clamp(height_limit.min, height_limit.max);
        let x = (if env.direction == Direction::Ltr {
            f64::from(origin.x) + cursor
        } else {
            f64::from(origin.x) + f64::from(extent.width) - cursor - f64::from(width)
        }) as f32;
        if let Some(instance) = presented(section, env) {
            push_allocation(&mut result, section, instance, x, origin.y + extent.height - height, width, height)?;
        }
        cursor += f64::from(width) + f64::from(env.gap);
    }
    for section in &layout.sections {
        if let SectionLocation::Floating { origin } = &section.location
            && let Some(instance) = presented(section, env)
        {
            let (min, max) = tab_bounds(layout, registry, section)?;
            let width = section.size.width.min(env.capacity.size.width).clamp(min.width, max.width);
            let height_limit = section_height_limit(layout, registry, section, env)?;
            let height = section.size.height.min(env.capacity.size.height).clamp(height_limit.min, height_limit.max);
            let x = origin.x.clamp(env.capacity.origin.x, (env.capacity.origin.x + env.capacity.size.width - width).max(env.capacity.origin.x))
                - scroll.x;
            let y = origin.y.clamp(env.capacity.origin.y, (env.capacity.origin.y + env.capacity.size.height - height).max(env.capacity.origin.y))
                - scroll.y;
            push_allocation(&mut result, section, instance, x, y, width, height)?;
        }
    }
    Ok(result)
}

fn push_allocation(
    result: &mut AllocatedGeometry,
    section: &Section,
    instance: &InstanceId,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> PanelResult<()> {
    let outer = LogicalRect { origin: Point { x, y }, size: Size { width, height } };
    outer.validate()?;
    result.sections.push(AllocatedSection { section: section.id.clone(), instance: instance.clone(), outer, collapsed: section.collapsed });
    Ok(())
}

/// Synthetic/reference convenience adapter. Product hosts capture real intrinsic
/// metrics through allocation_measure_requests, rather than using preferred size.
pub fn resolve_geometry(service: &LayoutService, capacity: LogicalRect, direction: Direction, chrome: f32) -> PanelResult<ResolvedLayout> {
    if !chrome.is_finite() || chrome <= 0.0 || chrome > 256.0 {
        return Err(error(Rule::Bounds, "invalid shared chrome height"));
    }
    let mut env = AllocationEnvironment {
        capacity,
        direction,
        header_height: chrome / 2.0,
        footer_height: chrome / 2.0,
        gap: 0.0,
        inline_max_height: 0.0,
        min_viewport: Size { width: 1.0, height: 1.0 },
        scroll: Point { x: 0.0, y: 0.0 },
        available: service
            .layout()
            .instances
            .iter()
            .filter(|instance| service.availability(&instance.id) == Availability::Available)
            .map(|instance| instance.id.clone())
            .collect(),
        content: Vec::new(),
        content_revision: service.inspect().revision,
    };
    for request in allocation_measure_requests(service.layout(), service.registry(), &env)? {
        let section = service.layout().containing(&request.instance)?;
        env.content.push(SideContentMeasure {
            needed_height: section.size.height.max(request.chrome_height),
            policy: service.registry().get(&service.layout().instance(&request.instance)?.kind)?.side_height_policy,
            request,
        });
    }
    let mut allocation = allocate_layout(service.layout(), service.registry(), &env)?;
    // The reference adapter may model explicit native hosts. They have their
    // own viewport and never consume the in-app allocator's capacity.
    if service.capabilities().native_windows {
        for section in &service.layout().sections {
            if let SectionLocation::Window { origin, .. } = &section.location
                && let Some(instance) = presented(section, &env)
            {
                let height = if section.collapsed { env.header_height } else { section.size.height };
                push_allocation(&mut allocation, section, instance, origin.x, origin.y, section.size.width, height)?;
            }
        }
    }
    let mut result = ResolvedLayout::default();
    for card in allocation.sections {
        let body_height = card.outer.size.height - chrome;
        let body = if card.collapsed || body_height <= 0.0 {
            None
        } else {
            Some(LogicalRect {
                origin: Point { x: card.outer.origin.x, y: card.outer.origin.y + chrome / 2.0 },
                size: Size { width: card.outer.size.width, height: body_height },
            })
        };
        let zone = service.layout().section(&card.section)?.location.zone();
        let painted = if zone == Zone::Window { vec![card.outer] } else { intersect_rect(card.outer, capacity).into_iter().collect::<Vec<_>>() };
        let title_key = service.registry().get(&service.layout().instance(&card.instance)?.kind)?.title_key.clone();
        let input = painted
            .iter()
            .map(|bounds| InputRegion {
                control: ControlId(card.instance.0.clone()),
                role: InputRole::Surface,
                bounds: *bounds,
                keyboard_focusable: true,
                accessible_name: title_key.clone(),
            })
            .collect();
        result.panels.push(ResolvedPanel { instance: card.instance, section: card.section, zone, outer: card.outer, body, painted, input });
    }
    for section in &service.layout().sections {
        if !result.panels.iter().any(|panel| panel.section == section.id) {
            result.unpresented.push(UnpresentedSection {
                section: section.id.clone(),
                reason: if section.location.zone() == Zone::Window {
                    if service.capabilities().native_windows { UnpresentedReason::Unavailable } else { UnpresentedReason::NativeUnsupported }
                } else {
                    UnpresentedReason::Unavailable
                },
            });
        }
    }
    result.validate()?;
    Ok(result)
}

pub fn intersect_rect(a: LogicalRect, b: LogicalRect) -> Option<LogicalRect> {
    if validate_allocation_capacity(a).is_err() || validate_allocation_capacity(b).is_err() {
        return None;
    }
    let x = a.origin.x.max(b.origin.x);
    let y = a.origin.y.max(b.origin.y);
    let width = (a.origin.x + a.size.width).min(b.origin.x + b.size.width) - x;
    let height = (a.origin.y + a.size.height).min(b.origin.y + b.size.height) - y;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    Some(LogicalRect { origin: Point { x, y }, size: Size { width, height } })
}

/// Surfaces child resolves theme tokens. Invalid authored pairs retain raw import
/// intent and last good tokens; they cannot be reported as accessible by recolouring.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ContrastOutcome {
    Readable { effective_background_alpha: f32 },
    RetainReadableFallback { reason: ContrastFailure },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContrastFailure {
    InvalidTokens,
    UnreadableAtFullOpacity,
}

impl ContrastOutcome {
    /// The surfaces adapter proves readability. This boundary enforces finite
    /// alpha and the policy that effective background alpha may only increase.
    pub fn validate(&self, requested_alpha: f32) -> PanelResult<()> {
        if !requested_alpha.is_finite() || !(0.0..=1.0).contains(&requested_alpha) {
            return Err(error(Rule::Bounds, "invalid requested background alpha"));
        }
        if let Self::Readable { effective_background_alpha } = self
            && (!effective_background_alpha.is_finite() || *effective_background_alpha < requested_alpha || *effective_background_alpha > 1.0)
        {
            return Err(error(Rule::Bounds, "resolved background alpha must be finite and only raised"));
        }
        Ok(())
    }
}

/// Shared surface precedence: Opaque wins, then explicit Translucent, then zone
/// default (bottom/native opaque, sides/free translucent). No colour implementation.
pub fn group_surface<'a>(zone: Zone, specs: impl IntoIterator<Item = &'a PanelSpec>) -> PanelSurface {
    let mut translucent = false;
    let mut text_heavy = false;
    let mut count = 0;
    for spec in specs {
        count += 1;
        text_heavy |= spec.text_heavy;
        if spec.surface == PanelSurface::Opaque {
            return PanelSurface::Opaque;
        }
        translucent |= spec.surface == PanelSurface::Translucent;
    }
    if translucent {
        PanelSurface::Translucent
    } else if matches!(zone, Zone::Bottom | Zone::Window) || (count > 1 && text_heavy) {
        PanelSurface::Opaque
    } else {
        PanelSurface::Translucent
    }
}
