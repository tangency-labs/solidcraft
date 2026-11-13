//! Directed stations on the accepted native Form Surface routes and cells.
use super::*;
use crate::kernel::{Body, DisplayGuidePoint};
use solidcore_record::brep::{Brep, Curve, Curve2, Edge, Loop, OrientedEdge, Shell, Surface};
use solidcraft_geom::{
    brep::{
        eval::{CurveEval, SurfaceEval},
        pcurve,
    },
    tess::EvalSurface,
};

const MAX_FACES: usize = 256;
const MAX_GUIDES: usize = 544;
const MAX_STATIONS: usize = 20_000;
const MAX_CONTROLS: usize = 16_384;
const MAX_DEGREE: usize = 32;

pub(super) fn frames(definition: &Definition, body: &Body, owners: Option<&[usize]>, guides: &SelectionGuides) -> SelectionFrames {
    let mut out = SelectionFrames::default();
    let SelectionGuides::Native(native) = guides else { return out };
    if definition.profile.is_none() || definition.placement.validate().is_err() || native.edges.len() > MAX_GUIDES {
        return out;
    }
    let mut remaining = WORK;
    let Some(mut eval) = Eval::new(native.body.record(), &mut remaining) else { return out };
    let local = |axes: [Vec3; 3]| axes.map(|axis| definition.placement.local_vector(axis));
    let mut routes = BTreeMap::new();
    for route in &native.edges {
        if route.edge[0] >= route.edge[1] || routes.insert(route.edge, route).is_some() {
            return out;
        }
    }
    let mut points = BTreeMap::<_, PointFrame>::new();
    for (&edge, guide) in &routes {
        let segments = segments(native.body.record(), guide, &mut remaining);
        if let Some(frame) = segments.as_deref().and_then(|segments| eval.mid_axes(segments, &mut remaining)) {
            out.edges.insert(edge, local(frame));
        }
        for (end, id) in edge.into_iter().enumerate() {
            let corner = points.entry(id).or_default();
            let samples = segments.as_deref().and_then(|segments| {
                let segment = if end == 0 { segments.first()? } else { segments.last()? };
                eval.segment(*segment, end as f64, &mut remaining)
            });
            if corner.add(samples.as_deref(), end == 1).is_none() {
                corner.incomplete = true;
            }
        }
    }
    for (id, corner) in points {
        if agrees(&corner.normals)
            && let Some(frame) = corner.finish()
        {
            out.points.insert(id, local(frame));
        }
    }
    // Legacy display-only cells may give exact guides on a different body.
    // They cannot confer selectable cell ownership on the unpartitioned skin.
    if std::ptr::eq(body.record(), native.body.record())
        && let Some(owners) = owners.filter(|owners| owners.len() == body.face_count())
    {
        let mut cells = BTreeMap::new();
        for (face, &owner) in owners.iter().enumerate() {
            cells.entry(owner).and_modify(|id| *id = None).or_insert(Some(face));
        }
        for (owner, face) in cells {
            if let Some(face) = face
                && let Some(frame) = definition.cage.faces.get(owner).and_then(|cell| eval.cell_axes(face, cell, &routes, &mut remaining))
            {
                out.faces.insert(owner, local(frame));
            }
        }
    }
    out
}

