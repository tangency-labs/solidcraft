//! Raw, clamped native tensor charts and closed raw pcurve trims.
use super::tensor::{self as ray, Control, Interval as I, MAX_DEGREE, ProofError, RayProof};
use solidcore_ops::ray_spline::{Axis, Grid, MAX_CONTROLS, MAX_SPANS, Span, SplineError, reserved, trim_parity};
use solidcore_record::brep::blend::BlendDefinition;
use solidcore_record::brep::{Brep, Curve2, Face, Loop, Surface, Vertex};

const MAX_TRIM_EDGES: usize = 4_096;

type Result<T> = std::result::Result<T, FaceError>;
type Rect = [[f64; 2]; 2];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceError {
    InvalidFace,
    UnsupportedSurface,
    UnsupportedSpline,
    UnqualifiedTrim,
    GeometryBudget,
    Proof(ProofError),
}

impl From<ProofError> for FaceError {
    fn from(error: ProofError) -> Self {
        Self::Proof(error)
    }
}

impl From<SplineError> for FaceError {
    fn from(error: SplineError) -> Self {
        match error {
            SplineError::InvalidSpline => Self::UnsupportedSpline,
            SplineError::Budget => Self::GeometryBudget,
            SplineError::Proof(error) => Self::Proof(error),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FaceProof {
    Miss,
    /// Raw face (u,v) and t in `origin + t * direction`.
    Unique {
        uv: Rect,
        depth: [f64; 2],
    },
    /// At least two distinct certified material hits in different open spans.
    Multiple,
    /// A possible hit or another root could not be qualified.
    Unresolved,
}

pub struct PreparedFace {
    surface: Sheet,
    trim: Trim,
}

impl PreparedFace {
    /// The record is the accepted edited patch, in the ray's coordinate frame.
    /// Native plane or tensor-side joins may close within recorded tolerance. Hits
    /// inside the resulting uncertainty are withheld from material decisions.
    pub fn prepare(record: &Brep, native_face: usize, work: &mut usize) -> Result<Self> {
        let face = record.faces.get(native_face).ok_or(FaceError::InvalidFace)?;
        if !valid_rect(face.domain) {
            return Err(FaceError::InvalidFace);
        }
        let surface = Sheet::prepare(face, work)?;
        let trim = Trim::prepare(record, face, &surface, work)?;
        Ok(Self { surface, trim })
    }

    pub fn prove(&self, origin: [f64; 3], direction: [f64; 3], ray_parameter: [f64; 2], work: &mut usize) -> Result<FaceProof> {
        let [u, v] = &self.surface.spans;
        let degree = self.surface.degree;
        let mut controls = ray::allocate((degree[0] + 1) * (degree[1] + 1))?;
        let mut result = FaceProof::Miss;
        let mut unresolved = false;
        for ys in v {
            for xs in u {
                ray::spend(work, controls.len())?;
                self.surface.grid.copy_span([xs.start, ys.start], degree.map(|n| n + 1), &mut controls)?;
                let proof = ray::prove_enclosed(degree, &controls, origin, direction, ray_parameter, work)?;
                match proof {
                    RayProof::Miss => {}
                    RayProof::Unresolved => unresolved = true,
                    RayProof::Unique { uv, depth } => {
                        let uv = raw_uv(uv, [xs.range, ys.range]);
                        let t = I { lo: depth[0], hi: depth[1] };
                        let hit = std::array::from_fn(|i| I::point(origin[i]).add(t.scale(direction[i])));
                        match self.material(uv, hit, work)? {
                            Some(true) if matches!(result, FaceProof::Unique { .. }) => return Ok(FaceProof::Multiple),
                            Some(true) => result = FaceProof::Unique { uv: uv.map(I::pair), depth },
                            Some(false) => {}
                            None => unresolved = true,
                        }
                    }
                }
            }
        }
        Ok(if unresolved { FaceProof::Unresolved } else { result })
    }

    fn material(&self, uv: [I; 2], hit: [I; 3], work: &mut usize) -> Result<Option<bool>> {
        ray::spend(work, self.trim.joins.len() * 3)?;
        if uv.iter().chain(&hit).any(|axis| !axis.is_finite())
            || self.trim.joins.iter().any(|join| join.iter().zip(hit).all(|(band, axis)| band.intersect(axis).is_some()))
        {
            return Ok(None);
        }
        let at = uv.map(|i| i.midpoint().clamp(i.lo, i.hi));
        // Try each coordinate direction. The same trim-clear UV box is needed
        // for either proof, even when one probe lies on a trim's vertex ordinate.
        for x in 0..2 {
            let mut parity = false;
            let mut proven = true;
            for piece in &self.trim.pieces {
                let Some(crossing) = trim_parity(piece, uv, at, x, 0, work)? else {
                    proven = false;
                    break;
                };
                parity ^= crossing;
            }
            if proven {
                return Ok(Some(parity));
            }
        }
        Ok(None)
    }
}

fn raw_uv(uv: Rect, domain: Rect) -> [I; 2] {
    std::array::from_fn(|i| {
        let lo = I::point(domain[i][0]);
        let width = I::point(domain[i][1]).sub(lo);
        lo.add(width.mul(I { lo: uv[i][0], hi: uv[i][1] }))
    })
}

fn valid_rect(domain: Rect) -> bool {
    domain.iter().all(|&[a, b]| a.is_finite() && b.is_finite() && a < b)
}

struct Sheet {
    grid: Grid,
    degree: [usize; 2],
    spans: [Vec<Span>; 2],
}

impl Sheet {
    fn prepare(face: &Face, work: &mut usize) -> Result<Self> {
        let domain = face.domain;
        match exact_parameters(face)? {
            Surface::Plane { origin, u_axis, v_axis } => Self::plane(*origin, u_axis.map(I::point), v_axis.map(I::point), domain, work),
            Surface::PlaneThrough { origin, u_point, v_point } => {
                let [du, dv] = through_axes(*origin, *u_point, *v_point);
                Self::plane(*origin, du, dv, domain, work)
            }
            Surface::Spline { u_knots, v_knots, points } => Self::tensor([u_knots, v_knots], points, |[x, y, z]| [x, y, z, 1.0], domain, work),
            Surface::RationalSpline { u_knots, v_knots, homogeneous } => Self::tensor([u_knots, v_knots], homogeneous, |p| p, domain, work),
            _ => Err(FaceError::UnsupportedSurface),
        }
    }

    fn plane(origin: [f64; 3], du: [I; 3], dv: [I; 3], domain: Rect, work: &mut usize) -> Result<Self> {
        let mut controls = ray::allocate(4)?;
        controls.copy_from_slice(&plane_controls(origin, du, dv, domain, work)?);
        Ok(Self { grid: Grid::new([2, 2], controls)?, degree: [1, 1], spans: domain.map(|range| vec![Span { start: 0, range }]) })
    }

    /// Enclose the connector when both pcurve endpoints lie on one retained
    /// chart side and one boundary span. The exact boundary row supplies the
    /// complete lift without guessing a closure across the chart interior.
    fn side_lift(&self, uv: [[I; 2]; 2], work: &mut usize) -> Result<[I; 3]> {
        if uv.iter().flatten().any(|v| !v.is_finite()) {
            return Err(FaceError::UnqualifiedTrim);
        }
        for axis in 0..2 {
            let fixed = uv[0][axis];
            let other = uv[1][axis];
            if fixed.lo != fixed.hi || fixed.lo != other.lo || fixed.hi != other.hi {
                continue;
            }
            let spans = &self.spans[axis];
            let (Some(first), Some(last)) = (spans.first(), spans.last()) else { return Err(FaceError::InvalidFace) };
            let side = if fixed.lo == first.range[0] {
                first.start
            } else if fixed.lo == last.range[1] {
                last.start + self.degree[axis]
            } else {
                continue;
            };
            let varying = 1 - axis;
            let range = uv[0][varying].hull(uv[1][varying]);
            ray::spend(work, self.spans[varying].len())?;
            let Some(span) = self.spans[varying].iter().find(|span| span.range[0] <= range.lo && range.hi <= span.range[1]) else {
                continue;
            };
            let lower = I::point(span.range[0]);
            let unit = range.sub(lower).div(I::point(span.range[1]).sub(lower));
            if !unit.is_finite() {
                return Err(FaceError::UnqualifiedTrim);
            }
            let unit = unit.intersect(I::UNIT).ok_or(FaceError::UnqualifiedTrim)?;
            let degree = self.degree[varying];
            ray::spend(work, degree + 1)?;
            let mut points = [[I::ZERO; 4]; MAX_DEGREE + 1];
            for (i, point) in points.iter_mut().enumerate().take(degree + 1) {
                let mut at = [side; 2];
                at[varying] = span.start + i;
                *point = self.grid.get(at)?;
            }
            let [x, y, z, w] = ray::curve_value(&points[..=degree], unit, work)?;
            if !w.is_finite() || w.lo <= 0.0 {
                return Err(FaceError::UnqualifiedTrim);
            }
            return Ok([x, y, z].map(|p| p.div(w)));
        }
        Err(FaceError::UnqualifiedTrim)
    }

    /// Some native trim endpoints land a few representable steps inward
    /// from a chart corner. Bound the entire connector in its corner span.
    /// The caller still has to certify this lift against the native vertex.
    fn corner_lift(&self, uv: [[I; 2]; 2], work: &mut usize) -> Result<[I; 3]> {
        for (corner, nearby) in [(uv[0], uv[1]), (uv[1], uv[0])] {
            let locate = |axis: usize| {
                let fixed = corner[axis];
                if !fixed.is_finite() || fixed.lo != fixed.hi || !nearby[axis].is_finite() {
                    return None;
                }
                let spans = &self.spans[axis];
                let span = if fixed.lo == spans.first()?.range[0] {
                    spans.first()?
                } else if fixed.lo == spans.last()?.range[1] {
                    spans.last()?
                } else {
                    return None;
                };
                let range = fixed.hull(nearby[axis]);
                let [lo, hi] = span.range;
                if range.lo < lo || hi < range.hi {
                    return None;
                }
                let unit = range.sub(I::point(lo)).div(I::point(hi).sub(I::point(lo))).intersect(I::UNIT)?;
                (unit.hi - unit.lo <= 16.0 * f64::EPSILON).then_some((span.start, unit))
            };
            let Some(((u, s), (v, t))) = locate(0).zip(locate(1)) else { continue };
            let [nu, nv] = self.degree.map(|n| n + 1);
            ray::spend(work, nu * nv)?;
            let mut rows = [[I::ZERO; 4]; MAX_DEGREE + 1];
            for (j, row) in rows[..nv].iter_mut().enumerate() {
                let mut controls = [[I::ZERO; 4]; MAX_DEGREE + 1];
                for (i, point) in controls[..nu].iter_mut().enumerate() {
                    *point = self.grid.get([u + i, v + j])?;
                }
                *row = ray::curve_value(&controls[..nu], s, work)?;
            }
            let [x, y, z, w] = ray::curve_value(&rows[..nv], t, work)?;
            if !w.is_finite() || w.lo <= 0.0 {
                return Err(FaceError::UnqualifiedTrim);
            }
            return Ok([x, y, z].map(|p| p.div(w)));
        }
        Err(FaceError::UnqualifiedTrim)
    }

    fn tensor<T: Copy>(knots: [&[f64]; 2], rows: &[Vec<T>], point: impl Fn(T) -> [f64; 4], domain: Rect, work: &mut usize) -> Result<Self> {
        // Raw record rows are U-major, unlike the evaluator and our U-fast net.
        let size = tensor_size(rows, work)?;
        let mut controls = ray::allocate(size[0] * size[1])?;
        for (u, row) in rows.iter().enumerate() {
            for (v, &p) in row.iter().enumerate() {
                controls[v * size[0] + u] = positive(point(p))?;
            }
        }
        let mut grid = Grid::new(size, controls)?;
        let mut u = Axis::new(knots[0], size[0], work)?;
        let mut v = Axis::new(knots[1], size[1], work)?;
        let spans = [u.spans(&mut grid, 0, domain[0], work)?, v.spans(&mut grid, 1, domain[1], work)?];
        if spans[0].len() * spans[1].len() > MAX_SPANS {
            return Err(FaceError::GeometryBudget);
        }
        Ok(Self { grid, degree: [u.degree(), v.degree()], spans })
    }
}

fn through_axes(origin: [f64; 3], u_point: [f64; 3], v_point: [f64; 3]) -> [[I; 3]; 2] {
    [u_point, v_point].map(|p| std::array::from_fn(|i| I::point(p[i]).sub(I::point(origin[i]))))
}

fn plane_controls(origin: [f64; 3], du: [I; 3], dv: [I; 3], domain: Rect, work: &mut usize) -> Result<[Control; 4]> {
    if origin.iter().any(|x| !x.is_finite()) || du.iter().chain(&dv).any(|x| !x.is_finite()) {
        return Err(FaceError::InvalidFace);
    }
    ray::spend(work, 64)?;
    Ok(std::array::from_fn(|i| {
        let u = domain[0][i % 2];
        let v = domain[1][i / 2];
        let [x, y, z] = std::array::from_fn(|j| I::point(origin[j]).add(du[j].scale(u)).add(dv[j].scale(v)));
        [x, y, z, I::point(1.0)]
    }))
}

fn tensor_size<T>(rows: &[Vec<T>], work: &mut usize) -> Result<[usize; 2]> {
    let size = [rows.len(), rows.first().map_or(0, Vec::len)];
    let count = size[0].checked_mul(size[1]).filter(|n| (1..=MAX_CONTROLS).contains(n)).ok_or(FaceError::GeometryBudget)?;
    ray::spend(work, 4 * count)?;
    if rows.iter().any(|row| row.len() != size[1]) {
        return Err(FaceError::UnsupportedSpline);
    }
    Ok(size)
}

/// Conservative bounds from the native surface. Trim preparation can fail
/// independently, so a face with uncertain trims retains this depth band.
pub(super) fn world_bounds(face: &Face, work: &mut usize) -> Result<[I; 3]> {
    let domain = face.domain;
    if !valid_rect(domain) {
        return Err(FaceError::InvalidFace);
    }
    match exact_parameters(face)? {
        Surface::Plane { origin, u_axis, v_axis } => {
            let controls = plane_controls(*origin, u_axis.map(I::point), v_axis.map(I::point), domain, work)?;
            control_bounds(controls.into_iter().map(Ok))
        }
        Surface::PlaneThrough { origin, u_point, v_point } => {
            let [du, dv] = through_axes(*origin, *u_point, *v_point);
            let controls = plane_controls(*origin, du, dv, domain, work)?;
            control_bounds(controls.into_iter().map(Ok))
        }
        Surface::Spline { u_knots, v_knots, points } => tensor_bounds([u_knots, v_knots], points, |[x, y, z]| [x, y, z, 1.0], domain, work),
        Surface::RationalSpline { u_knots, v_knots, homogeneous } => tensor_bounds([u_knots, v_knots], homogeneous, |p| p, domain, work),
        Surface::Cylinder { frame, radius } => revolved_bounds(frame, domain[1], I::point(*radius)),
        Surface::Cone { frame, radius_at_origin, half_angle } => {
            let tangent = half_angle.tan();
            let slope = I { lo: tangent.next_down(), hi: tangent.next_up() };
            let height = I { lo: domain[1][0], hi: domain[1][1] };
            revolved_bounds(frame, domain[1], I::point(*radius_at_origin).add(height.mul(slope)))
        }
        Surface::Sphere { frame, radius } => revolved_bounds(frame, [-radius.abs(), radius.abs()], I::point(radius.abs())),
        Surface::BallPatch { center, radius } => sphere_bounds(*center, I::point(radius.abs())),
        Surface::Torus { frame, major_radius, minor_radius, .. } => {
            let tube = minor_radius.abs();
            revolved_bounds(frame, [-tube, tube], I::point(major_radius.abs()).add(I::point(tube)))
        }
        _ => Err(FaceError::UnsupportedSurface),
    }
}

fn exact_parameters(face: &Face) -> Result<&Surface> {
    if matches!(&face.surface, Surface::Blend { definition } if matches!(definition.as_ref(), BlendDefinition::Procedural(_))) {
        return Err(FaceError::UnsupportedSurface);
    }
    Ok(face.parameters())
}

fn sphere_bounds(center: [f64; 3], radius: I) -> Result<[I; 3]> {
    let bounds = center.map(|coordinate| I::point(coordinate).add(radius.mul(I { lo: -1.0, hi: 1.0 })));
    bounds.iter().all(|bound| bound.is_finite()).then_some(bounds).ok_or(FaceError::UnsupportedSurface)
}

fn revolved_bounds(frame: &solidcore_record::brep::Frame, height: [f64; 2], radius: I) -> Result<[I; 3]> {
    let x = frame.x_axis.map(I::point);
    let y = frame.y_axis.map(I::point);
    let z = [x[1].mul(y[2]).sub(x[2].mul(y[1])), x[2].mul(y[0]).sub(x[0].mul(y[2])), x[0].mul(y[1]).sub(x[1].mul(y[0]))];
    let height = I { lo: height[0], hi: height[1] };
    let r = radius.mul(I { lo: -1.0, hi: 1.0 });
    let bounds = std::array::from_fn(|d| I::point(frame.origin[d]).add(z[d].mul(height)).add(x[d].mul(r)).add(y[d].mul(r)));
    bounds.iter().all(|bound| bound.is_finite()).then_some(bounds).ok_or(FaceError::UnsupportedSurface)
}

fn tensor_bounds<T: Copy>(knots: [&[f64]; 2], rows: &[Vec<T>], point: impl Fn(T) -> [f64; 4], domain: Rect, work: &mut usize) -> Result<[I; 3]> {
    let size = tensor_size(rows, work)?;
    for ((knots, count), [lo, hi]) in knots.into_iter().zip(size).zip(domain) {
        let (_, [a, b]) = Axis::validate(knots, count, work)?;
        if lo < a || hi > b {
            return Err(FaceError::UnsupportedSpline);
        }
    }
    ray::spend(work, 8 * size[0] * size[1])?;
    control_bounds(rows.iter().flatten().copied().map(|p| positive(point(p))))
}

fn control_bounds(controls: impl Iterator<Item = Result<Control>>) -> Result<[I; 3]> {
    let mut bounds = None;
    for p in controls {
        let p = p?;
        let cartesian = [p[0], p[1], p[2]].map(|coordinate| coordinate.div(p[3]));
        if cartesian.iter().any(|coordinate| !coordinate.is_finite()) {
            return Err(FaceError::UnsupportedSurface);
        }
        bounds = Some(bounds.map_or(cartesian, |previous: [I; 3]| std::array::from_fn(|i| previous[i].hull(cartesian[i]))));
    }
    bounds.ok_or(FaceError::InvalidFace)
}

fn positive(p: [f64; 4]) -> Result<Control> {
    if p[3] < f64::MIN_POSITIVE || p.iter().any(|x| !x.is_finite()) {
        return Err(FaceError::UnsupportedSpline);
    }
    Ok(p.map(I::point))
}

#[derive(Clone, Copy)]
struct TrimEnd {
    vertex: u32,
    uv: Control,
    tolerance: f64,
}

#[derive(Default)]
struct Trim {
    pieces: Vec<Vec<Control>>,
    joins: Vec<[I; 3]>,
}

impl Trim {
    fn prepare(record: &Brep, face: &Face, surface: &Sheet, work: &mut usize) -> Result<Self> {
        if face.loops.is_empty() || face.loops.len() > MAX_TRIM_EDGES {
            return Err(FaceError::UnqualifiedTrim);
        }
        let mut output = Self::default();
        let mut edge_count = 0;
        let mut controls = 0;
        for boundary in &face.loops {
            let Loop::Edges { edges } = boundary else { return Err(FaceError::UnqualifiedTrim) };
            edge_count += edges.len().min(MAX_TRIM_EDGES + 1);
            if edges.is_empty() || edge_count > MAX_TRIM_EDGES {
                return Err(FaceError::GeometryBudget);
            }
            let mut first = None;
            let mut last = None;
            for use_ in edges {
                ray::spend(work, 1)?;
                let edge = record.edges.get(use_.edge as usize).ok_or(FaceError::InvalidFace)?;
                let [from, to] = if use_.forward { edge.vertices } else { [edge.vertices[1], edge.vertices[0]] };
                if [from, to].iter().any(|&vertex| record.vertices.get(vertex as usize).is_none()) {
                    return Err(FaceError::InvalidFace);
                }
                let mut pieces = trim_spline(&use_.pcurve, edge.interval, work)?;
                if !use_.forward {
                    pieces.reverse();
                    for piece in &mut pieces {
                        piece.reverse();
                    }
                }
                let a = *pieces.first().and_then(|p| p.first()).ok_or(FaceError::UnqualifiedTrim)?;
                let b = *pieces.last().and_then(|p| p.last()).ok_or(FaceError::UnqualifiedTrim)?;
                let start = TrimEnd { vertex: from, uv: a, tolerance: edge.tolerance };
                if let Some(end) = last
                    && output.join(record, face, surface, end, start, work)?
                {
                    controls += 2;
                }
                first.get_or_insert(start);
                last = Some(TrimEnd { vertex: to, uv: b, tolerance: edge.tolerance });
                controls += pieces.iter().map(Vec::len).sum::<usize>();
                if controls > MAX_CONTROLS {
                    return Err(FaceError::GeometryBudget);
                }
                output.pieces.try_reserve(pieces.len()).map_err(|_| ProofError::Allocation)?;
                output.pieces.extend(pieces);
            }
            let (Some(start), Some(end)) = (first, last) else { return Err(FaceError::UnqualifiedTrim) };
            if output.join(record, face, surface, end, start, work)? {
                controls += 2;
                if controls > MAX_CONTROLS {
                    return Err(FaceError::GeometryBudget);
                }
            }
        }
        Ok(output)
    }

    fn join(&mut self, record: &Brep, face: &Face, surface: &Sheet, end: TrimEnd, start: TrimEnd, work: &mut usize) -> Result<bool> {
        if end.vertex != start.vertex {
            return Err(FaceError::UnqualifiedTrim);
        }
        if same_endpoint(end.uv, start.uv) {
            return Ok(false);
        }
        let vertex = record.vertices.get(end.vertex as usize).ok_or(FaceError::InvalidFace)?;
        let band = native_trim_join(face, surface, vertex, [end.tolerance, start.tolerance], [end.uv, start.uv], work)?;
        self.pieces.try_reserve(1).map_err(|_| ProofError::Allocation)?;
        self.joins.try_reserve(1).map_err(|_| ProofError::Allocation)?;
        let mut connector = reserved(2)?;
        connector.extend([end.uv, start.uv]);
        self.pieces.push(connector);
        self.joins.push(band);
        Ok(true)
    }
}

fn native_trim_join(
    face: &Face,
    surface: &Sheet,
    vertex: &Vertex,
    tolerances: [f64; 2],
    endpoints: [Control; 2],
    work: &mut usize,
) -> Result<[I; 3]> {
    ray::spend(work, 96)?;
    if vertex.point.iter().any(|x| !x.is_finite()) || [vertex.tolerance, tolerances[0], tolerances[1]].iter().any(|x| !x.is_finite() || *x < 0.0) {
        return Err(FaceError::UnqualifiedTrim);
    }
    let affine = match face.parameters() {
        Surface::Plane { origin, u_axis, v_axis } => Some((*origin, [u_axis.map(I::point), v_axis.map(I::point)])),
        Surface::PlaneThrough { origin, u_point, v_point } => Some((*origin, through_axes(*origin, *u_point, *v_point))),
        _ => None,
    };
    let radius = I::point(vertex.tolerance).add(I::point(tolerances[0].max(tolerances[1])).scale(2.0));
    let squared_radius = radius.mul(radius);
    if !radius.is_finite() || radius.lo < 0.0 || !squared_radius.is_finite() {
        return Err(FaceError::UnqualifiedTrim);
    }
    let center = vertex.point.map(I::point);
    let uv = endpoints.map(|p| [p[0].div(p[3]), p[1].div(p[3])]);
    let certify = |point: [I; 3]| {
        let distance = point.into_iter().zip(center).fold(I::ZERO, |sum, (axis, center)| {
            let delta = axis.sub(center);
            sum.add(delta.mul(delta))
        });
        if distance.is_finite() && distance.hi <= squared_radius.lo { Ok(()) } else { Err(FaceError::UnqualifiedTrim) }
    };
    if let Some((origin, [du, dv])) = affine {
        // A convex ball contains the entire affine lift when it contains both endpoints.
        for [u, v] in uv {
            certify(std::array::from_fn(|i| I::point(origin[i]).add(du[i].mul(u)).add(dv[i].mul(v))))?;
        }
    } else {
        let lift = match surface.side_lift(uv, work) {
            Err(FaceError::UnqualifiedTrim) => surface.corner_lift(uv, work),
            other => other,
        }?;
        certify(lift)?;
    }
    let extent = I { lo: -radius.hi, hi: radius.hi };
    let band = center.map(|x| x.add(extent));
    if band.iter().any(|axis| !axis.is_finite()) {
        return Err(FaceError::UnqualifiedTrim);
    }
    Ok(band)
}

fn same_endpoint(a: Control, b: Control) -> bool {
    // Exact homogeneous equality suffices. Different weights can represent the
    // same point, but a tolerance or equal enclosing interval does not prove it.
    [0, 1, 3].into_iter().all(|i| a[i].lo == a[i].hi && b[i].lo == b[i].hi && a[i].lo == b[i].lo)
}

fn trim_spline(curve: &Curve2, range: [f64; 2], work: &mut usize) -> Result<Vec<Vec<Control>>> {
    let (knots, count) = match curve {
        Curve2::Spline { knots, points } => (knots, points.len()),
        Curve2::RationalSpline { knots, homogeneous } => (knots, homogeneous.len()),
    };
    let mut axis = Axis::new(knots, count, work)?;
    let [lo, hi] = axis.domain()?;
    if range.iter().any(|x| !x.is_finite()) {
        return Err(FaceError::UnqualifiedTrim);
    }
    // Native pcurve evaluation clamps outside the active knots. A constant
    // endpoint tail has no boundary image beyond the endpoint itself.
    let range = range.map(|x| x.clamp(lo, hi));
    ray::spend(work, 4 * count)?;
    let mut controls = ray::allocate(count)?;
    for (i, p) in controls.iter_mut().enumerate() {
        *p = match curve {
            Curve2::Spline { points, .. } => points.get(i).map(|&[u, v]| [u, v, 0.0, 1.0]),
            Curve2::RationalSpline { homogeneous, .. } => homogeneous.get(i).map(|&[u, v, w]| [u, v, 0.0, w]),
        }
        .ok_or(FaceError::UnqualifiedTrim)
        .and_then(positive)?;
    }
    let mut grid = Grid::new([count, 1], controls)?;
    let spans = axis.spans(&mut grid, 0, range, work)?;
    let mut pieces = reserved(spans.len())?;
    for span in spans {
        ray::spend(work, axis.degree() + 1)?;
        let mut controls = ray::allocate(axis.degree() + 1)?;
        grid.copy_span([span.start, 0], [axis.degree() + 1, 1], &mut controls)?;
        pieces.push(controls);
    }
    Ok(pieces)
}
