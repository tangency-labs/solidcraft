//! Exact convex parallel boundaries with persistent extinct arc identities.
use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::TAU;

use serde::{Deserialize, Serialize};
use solidcraft_geom::{Seg2, Vec2};

use crate::{Curve, CurveKind, OffsetChain, Sketch, SketchError};
type Result<T> = std::result::Result<T, SketchError>;
const MAX_GROUPS: usize = 128;
const MAX_ROLES: usize = 512;
const MAX_TOTAL_ROLES: usize = 20_000;
const MAX_WORK: usize = 262_144;
fn invalid(message: &str) -> SketchError {
    SketchError::Invalid(message.into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OffsetRoleKind {
    Line,
    Arc,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OffsetRoleFlags {
    pub construction: bool,
    pub centerline: bool,
    pub reversed: bool,
    pub fixed: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub infinite: bool,
}
impl OffsetRoleFlags {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
    fn of(curve: &Curve) -> Self {
        Self {
            construction: curve.construction,
            centerline: curve.centerline,
            reversed: curve.reversed,
            fixed: curve.fixed,
            infinite: curve.infinite,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OffsetRole {
    pub output: String,
    pub kind: OffsetRoleKind,
    pub start: String,
    pub end: String,
    pub retired: bool,
    #[serde(default, skip_serializing_if = "OffsetRoleFlags::is_default")]
    pub flags: OffsetRoleFlags,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OffsetTopology {
    pub version: u8,
    pub id: String,
    pub roles: Vec<OffsetRole>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct OffsetCreated {
    pub association: String,
    pub curves: Vec<String>,
}

#[derive(Clone, Copy)]
struct Arc {
    centre: Vec2,
    radius: f64,
    start: f64,
    sweep: f64,
    centre_index: usize,
}
#[derive(Clone, Copy)]
struct Source {
    a: Vec2,
    b: Vec2,
    arc: Option<Arc>,
}
impl Source {
    fn tangent(&self, end: bool, orientation: f64) -> Option<Vec2> {
        match self.arc {
            Some(arc) => (if end { self.b } else { self.a } - arc.centre).normalized().map(|r| r.perp() * orientation),
            None => (self.b - self.a).normalized(),
        }
    }
}
struct Boundary {
    source: Vec<Source>,
    orientation: f64,
    pad: f64,
}
#[derive(Clone, Copy)]
struct Span {
    a: Vec2,
    b: Vec2,
    arc: Option<Arc>,
    retired: bool,
}
fn angle_in(angle: f64, arc: Arc) -> bool {
    (angle - arc.start).rem_euclid(TAU) <= arc.sweep + 1e-12
}
fn max_dot(v: Vec2, arc: Arc) -> f64 {
    let mut value = v.dot(Vec2::from_angle(arc.start)).max(v.dot(Vec2::from_angle(arc.start + arc.sweep)));
    if angle_in(v.angle(), arc) {
        value = value.max(v.len());
    }
    value
}
fn arc_projection(arc: Arc, n: Vec2) -> f64 {
    arc.centre.dot(n) + arc.radius * max_dot(n, arc)
}
fn arc_against_arc(output: Arc, source: Arc) -> f64 {
    let delta = output.centre - source.centre;
    let mut candidates = vec![source.start, source.start + source.sweep];
    for theta in [output.start, output.start + output.sweep, delta.angle()] {
        if angle_in(theta, source) {
            candidates.push(theta);
        }
    }
    for phi in [output.start, output.start + output.sweep] {
        let theta = (delta + Vec2::from_angle(phi) * output.radius).angle();
        if angle_in(theta, source) {
            candidates.push(theta);
        }
    }
    candidates
        .into_iter()
        .map(|theta| {
            let n = Vec2::from_angle(theta);
            delta.dot(n) + output.radius * max_dot(n, output)
        })
        .fold(f64::NEG_INFINITY, f64::max)
}

impl Sketch {
    fn convex_boundary(&self, sources: &[(String, bool)]) -> Result<Option<Boundary>> {
        if !(3..=MAX_ROLES).contains(&sources.len()) {
            return Ok(None);
        }
        let mut seen = BTreeSet::new();
        let mut source = Vec::with_capacity(sources.len());
        for (id, forward) in sources {
            if !seen.insert(id) {
                return Err(invalid("duplicate convex offset source"));
            }
            let curve = self
                .curves
                .get(self.curve_index(id).ok_or_else(|| invalid("offset source vanished"))?)
                .ok_or_else(|| invalid("offset source vanished"))?;
            let get = |i| self.point(i).ok_or_else(|| invalid("offset source point vanished"));
            let piece = match curve.kind {
                CurveKind::Line { a, b } => {
                    let (a, b) = (get(a)?, get(b)?);
                    if *forward { Source { a, b, arc: None } } else { Source { a: b, b: a, arc: None } }
                }
                CurveKind::Arc { c, a, b } => {
                    let (centre, a, b) = (get(c)?, get(a)?, get(b)?);
                    let radius = centre.dist(a);
                    let start = (a - centre).angle();
                    let sweep = ((b - centre).angle() - start).rem_euclid(TAU);
                    if !radius.is_finite() || radius <= 1e-9 || sweep <= 1e-9 || (centre.dist(b) - radius).abs() > 1e-6 {
                        return Ok(None);
                    }
                    let arc = Some(Arc { centre, radius, start, sweep, centre_index: c });
                    if *forward { Source { a, b, arc } } else { Source { a: b, b: a, arc } }
                }
                _ => return Ok(None),
            };
            if !piece.a.is_finite() || !piece.b.is_finite() || piece.a.dist(piece.b) <= 1e-9 {
                return Ok(None);
            }
            source.push(piece);
        }
        let origin = source.first().ok_or_else(|| invalid("empty offset boundary"))?.a;
        let scale = source.iter().flat_map(|p| [p.a, p.b]).map(|p| p.dist(origin)).fold(1.0, f64::max);
        let pad = 1e-7 + 256.0 * f64::EPSILON * scale;
        if !pad.is_finite() || pad >= 0.002 {
            return Err(invalid("convex offset coordinate precision exceeds certificate"));
        }
        let mut area = 0.0;
        for (i, piece) in source.iter().enumerate() {
            if piece.b.dist(source.get((i + 1) % source.len()).ok_or_else(|| invalid("missing offset neighbour"))?.a) > pad {
                return Ok(None);
            }
            area += match piece.arc {
                Some(arc) => {
                    let forward = sources.get(i).is_some_and(|x| x.1);
                    Seg2::Arc {
                        center: arc.centre - origin,
                        radius: arc.radius,
                        start: if forward { arc.start } else { arc.start + arc.sweep },
                        sweep: if forward { arc.sweep } else { -arc.sweep },
                    }
                    .area_term()
                }
                None => (piece.a - origin).cross(piece.b - origin) * 0.5,
            };
        }
        if !area.is_finite() || area.abs() <= pad * scale {
            return Ok(None);
        }
        let orientation = area.signum();
        let mut turn = 0.0;
        let mut lines = 0;
        for (i, piece) in source.iter().enumerate() {
            if let Some(arc) = piece.arc {
                if sources.get(i).is_none_or(|x| if orientation > 0.0 { !x.1 } else { x.1 }) {
                    return Ok(None);
                }
                turn += arc.sweep;
            } else {
                lines += 1;
            }
            let end = piece.tangent(true, orientation).ok_or_else(|| invalid("singular offset tangent"))?;
            let next =
                source.get((i + 1) % source.len()).and_then(|p| p.tangent(false, orientation)).ok_or_else(|| invalid("singular offset tangent"))?;
            if end.dot(next) < 1.0 - 1e-8 || end.cross(next).abs() > 1e-6 {
                return Ok(None);
            }
        }
        // A regular closed G1 boundary with monotone curvature and total turning
        // exactly 2pi is a simple convex boundary. Reject loops and sharp sources.
        if lines < 3 || (turn - TAU).abs() > 1e-6 {
            return Ok(None);
        }
        Ok(Some(Boundary { source, orientation, pad }))
    }
    pub fn convex_offset_supported(&self, sources: &[(String, bool)]) -> Result<bool> {
        Ok(self.convex_boundary(sources)?.is_some())
    }
}
impl Boundary {
    fn build(&self, distance: f64) -> Result<Vec<Span>> {
        if !distance.is_finite() || distance.abs() <= 1e-9 || distance.abs() >= 1e9 {
            return Err(invalid("invalid convex offset distance"));
        }
        let inset = distance * self.orientation;
        let mut spans = Vec::with_capacity(self.source.len());
        for piece in &self.source {
            let span = match piece.arc {
                Some(mut arc) => {
                    arc.radius -= inset;
                    let retired = arc.radius <= 1e-9;
                    if retired {
                        Span { a: piece.a, b: piece.b, arc: Some(arc), retired: true }
                    } else {
                        let a = arc.centre + (piece.a - arc.centre).normalized().ok_or_else(|| invalid("singular offset arc"))? * arc.radius;
                        let b = arc.centre + (piece.b - arc.centre).normalized().ok_or_else(|| invalid("singular offset arc"))? * arc.radius;
                        Span { a, b, arc: Some(arc), retired: false }
                    }
                }
                None => {
                    let shift = (piece.b - piece.a).normalized().ok_or_else(|| invalid("singular offset line"))?.perp() * distance;
                    Span { a: piece.a + shift, b: piece.b + shift, arc: None, retired: false }
                }
            };
            spans.push(span);
        }
        let active: Vec<_> = spans.iter().enumerate().filter_map(|(i, s)| (!s.retired).then_some(i)).collect();
        if active.len() < 3 {
            return Err(invalid("the convex offset region disappears"));
        }
        for (position, i) in active.iter().copied().enumerate() {
            let j = *active.get((position + 1) % active.len()).ok_or_else(|| invalid("missing active offset neighbour"))?;
            let a = *spans.get(i).ok_or_else(|| invalid("missing offset span"))?;
            let b = *spans.get(j).ok_or_else(|| invalid("missing offset span"))?;
            let next = (i + 1) % spans.len();
            let joint = if next == j {
                if a.b.dist(b.a) > self.pad {
                    return Err(invalid("offset tangent junction no longer meets"));
                }
                a.b
            } else {
                let retired = *spans.get(next).ok_or_else(|| invalid("missing retired offset span"))?;
                if (next + 1) % spans.len() != j
                    || !retired.retired
                    || a.arc.is_some()
                    || b.arc.is_some()
                    || retired.arc.is_none_or(|x| x.sweep >= std::f64::consts::PI - 1e-8)
                {
                    return Err(invalid("offset arc extinction requires two adjacent straight supports"));
                }
                let (u, v) = (a.b - a.a, b.b - b.a);
                let cross = u.cross(v);
                if cross.abs() <= 1e-10 * u.len() * v.len() {
                    return Err(invalid("ambiguous offset extinction junction"));
                }
                let q = a.a + u * ((b.a - a.a).cross(v) / cross);
                if !q.is_finite() || q.x.abs() >= 1e9 || q.y.abs() >= 1e9 {
                    return Err(invalid("unbounded offset extinction junction"));
                }
                if let Some(s) = spans.get_mut(next) {
                    s.a = q;
                    s.b = q;
                }
                q
            };
            if let Some(s) = spans.get_mut(i) {
                s.b = joint;
            }
            if let Some(s) = spans.get_mut(j) {
                s.a = joint;
            }
        }
        let mut work = 0usize;
        for (index, span) in spans.iter().enumerate().filter(|(_, s)| !s.retired) {
            if span.a.dist(span.b) <= self.pad {
                return Err(invalid("the convex offset region disappears"));
            }
            if span.arc.is_none() {
                let source = self.source.get(index).ok_or_else(|| invalid("missing source span"))?;
                if (span.b - span.a).dot(source.b - source.a) <= self.pad * source.a.dist(source.b) {
                    return Err(invalid("offset line is overrun by its neighbours"));
                }
            }
            for source in &self.source {
                work += 1;
                if work > MAX_WORK {
                    return Err(invalid("convex offset certificate work limit"));
                }
                let (maximum, limit) = match source.arc {
                    Some(arc) => {
                        let maximum = match span.arc {
                            Some(output) => arc_against_arc(output, arc),
                            None => max_dot(span.a - arc.centre, arc).max(max_dot(span.b - arc.centre, arc)),
                        };
                        (maximum, arc.radius - inset)
                    }
                    None => {
                        let normal = (source.b - source.a).normalized().ok_or_else(|| invalid("singular source support"))?.perp() * -self.orientation;
                        let maximum = match span.arc {
                            Some(mut arc) => {
                                arc.centre -= source.a;
                                arc_projection(arc, normal)
                            }
                            None => (span.a - source.a).dot(normal).max((span.b - source.a).dot(normal)),
                        };
                        (maximum, -inset)
                    }
                };
                if !maximum.is_finite() || maximum > limit + self.pad {
                    return Err(invalid("convex offset fails whole-boundary normal support certificate"));
                }
            }
        }
        Ok(spans)
    }
}

impl Sketch {
    /// Validate saved role ownership without evaluating source geometry/parameters.
    pub fn validate_offset_topologies(&self) -> Result<()> {
        if self.offsets.iter().all(|r| r.topology.is_none()) {
            return Ok(());
        }
        let mut groups = BTreeSet::new();
        let mut outputs = BTreeSet::new();
        let mut total = 0usize;
        let point_index: BTreeMap<_, _> = self.points.iter().enumerate().map(|(i, p)| (p.id.as_str(), i)).collect();
        let curve_index: BTreeMap<_, _> = self.curves.iter().enumerate().map(|(i, p)| (p.id.as_str(), i)).collect();
        let mut owners: BTreeMap<usize, BTreeSet<&str>> = BTreeMap::new();
        for curve in &self.curves {
            for p in curve.kind.point_ids() {
                owners.entry(p).or_default().insert(curve.id.as_str());
            }
        }
        let other_ids: BTreeSet<&str> = self
            .points
            .iter()
            .map(|x| x.id.as_str())
            .chain(self.constraints.iter().map(|x| x.id.as_str()))
            .chain(self.links.iter().map(|x| x.id.as_str()))
            .chain(self.wires.iter().map(|x| x.id.as_str()))
            .collect();
        let id_ok = |id: &str| !id.is_empty() && id.len() <= 512;
        for record in &self.offsets {
            let Some(topology) = &record.topology else { continue };
            total = total.checked_add(topology.roles.len()).ok_or(SketchError::TooLarge)?;
            if topology.version != 1
                || !record.closed
                || !id_ok(&topology.id)
                || !(3..=MAX_ROLES).contains(&topology.roles.len())
                || total > MAX_TOTAL_ROLES
                || topology.roles.len() != record.sources.len()
                || !record.distance.is_finite()
                || record.distance.abs() <= 1e-9
                || record.distance.abs() >= 1e9
                || !groups.insert(topology.id.as_str())
                || groups.len() > MAX_GROUPS
                || curve_index.contains_key(topology.id.as_str())
                || other_ids.contains(topology.id.as_str())
            {
                return Err(invalid("damaged convex offset topology"));
            }
            let active: Vec<_> = topology.roles.iter().filter(|r| !r.retired).map(|r| r.output.as_str()).collect();
            if active.len() < 3 || !record.outputs.iter().map(String::as_str).eq(active) {
                return Err(invalid("damaged offset active role order"));
            }
            let record_outputs: BTreeSet<_> = topology.roles.iter().map(|r| r.output.as_str()).collect();
            let mut sources = BTreeSet::new();
            for ((source, forward), role) in record.sources.iter().zip(&topology.roles) {
                if !id_ok(source)
                    || !sources.insert(source.as_str())
                    || !id_ok(&role.output)
                    || !id_ok(&role.start)
                    || !id_ok(&role.end)
                    || role.start == role.end
                    || role.output == *source
                    || !outputs.insert(role.output.as_str())
                    || other_ids.contains(role.output.as_str())
                {
                    return Err(invalid("damaged offset reserved identity"));
                }
                let source_curve = self
                    .curves
                    .get(*curve_index.get(source.as_str()).ok_or_else(|| invalid("missing offset source role"))?)
                    .ok_or_else(|| invalid("missing offset source role"))?;
                let centre = match (&source_curve.kind, role.kind) {
                    (CurveKind::Line { .. }, OffsetRoleKind::Line) => None,
                    (CurveKind::Arc { c, .. }, OffsetRoleKind::Arc) => Some(*c),
                    _ => return Err(invalid("offset source changed role kind")),
                };
                if role.retired && role.kind != OffsetRoleKind::Arc {
                    return Err(invalid("only offset arcs may retire"));
                }
                let a = *point_index.get(role.start.as_str()).ok_or_else(|| invalid("missing offset endpoint role"))?;
                let b = *point_index.get(role.end.as_str()).ok_or_else(|| invalid("missing offset endpoint role"))?;
                for p in [a, b] {
                    if p == 0
                        || self.points.get(p).is_none_or(|p| !p.fixed || p.link.is_some())
                        || owners.get(&p).is_none_or(|users| users.is_empty() || users.iter().any(|id| !record_outputs.contains(id)))
                    {
                        return Err(invalid("offset endpoints must be fixed private group roles"));
                    }
                }
                if role.retired {
                    if curve_index.contains_key(role.output.as_str()) {
                        return Err(invalid("retired offset curve is still active"));
                    }
                } else {
                    let output = self
                        .curves
                        .get(*curve_index.get(role.output.as_str()).ok_or_else(|| invalid("missing active offset curve"))?)
                        .ok_or_else(|| invalid("missing active offset curve"))?;
                    let valid = match output.kind {
                        CurveKind::Line { a: pa, b: pb } => role.kind == OffsetRoleKind::Line && pa == a && pb == b,
                        CurveKind::Arc { c, a: pa, b: pb } => {
                            role.kind == OffsetRoleKind::Arc && centre == Some(c) && if *forward { pa == a && pb == b } else { pa == b && pb == a }
                        }
                        _ => false,
                    };
                    if !valid || output.link.is_some() {
                        return Err(invalid("active offset curve has incompatible point roles"));
                    }
                }
            }
            if sources.iter().any(|id| record_outputs.contains(id)) {
                return Err(invalid("offset source/output alias"));
            }
        }
        if groups.iter().any(|id| outputs.contains(id)) {
            return Err(invalid("offset group/output identity alias"));
        }
        // Index associations once, rather than copying every source for every
        // output. Charge all legacy records too, before allocating the graph.
        let mut budget = self.offsets.len().saturating_add(self.freeform_offsets.len());
        for r in &self.offsets {
            budget = budget.saturating_add(r.sources.len()).saturating_add(r.topology.as_ref().map_or(r.outputs.len(), |t| t.roles.len()));
        }
        budget = budget.saturating_add(self.freeform_offsets.len().saturating_mul(2));
        if budget > MAX_WORK {
            return Err(invalid("offset dependency work limit"));
        }
        let count = self.offsets.len() + self.freeform_offsets.len();
        let mut producer = BTreeMap::new();
        for (index, record) in self.offsets.iter().enumerate() {
            let ids: Box<dyn Iterator<Item = &str> + '_> = match &record.topology {
                Some(t) => Box::new(t.roles.iter().map(|r| r.output.as_str())),
                None => Box::new(record.outputs.iter().map(String::as_str)),
            };
            for id in ids {
                if producer.insert(id, index).is_some() {
                    return Err(invalid("offset output has multiple producers"));
                }
            }
        }
        for (index, record) in self.freeform_offsets.iter().enumerate() {
            if producer.insert(record.output.as_str(), self.offsets.len() + index).is_some() {
                return Err(invalid("offset output has multiple producers"));
            }
        }
        let mut edges = vec![BTreeSet::new(); count];
        for (index, record) in self.offsets.iter().enumerate() {
            for (source, _) in &record.sources {
                if let Some(parent) = producer.get(source.as_str()) {
                    edges.get_mut(index).ok_or_else(|| invalid("invalid dependency node"))?.insert(*parent);
                }
            }
        }
        for (index, record) in self.freeform_offsets.iter().enumerate() {
            if let Some(parent) = producer.get(record.source.as_str()) {
                edges.get_mut(self.offsets.len() + index).ok_or_else(|| invalid("invalid dependency node"))?.insert(*parent);
            }
        }
        let mut colour = vec![0u8; count];
        for start in 0..count {
            if colour.get(start).is_none_or(|c| *c != 0) {
                continue;
            }
            let mut stack = vec![(start, false)];
            while let Some((node, finish)) = stack.pop() {
                if finish {
                    *colour.get_mut(node).ok_or_else(|| invalid("invalid dependency node"))? = 2;
                    continue;
                }
                let state = *colour.get(node).ok_or_else(|| invalid("invalid dependency node"))?;
                if state == 2 {
                    continue;
                }
                if state == 1 {
                    return Err(invalid("cyclic offset association"));
                }
                *colour.get_mut(node).ok_or_else(|| invalid("invalid dependency node"))? = 1;
                stack.push((node, true));
                for &parent in edges.get(node).ok_or_else(|| invalid("invalid dependency node"))? {
                    stack.push((parent, false));
                }
            }
        }
        Ok(())
    }
    pub fn offset_boundary(&self, id: &str) -> Result<Vec<String>> {
        let mut matching = self.offsets.iter().filter(|r| r.topology.as_ref().is_some_and(|t| t.id == id));
        let record = matching.next().ok_or_else(|| invalid("unknown offset boundary association"))?;
        if matching.next().is_some() {
            return Err(invalid("ambiguous offset boundary association"));
        }
        Ok(record.outputs.clone())
    }
    fn match_offset_boundary(&self, ids: &[String], reserved: bool) -> Result<Option<String>> {
        let requested: BTreeSet<_> = ids.iter().map(String::as_str).collect();
        if ids.is_empty() || requested.len() != ids.len() {
            return Err(invalid("duplicate or empty offset boundary selection"));
        }
        let mut found = None;
        for record in &self.offsets {
            let Some(topology) = &record.topology else { continue };
            let expected: BTreeSet<_> = if reserved {
                topology.roles.iter().map(|r| r.output.as_str()).collect()
            } else {
                record.outputs.iter().map(String::as_str).collect()
            };
            if expected == requested {
                if found.is_some() {
                    return Err(invalid("ambiguous offset boundary selection"));
                }
                found = Some(topology.id.clone());
            }
        }
        Ok(found)
    }
    pub fn offset_boundary_for_curves(&self, ids: &[String]) -> Result<Option<String>> {
        self.match_offset_boundary(ids, false)
    }
    pub fn offset_boundary_for_reserved_curves(&self, ids: &[String]) -> Result<Option<String>> {
        self.match_offset_boundary(ids, true)
    }

    pub fn add_convex_offset(&mut self, sources: Vec<(String, bool)>, distance: f64) -> Result<OffsetCreated> {
        let mut candidate = self.clone();
        let boundary = candidate.convex_boundary(&sources)?.ok_or_else(|| invalid("offset source is not a closed convex G1 analytic boundary"))?;
        let spans = boundary.build(distance)?;
        if candidate.offsets.iter().filter(|r| r.topology.is_some()).count() >= MAX_GROUPS {
            return Err(SketchError::TooLarge);
        }
        let association = candidate.fresh("o")?;
        let mut nodes = Vec::new();
        // One node per original directed joint. Retired arcs keep both endpoint
        // roles, each still used by an adjacent surviving straight output.
        for span in &spans {
            let point = candidate.add_point(span.a, None)?;
            if let Some(p) = candidate.points.get_mut(point) {
                p.fixed = true;
            }
            nodes.push(point);
        }
        let mut roles = Vec::new();
        let mut outputs = Vec::new();
        for (i, span) in spans.iter().enumerate() {
            let a = *nodes.get(i).ok_or_else(|| invalid("missing output node"))?;
            let b = *nodes.get((i + 1) % nodes.len()).ok_or_else(|| invalid("missing output node"))?;
            let kind = if span.arc.is_some() { OffsetRoleKind::Arc } else { OffsetRoleKind::Line };
            let output = candidate.fresh(if kind == OffsetRoleKind::Arc { "a" } else { "l" })?;
            let role = OffsetRole {
                output: output.clone(),
                kind,
                start: candidate.points.get(a).ok_or_else(|| invalid("missing output node"))?.id.clone(),
                end: candidate.points.get(b).ok_or_else(|| invalid("missing output node"))?.id.clone(),
                retired: span.retired,
                flags: OffsetRoleFlags::default(),
            };
            if !span.retired {
                let curve_kind = match span.arc {
                    Some(arc) => {
                        if sources.get(i).is_some_and(|x| x.1) {
                            CurveKind::Arc { c: arc.centre_index, a, b }
                        } else {
                            CurveKind::Arc { c: arc.centre_index, a: b, b: a }
                        }
                    }
                    None => CurveKind::Line { a, b },
                };
                candidate.curves.push(Curve {
                    id: output.clone(),
                    kind: curve_kind,
                    construction: false,
                    reversed: false,
                    link: None,
                    centerline: false,
                    fixed: false,
                    infinite: false,
                    spline_handle_of: None,
                });
                outputs.push(output);
            }
            roles.push(role);
        }
        candidate.offsets.push(OffsetChain {
            sources,
            outputs: outputs.clone(),
            distance,
            closed: true,
            topology: Some(OffsetTopology { version: 1, id: association.clone(), roles }),
        });
        candidate.validate_offset_topologies()?;
        *self = candidate;
        Ok(OffsetCreated { association, curves: outputs })
    }

    pub(crate) fn refresh_convex_offset(&mut self, index: usize) -> Result<bool> {
        let record = self.offsets.get(index).cloned().ok_or_else(|| invalid("missing offset association"))?;
        if !record.closed {
            return Ok(false);
        }
        let boundary = self.convex_boundary(&record.sources)?;
        let Some(boundary) = boundary else {
            if record.topology.is_some() {
                return Err(invalid("topology-aware offset source is no longer convex G1"));
            }
            return Ok(false);
        };
        let spans = boundary.build(record.distance)?;
        let mut topology = match record.topology.clone() {
            Some(t) => t,
            None => {
                if record.outputs.len() != record.sources.len() {
                    return Err(invalid("damaged legacy offset correspondence"));
                }
                let mut roles = Vec::new();
                for ((output, (_, forward)), source) in record.outputs.iter().zip(&record.sources).zip(&boundary.source) {
                    let curve = self
                        .curves
                        .get(self.curve_index(output).ok_or_else(|| invalid("missing legacy offset output"))?)
                        .ok_or_else(|| invalid("missing legacy offset output"))?;
                    let (a, b, kind) = match curve.kind {
                        CurveKind::Line { a, b } if source.arc.is_none() => (a, b, OffsetRoleKind::Line),
                        CurveKind::Arc { c, a, b } if source.arc.is_some_and(|arc| arc.centre_index == c) => {
                            if *forward {
                                (a, b, OffsetRoleKind::Arc)
                            } else {
                                (b, a, OffsetRoleKind::Arc)
                            }
                        }
                        _ => return Err(invalid("legacy offset output changed role")),
                    };
                    roles.push(OffsetRole {
                        output: output.clone(),
                        kind,
                        start: self.points.get(a).ok_or_else(|| invalid("missing legacy offset node"))?.id.clone(),
                        end: self.points.get(b).ok_or_else(|| invalid("missing legacy offset node"))?.id.clone(),
                        retired: false,
                        flags: OffsetRoleFlags::of(curve),
                    });
                }
                OffsetTopology { version: 1, id: self.fresh("o")?, roles }
            }
        };
        if topology.roles.len() != spans.len() {
            return Err(invalid("damaged convex offset role count"));
        }
        let retiring: BTreeSet<_> =
            topology.roles.iter().zip(&spans).filter_map(|(r, s)| (s.retired && !r.retired).then_some(r.output.clone())).collect();
        let dead: BTreeSet<_> = self.curves.iter().enumerate().filter_map(|(i, c)| retiring.contains(c.id.as_str()).then_some(i)).collect();
        for role in &mut topology.roles {
            if retiring.contains(role.output.as_str()) {
                let curve = self
                    .curves
                    .get(self.curve_index(&role.output).ok_or_else(|| invalid("missing retiring curve"))?)
                    .ok_or_else(|| invalid("missing retiring curve"))?;
                role.flags = OffsetRoleFlags::of(curve);
            }
        }
        for constraint in &self.constraints {
            let mut kind = constraint.kind.clone();
            if !kind.remap(&Some, &|c| (!dead.contains(&c)).then_some(c)) {
                return Err(invalid("an extinguished offset arc is referenced by a constraint"));
            }
        }
        if self.offsets.iter().any(|r| r.sources.iter().any(|s| retiring.contains(s.0.as_str())))
            || self.freeform_offsets.iter().any(|r| retiring.contains(r.source.as_str()))
        {
            return Err(invalid("an extinguished offset arc is retained by a dependent association"));
        }
        let mut map = vec![None; self.curves.len()];
        let mut next = 0usize;
        for (i, slot) in map.iter_mut().enumerate() {
            if !dead.contains(&i) {
                *slot = Some(next);
                next += 1;
            }
        }
        let mut i = 0;
        self.curves.retain(|_| {
            let keep = !dead.contains(&i);
            i += 1;
            keep
        });
        for constraint in &mut self.constraints {
            if !constraint.kind.remap(&Some, &|i| map.get(i).copied().flatten()) {
                return Err(invalid("offset constraint remapping failed"));
            }
        }
        let mut outputs = Vec::new();
        for (i, (role, span)) in topology.roles.iter_mut().zip(&spans).enumerate() {
            let a = self.point_index(&role.start).ok_or_else(|| invalid("missing reserved offset endpoint"))?;
            let b = self.point_index(&role.end).ok_or_else(|| invalid("missing reserved offset endpoint"))?;
            for (point, pos) in [(a, span.a), (b, span.b)] {
                let p = self.points.get_mut(point).ok_or_else(|| invalid("missing reserved offset endpoint"))?;
                p.pos = pos;
                p.fixed = true;
            }
            if !span.retired {
                let kind = match span.arc {
                    Some(arc) => {
                        if record.sources.get(i).is_some_and(|x| x.1) {
                            CurveKind::Arc { c: arc.centre_index, a, b }
                        } else {
                            CurveKind::Arc { c: arc.centre_index, a: b, b: a }
                        }
                    }
                    None => CurveKind::Line { a, b },
                };
                if let Some(ci) = self.curve_index(&role.output) {
                    if let Some(curve) = self.curves.get_mut(ci) {
                        curve.kind = kind;
                    }
                } else {
                    if !role.retired {
                        return Err(invalid("active offset output vanished"));
                    }
                    if self.points.iter().any(|p| p.id == role.output)
                        || self.constraints.iter().any(|c| c.id == role.output)
                        || self.links.iter().any(|l| l.id == role.output)
                        || self.wires.iter().any(|w| w.id == role.output)
                    {
                        return Err(invalid("retired offset identity was stolen"));
                    }
                    self.curves.push(Curve {
                        id: role.output.clone(),
                        kind,
                        construction: role.flags.construction,
                        reversed: role.flags.reversed,
                        link: None,
                        centerline: role.flags.centerline,
                        fixed: role.flags.fixed,
                        infinite: role.flags.infinite,
                        spline_handle_of: None,
                    });
                }
                outputs.push(role.output.clone());
            }
            role.retired = span.retired;
        }
        let record = self.offsets.get_mut(index).ok_or_else(|| invalid("missing offset association"))?;
        record.outputs = outputs;
        record.topology = Some(topology);
        self.validate_offset_topologies()?;
        Ok(true)
    }

    /// Caller materializes current driving values before freezing the association.
    pub fn break_offset_topology(&mut self, output: &str) -> Result<Option<Vec<String>>> {
        let Some(index) =
            self.offsets.iter().position(|r| r.topology.as_ref().is_some_and(|t| t.id == output || t.roles.iter().any(|role| role.output == output)))
        else {
            return Ok(None);
        };
        let mut candidate = self.clone();
        candidate.refresh_offsets()?;
        let record = candidate.offsets.get(index).cloned().ok_or_else(|| invalid("missing offset association"))?;
        let outputs: BTreeSet<_> = record.outputs.iter().map(String::as_str).collect();
        let owned: BTreeSet<_> = candidate
            .curves
            .iter()
            .filter(|c| outputs.contains(c.id.as_str()))
            .flat_map(|c| match c.kind {
                CurveKind::Line { a, b } | CurveKind::Arc { a, b, .. } => vec![a, b],
                _ => Vec::new(),
            })
            .collect();
        // Arc centres were deliberately shared with their analytic sources.
        // Detach each distinct centre once, so released geometry stays frozen
        // when the source is subsequently moved or dimensioned.
        let centres: BTreeSet<_> = candidate
            .curves
            .iter()
            .filter_map(|c| {
                if outputs.contains(c.id.as_str())
                    && let CurveKind::Arc { c, .. } = c.kind
                {
                    return Some(c);
                }
                None
            })
            .collect();
        let mut detached = BTreeMap::new();
        for centre in centres {
            let position = candidate.points.get(centre).ok_or_else(|| invalid("missing offset arc centre"))?.pos;
            detached.insert(centre, candidate.add_point(position, None)?);
        }
        for curve in &mut candidate.curves {
            if outputs.contains(curve.id.as_str())
                && let CurveKind::Arc { c, .. } = &mut curve.kind
                && let Some(&point) = detached.get(c)
            {
                *c = point;
            }
        }
        candidate.offsets.remove(index);
        candidate.constraints.retain(|c| !matches!(&c.kind, crate::ConstraintKind::OffsetDistance { output, .. } if record.owns_output(output)));
        for point in owned {
            if point != 0
                && candidate.curves.iter().all(|c| !c.kind.uses(point) || outputs.contains(c.id.as_str()))
                && let Some(p) = candidate.points.get_mut(point)
            {
                p.fixed = false;
            }
        }
        *self = candidate;
        Ok(Some(record.outputs))
    }
}

#[cfg(test)]
#[path = "offset_topology_tests.rs"]
mod tests;