fn agrees(normals: &BTreeMap<usize, Vec3>) -> bool {
    normals.values().next().is_some_and(|first| normals.values().all(|other| first.dot(*other) >= SAME_DIRECTION))
}
fn agreed_axes(samples: &[EdgeSample]) -> Option<[Vec3; 3]> {
    let (x, normals) = edge_samples(samples)?;
    agrees(&normals).then_some(())?;
    axes(x, bisector(&normals)?)
}
#[derive(Clone, Copy)]
struct Segment {
    face: usize,
    a: DisplayGuidePoint,
    b: DisplayGuidePoint,
}
fn segments(body: &Brep, guide: &NativeGuide, remaining: &mut usize) -> Option<Vec<Segment>> {
    if guide.pieces.is_empty() || guide.pieces.len() > MAX_STATIONS {
        return None;
    }
    let mut out = Vec::new();
    let mut last = None;
    for route in &guide.pieces {
        if route.points.len() < 2 || route.points.len() > MAX_STATIONS || out.len().checked_add(route.points.len() - 1)? > MAX_STATIONS {
            return None;
        }
        spend(remaining, route.points.len())?;
        if let Some(previous) = last {
            let next = *route.points.first()?;
            if previous != next && !vertex(body, previous).zip(vertex(body, next)).is_some_and(|(a, b)| a == b) {
                return None;
            }
        }
        for pair in route.points.windows(2) {
            let [a, b] = pair else { return None };
            if a != b {
                out.push(Segment { face: route.face as usize, a: *a, b: *b });
            }
        }
        last = route.points.last().copied();
    }
    (!out.is_empty()).then_some(out)
}

fn vertex(body: &Brep, station: DisplayGuidePoint) -> Option<u32> {
    let DisplayGuidePoint::Edge { edge, parameter } = station else { return None };
    let edge = body.edges.get(edge as usize)?;
    if parameter == edge.interval[0] {
        Some(edge.vertices[0])
    } else if parameter == edge.interval[1] {
        Some(edge.vertices[1])
    } else {
        None
    }
}

struct Eval<'a> {
    body: &'a Brep,
    sense: BTreeMap<usize, Option<bool>>,
    surfaces: BTreeMap<usize, SurfaceEval>,
}
impl<'a> Eval<'a> {
    fn new(body: &'a Brep, remaining: &mut usize) -> Option<Self> {
        if body.faces.len() > MAX_FACES || body.edges.len() > MAX_GUIDES * 4 || body.shells.len() > MAX_FACES {
            return None;
        }
        let mut sense = BTreeMap::new();
        for shell in &body.shells {
            if let Shell::Faces { faces, .. } = shell {
                spend(remaining, faces.len())?;
                for face in faces {
                    sense.entry(face.face as usize).and_modify(|sense| *sense = None).or_insert(Some(face.forward));
                }
            }
        }
        Some(Self { body, sense, surfaces: BTreeMap::new() })
    }

