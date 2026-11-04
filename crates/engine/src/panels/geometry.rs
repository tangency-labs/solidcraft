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
/// Free/native flow starts at aspect 1, switches Tall->Wide above 1.1, Wide->Tall
/// below 0.9, and retains the previous flow inside that band.
pub fn responsive(zone: Zone, usable: Size, previous: Option<Flow>) -> PanelResult<(Flow, SizeClass)> {
    usable.validate()?;
    let aspect = usable.width / usable.height;
    let flow = match zone {
        Zone::Left | Zone::Right => Flow::Tall,
        Zone::Bottom => Flow::Wide,
        Zone::Floating | Zone::Window => match previous {
            Some(Flow::Tall) if aspect <= 1.1 => Flow::Tall,
            Some(Flow::Wide) if aspect >= 0.9 => Flow::Wide,
            _ if aspect >= 1.0 => Flow::Wide,
            _ => Flow::Tall,
        },
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
    pub body: LogicalRect,
    pub painted: Vec<LogicalRect>,
    pub input: Vec<InputRegion>,
}
impl ResolvedPanel {
    pub fn validate(&self) -> PanelResult<()> {
        valid_id(&self.instance.0)?;
        valid_id(&self.section.0)?;
        self.outer.validate()?;
        self.body.validate()?;
        if !self.outer.contains_rect(self.body) {
            return Err(error(Rule::Bounds, "usable body outside outer host"));
        }
        if self.painted.is_empty() || self.painted.len() > MAX_TABS || self.input.len() > MAX_INSTANCES {
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

/// Reference geometry for synthetic mounts. Production docking owns its richer
/// resolver; both return the same meaningful geometry contract. No layout writes.
pub fn resolve_geometry(service: &LayoutService, capacity: LogicalRect, direction: Direction, chrome: f32) -> PanelResult<ResolvedLayout> {
    capacity.validate()?;
    if !chrome.is_finite() || chrome < 0.0 || chrome > 256.0 {
        return Err(error(Rule::Bounds, "invalid header/footer chrome extent"));
    }
    let mut result = ResolvedLayout::default();
    let mut left = capacity.origin.x;
    let mut right = capacity.origin.x + capacity.size.width;
    let mut bottom = capacity.origin.y + capacity.size.height;
    let mut left_y = capacity.origin.y;
    let mut right_y = capacity.origin.y;
    let mut bottom_x = capacity.origin.x;
    let mut rail_y = capacity.origin.y;
    let mut occupied: Vec<LogicalRect> = Vec::new();
    for section in &service.layout().sections {
        let id = &section.active;
        let zone = section.location.zone();
        let peek = &service.interaction().peek;
        let active = peek.as_ref().filter(|p| section.tabs.contains(p)).unwrap_or(id);
        let available = service.availability(active) == Availability::Available;
        let reason = if !available {
            Some(UnpresentedReason::Unavailable)
        } else if zone == Zone::Window && !service.capabilities().native_windows {
            Some(UnpresentedReason::NativeUnsupported)
        } else if section.collapsed && peek.as_ref() != Some(active) {
            Some(UnpresentedReason::Collapsed)
        } else {
            None
        };
        if let Some(reason) = reason {
            result.unpresented.push(UnpresentedSection { section: section.id.clone(), reason: reason.clone() });
            if reason == UnpresentedReason::Collapsed {
                append_rail(&mut result, service, active, capacity, direction, zone, &mut rail_y)?;
            }
            continue;
        }
        let size = section.size;
        let origin = match &section.location {
            SectionLocation::Floating { origin } | SectionLocation::Window { origin, .. } => *origin,
            SectionLocation::Dock { zone } => match direction.edge(*zone)? {
                PhysicalEdge::Left => Point { x: left, y: left_y },
                PhysicalEdge::Right => Point { x: right - size.width, y: right_y },
                PhysicalEdge::Bottom => Point { x: bottom_x, y: bottom - size.height },
            },
        };
        let outer = LogicalRect { origin, size };
        let overlaps_dock = zone.docked()
            && !section.collapsed
            && occupied.iter().any(|r| {
                r.origin.x < origin.x + size.width
                    && origin.x < r.origin.x + r.size.width
                    && r.origin.y < origin.y + size.height
                    && origin.y < r.origin.y + r.size.height
            });
        if !capacity.contains_rect(outer) || size.height <= chrome || overlaps_dock {
            result.unpresented.push(UnpresentedSection { section: section.id.clone(), reason: UnpresentedReason::MinimumOverflow });
            append_rail(&mut result, service, active, capacity, direction, zone, &mut rail_y)?;
            continue;
        }
        if zone.docked() && !section.collapsed {
            occupied.push(outer);
            match direction.edge(zone)? {
                PhysicalEdge::Left => {
                    left_y += size.height;
                    left = left.max(origin.x);
                }
                PhysicalEdge::Right => {
                    right_y += size.height;
                    right = right.min(origin.x + size.width);
                }
                PhysicalEdge::Bottom => {
                    bottom_x += size.width;
                    bottom = bottom.min(origin.y + size.height);
                }
            }
        }
        let body =
            LogicalRect { origin: Point { x: origin.x, y: origin.y + chrome / 2.0 }, size: Size { width: size.width, height: size.height - chrome } };
        let panel = ResolvedPanel {
            instance: active.clone(),
            section: section.id.clone(),
            zone,
            outer,
            body,
            painted: vec![outer],
            input: vec![InputRegion {
                control: ControlId::new(format!("host.{}", section.id.0).chars().take(MAX_ID_BYTES).collect::<String>())?,
                role: InputRole::Surface,
                bounds: outer,
                keyboard_focusable: true,
                accessible_name: service.registry().get(&service.layout().instance(active)?.kind)?.title_key.clone(),
            }],
        };
        panel.validate()?;
        result.panels.push(panel);
    }
    result.validate()?;
    Ok(result)
}

fn append_rail(
    result: &mut ResolvedLayout,
    service: &LayoutService,
    active: &InstanceId,
    capacity: LogicalRect,
    direction: Direction,
    zone: Zone,
    rail_y: &mut f32,
) -> PanelResult<()> {
    // Rails are bounded chrome, not below-minimum content bodies. If every entry
    // cannot fit, the complete unpresented list remains available to a host's rail.
    let width = 32.0_f32.min(capacity.size.width);
    let height = 32.0_f32.min(capacity.size.height);
    if *rail_y + height <= capacity.origin.y + capacity.size.height {
        let right = direction.edge(if zone.docked() { zone } else { Zone::Left })? == PhysicalEdge::Right;
        let x = if right { capacity.origin.x + capacity.size.width - width } else { capacity.origin.x };
        result.rails.push(InputRegion {
            control: ControlId::new(format!("rail.{}", result.rails.len()))?,
            role: InputRole::Rail,
            bounds: LogicalRect { origin: Point { x, y: *rail_y }, size: Size { width, height } },
            keyboard_focusable: true,
            accessible_name: service.registry().get(&service.layout().instance(active)?.kind)?.title_key.clone(),
        });
        *rail_y += height;
    }
    Ok(())
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
    for spec in specs {
        if spec.surface == PanelSurface::Opaque {
            return PanelSurface::Opaque;
        }
        translucent |= spec.surface == PanelSurface::Translucent;
    }
    if translucent || !matches!(zone, Zone::Bottom | Zone::Window) { PanelSurface::Translucent } else { PanelSurface::Opaque }
}