    fn surface_jet(&mut self, face: usize, uv: [f64; 2], remaining: &mut usize) -> Option<(Vec3, Vec3, Vec3, Vec3)> {
        let forward = *self.sense.get(&face)?.as_ref()?;
        let record = self.body.faces.get(face)?;
        let uv = [bounded(uv[0], record.domain[0])?, bounded(uv[1], record.domain[1])?];
        let surface = match self.surfaces.entry(face) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                let source = record.parameters();
                match source {
                    Surface::Plane { .. } | Surface::PlaneThrough { .. } => spend(remaining, 12)?,
                    Surface::Spline { u_knots, v_knots, points } => tensor_budget(u_knots, v_knots, points, remaining)?,
                    Surface::RationalSpline { u_knots, v_knots, homogeneous } => tensor_budget(u_knots, v_knots, homogeneous, remaining)?,
                    // These are exactly the record parameter families used by
                    // native Profile guide routes. No display UV remapping.
                    _ => return None,
                }
                entry.insert(SurfaceEval::new(source).ok()?)
            }
        };
        let uv = match surface {
            SurfaceEval::Eval(EvalSurface::Plane { .. }, _) => {
                spend(remaining, 12)?;
                uv
            }
            SurfaceEval::Eval(EvalSurface::Nurbs { degree, knots, .. }, domain) => {
                let [(u0, u1), (v0, v1)] = [domain.range[0]?, domain.range[1]?];
                let uv = [bounded(uv[0], [u0, u1])?, bounded(uv[1], [v0, v1])?];
                if !(0..2).all(|d| regular_knot(&knots[d], degree[d], uv[d])) {
                    return None;
                }
                spend(remaining, 4 * (degree[0] + 1) * (degree[1] + 1))?;
                uv
            }
            _ => return None,
        };
        let (point, du, dv) = surface.derivs(uv[0], uv[1])?;
        let n = normal(du, dv)?;
        point.is_finite().then_some((point, du, dv, if forward { n } else { -n }))
    }

    fn edge_use(&self, face: usize, id: u32) -> Option<(&'a Edge, &'a OrientedEdge)> {
        let edge = self.body.edges.get(id as usize)?;
        if edge.uses.len() > 2 {
            return None;
        }
        let mut uses = edge.uses.iter().filter(|use_| use_.face as usize == face);
        let use_ = uses.next()?;
        if uses.next().is_some() {
            return None;
        }
        let Loop::Edges { edges } = self.body.faces.get(face)?.loops.get(use_.r#loop as usize)? else { return None };
        let use_ = edges.get(use_.position as usize)?;
        (use_.edge == id).then_some((edge, use_))
    }

    fn station_uv(&self, face: usize, point: DisplayGuidePoint, linear: bool, remaining: &mut usize) -> Option<[f64; 2]> {
        match point {
            DisplayGuidePoint::Surface(uv) if !linear => uv.iter().all(|v| v.is_finite()).then_some(uv),
            DisplayGuidePoint::Surface(_) => None,
            DisplayGuidePoint::Edge { edge, parameter } => {
                let (edge, use_) = self.edge_use(face, edge)?;
                let pcurve = &use_.pcurve;
                let (knots, count) = match pcurve {
                    Curve2::Spline { knots, points } => (knots, points.len()),
                    Curve2::RationalSpline { knots, homogeneous } => {
                        if linear && !homogeneous.iter().all(|p| p[2].is_finite() && p[2] > 0.0) {
                            return None;
                        }
                        (knots, homogeneous.len())
                    }
                };
                let degree = curve_budget(knots, count, remaining)?;
                if linear && (degree != 1 || count != 2) {
                    return None;
                }
                let parameter = bounded(bounded(parameter, edge.interval)?, pcurve.interval()?)?;
                pcurve::eval(pcurve, parameter).filter(|uv| uv.iter().all(|v| v.is_finite()))
            }
        }
    }

    fn intrinsic(&mut self, face: usize, id: u32, parameter: f64, direction: f64, remaining: &mut usize) -> Option<Vec<EdgeSample>> {
        let (edge, _) = self.edge_use(face, id)?;
        let parameter = bounded(parameter, edge.interval)?;
        let curve = edge.parameters();
        match curve {
            Curve::Line { .. } | Curve::Circle { .. } | Curve::Ellipse { .. } => spend(remaining, 12)?,
            Curve::Spline { knots, points } => {
                let degree = curve_budget(knots, points.len(), remaining)?;
                regular_knot(knots, degree, parameter).then_some(())?;
            }
            Curve::RationalSpline { knots, homogeneous } => {
                let degree = curve_budget(knots, homogeneous.len(), remaining)?;
                regular_knot(knots, degree, parameter).then_some(())?;
            }
            _ => return None,
        }
        let (point, derivative) = CurveEval::new(curve).ok()?.derivs(parameter)?;
        let tangent = unit(derivative)? * direction;
        let tolerance = edge.tolerance.max(1e-7);
        if !point.is_finite() || !edge.tolerance.is_finite() || !(0.0..=1.0).contains(&edge.tolerance) {
            return None;
        }
        let position = DisplayGuidePoint::Edge { edge: id, parameter };
        let station = if let Some(vertex) = vertex(self.body, position) {
            let [x, y, z] = self.body.vertices.get(vertex as usize)?.point;
            Vec3::new(x, y, z)
        } else {
            point
        };
        if !station.is_finite() || station.dist(point) > tolerance {
            return None;
        }
        let mut samples = Vec::new();
        for use_ in &edge.uses {
            let chart = use_.face as usize;
            let uv = self.station_uv(chart, DisplayGuidePoint::Edge { edge: id, parameter }, false, remaining)?;
            let (on, _, _, normal) = self.surface_jet(chart, uv, remaining)?;
            if on.dist(station) > tolerance || tangent.dot(normal).abs() > 1e-4 {
                return None;
            }
            samples.push(EdgeSample { chart, point: station, tangent, normal });
        }
        Some(samples)
    }

    fn segment(&mut self, segment: Segment, t: f64, remaining: &mut usize) -> Option<Vec<EdgeSample>> {
        spend(remaining, 1)?;
        if let (DisplayGuidePoint::Edge { edge, parameter: a }, DisplayGuidePoint::Edge { edge: other, parameter: b }) = (segment.a, segment.b)
            && edge == other
        {
            return self.intrinsic(segment.face, edge, a * (1.0 - t) + b * t, (b - a).signum(), remaining);
        }
        let a = self.station_uv(segment.face, segment.a, false, remaining)?;
        let b = self.station_uv(segment.face, segment.b, false, remaining)?;
        let uv = std::array::from_fn(|d| a[d] * (1.0 - t) + b[d] * t);
        let (point, du, dv, normal) = self.surface_jet(segment.face, uv, remaining)?;
        let tangent = unit(du * (b[0] - a[0]) + dv * (b[1] - a[1]))?;
        let end = if t == 0.0 {
            Some(segment.a)
        } else if t == 1.0 {
            Some(segment.b)
        } else {
            None
        };
        if let Some(DisplayGuidePoint::Edge { edge, parameter }) = end {
            // At a real native seam also qualify every incident chart. Keep
            // the guide's directional derivative, not the trim tangent.
            let mut samples = self.intrinsic(segment.face, edge, parameter, 1.0, remaining)?;
            for sample in &mut samples {
                if tangent.dot(sample.normal).abs() > 1e-4 {
                    return None;
                }
                sample.tangent = tangent;
            }
            return Some(samples);
        }
        Some(vec![EdgeSample { chart: segment.face, point, tangent, normal }])
    }

    fn mid_axes(&mut self, segments: &[Segment], remaining: &mut usize) -> Option<[Vec3; 3]> {
        // Native routes retain ordered parameter restrictions, not arclength.
        // At the central join use both exact one-sided differentials.
        let mid = segments.len() / 2;
        let segment = *segments.get(mid)?;
        let samples = if segments.len().is_multiple_of(2) {
            let mut samples = self.segment(*segments.get(mid.checked_sub(1)?)?, 1.0, remaining)?;
            samples.extend(self.segment(segment, 0.0, remaining)?);
            samples
        } else {
            self.segment(segment, 0.5, remaining)?
        };
        agreed_axes(&samples)
    }

    fn cell_axes(&mut self, face: usize, cell: &[usize], guides: &BTreeMap<[usize; 2], &NativeGuide>, remaining: &mut usize) -> Option<[Vec3; 3]> {
        let &[a, b, c, d] = cell else { return None };
        let [Loop::Edges { edges }] = self.body.faces.get(face)?.loops.as_slice() else { return None };
        if edges.len() != 4 {
            return None;
        }
        let mut stations = Vec::with_capacity(4);
        let mut used = std::collections::BTreeSet::new();
        for [start, end] in [[a, b], [b, c], [c, d], [d, a]] {
            let guide = guides.get(&[start.min(end), start.max(end)])?;
            let [piece] = guide.pieces.as_slice() else { return None };
            let [first @ DisplayGuidePoint::Edge { edge, parameter: lo }, last @ DisplayGuidePoint::Edge { edge: other, parameter: hi }] =
                piece.points.as_slice()
            else {
                return None;
            };
            if edge != other || !used.insert(*edge) {
                return None;
            }
            let interval = self.body.edges.get(*edge as usize)?.interval;
            if [*lo, *hi] != interval && [*hi, *lo] != interval {
                return None;
            }
            let (p, q) = if start < end { (*first, *last) } else { (*last, *first) };
            stations.push((self.station_uv(face, p, true, remaining)?, self.station_uv(face, q, true, remaining)?));
        }
        let [(a, ab), (b, bc), (c, cd), (d, da)] = stations.as_slice() else { return None };
        if [*ab, *bc, *cd, *da] != [*b, *c, *d, *a] {
            return None;
        }
        let uv = std::array::from_fn(|i| a[i] * 0.25 + b[i] * 0.25 + c[i] * 0.25 + d[i] * 0.25);
        let turns = [orient(*a, *b, *c)?, orient(*b, *c, *d)?, orient(*c, *d, *a)?, orient(*d, *a, *b)?];
        if !turns.iter().all(|turn| *turn == turns[0])
            || ![(a, b), (b, c), (c, d), (d, a)].into_iter().all(|(p, q)| orient(*p, *q, uv) == Some(turns[0]))
        {
            return None;
        }
        let (_, du, dv, n) = self.surface_jet(face, uv, remaining)?;
        let along: [f64; 2] = std::array::from_fn(|i| (b[i] - a[i]) + (c[i] - d[i]));
        axes(du * along[0] + dv * along[1], n)
    }
}

fn bounded(value: f64, [a, b]: [f64; 2]) -> Option<f64> {
    if !value.is_finite() || !a.is_finite() || !b.is_finite() || a > b {
        return None;
    }
    let clamped = value.clamp(a, b);
    ((value - clamped).abs() <= 64.0 * f64::EPSILON * a.abs().max(b.abs()).max(1.0)).then_some(clamped)
}
fn regular_knot(knots: &[f64], degree: usize, at: f64) -> bool {
    let (Some(a), Some(b)) = (knots.get(degree), knots.get(knots.len().saturating_sub(degree + 1))) else { return false };
    at == *a || at == *b || (*a < at && at < *b && knots.partition_point(|&x| x <= at).saturating_sub(knots.partition_point(|&x| x < at)) < degree)
}
fn curve_budget(knots: &[f64], count: usize, remaining: &mut usize) -> Option<usize> {
    if count > MAX_CONTROLS || knots.len() > MAX_CONTROLS {
        return None;
    }
    let degree = knots.len().checked_sub(count + 1)?;
    if degree == 0 || degree > MAX_DEGREE || degree >= count {
        return None;
    }
    spend(remaining, knots.len() + 3 * count + 3 * (degree + 1) * (degree + 1))?;
    Some(degree)
}
fn tensor_budget<T>(u: &[f64], v: &[f64], rows: &[Vec<T>], remaining: &mut usize) -> Option<()> {
    if rows.len() > MAX_CONTROLS || u.len() > MAX_CONTROLS || v.len() > MAX_CONTROLS {
        return None;
    }
    let columns = rows.first()?.len();
    let count = rows.len().checked_mul(columns)?;
    if count == 0 || count > MAX_CONTROLS || rows.iter().any(|row| row.len() != columns) {
        return None;
    }
    spend(remaining, 8 * count + u.len() + v.len())
}
fn orient(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Option<bool> {
    let x = [b[0] - a[0], b[1] - a[1]];
    let y = [c[0] - a[0], c[1] - a[1]];
    let turn = x[0] * y[1] - x[1] * y[0];
    let scale = a.into_iter().chain(b).chain(c).map(f64::abs).fold(0.0, f64::max);
    let pad = 256.0 * f64::EPSILON * scale * x[0].abs().max(x[1].abs()).max(y[0].abs()).max(y[1].abs());
    (turn.is_finite() && turn.abs() > pad).then_some(turn > 0.0)
}
