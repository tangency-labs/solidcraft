//! Persistent free-form offsets with a whole-span normal-distance error certificate.
//! The certificate bounds the second derivative of the exact normal offset and of
//! the actual cubic interpolant (natural for legacy output, periodic for new closed output). The endpoint interpolation theorem then
//! bounds their distance everywhere, rather than just at selected samples. At the
//! node limit, interior splits align the curves geometrically instead of requiring
//! identical parameter speeds; stored nodes and interpolation remain unchanged.
use crate::{CurveKind, Sketch, SketchError, fit_beziers_mode};
use serde::{Deserialize, Serialize};
use solidcraft_geom::{Seg2, Vec2};

mod trim;
pub use trim::FreeformOffsetOutput;

pub const OFFSET_TOLERANCE: f64 = 0.002;
const MAX_PIECES: usize = 256;
const MAX_ASSOCIATIONS: usize = 128;
const MAX_OFFSET_WORK: usize = 1_048_576;
struct Fit {
    tolerance: f64,
    periodic: bool,
}
#[derive(Clone, Copy)]
struct Certificate {
    tolerance: f64,
    align_midpoints: bool,
}
type Result<T> = std::result::Result<T, SketchError>;
fn bad(message: &str) -> SketchError {
    SketchError::Invalid(message.into())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FreeformOffset {
    pub source: String,
    pub output: String,
    pub forward: bool,
    pub distance: f64,
    /// Uniform dyadic subdivision count per exact source piece; preserves point roles.
    pub divisions: usize,
    pub pieces: usize,
    pub closed: bool,
    pub tolerance: f64,
    pub certificate: u8,
}

#[derive(Clone, Copy)]
struct I {
    lo: f64,
    hi: f64,
}
impl I {
    fn value(v: f64) -> Self {
        Self { lo: v, hi: v }
    }
    fn hull(values: impl Iterator<Item = f64>) -> Self {
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for v in values {
            lo = lo.min(v);
            hi = hi.max(v);
        }
        let padding = 64.0 * f64::EPSILON * (lo.abs().max(hi.abs()) + 1.0);
        Self { lo: lo - padding, hi: hi + padding }
    }
    fn add(self, b: Self) -> Self {
        Self { lo: self.lo + b.lo, hi: self.hi + b.hi }
    }
    fn neg(self) -> Self {
        Self { lo: -self.hi, hi: -self.lo }
    }
    fn sub(self, b: Self) -> Self {
        self.add(b.neg())
    }
    fn mul(self, b: Self) -> Self {
        Self::hull([self.lo * b.lo, self.lo * b.hi, self.hi * b.lo, self.hi * b.hi].into_iter())
    }
    fn scale(self, b: f64) -> Self {
        self.mul(Self::value(b))
    }
    fn div(self, b: Self) -> Result<Self> {
        if b.lo <= 0.0 || !b.hi.is_finite() {
            return Err(bad("uncertifiable rational offset denominator"));
        }
        Ok(self.mul(Self { lo: 1.0 / b.hi, hi: 1.0 / b.lo }))
    }
    fn abs_max(self) -> f64 {
        self.lo.abs().max(self.hi.abs())
    }
    fn abs_min(self) -> f64 {
        if self.lo <= 0.0 && self.hi >= 0.0 { 0.0 } else { self.lo.abs().min(self.hi.abs()) }
    }
}
#[derive(Clone, Copy)]
struct V {
    x: I,
    y: I,
}
impl V {
    fn hull(values: &[Vec2]) -> Self {
        Self { x: I::hull(values.iter().map(|v| v.x)), y: I::hull(values.iter().map(|v| v.y)) }
    }
    fn sub(self, b: Self) -> Self {
        Self { x: self.x.sub(b.x), y: self.y.sub(b.y) }
    }
    fn mul(self, b: I) -> Self {
        Self { x: self.x.mul(b), y: self.y.mul(b) }
    }
    fn div(self, b: I) -> Result<Self> {
        Ok(Self { x: self.x.div(b)?, y: self.y.div(b)? })
    }
    fn upper(self) -> f64 {
        self.x.abs_max().hypot(self.y.abs_max())
    }
    fn lower(self) -> f64 {
        self.x.abs_min().hypot(self.y.abs_min())
    }
    fn cross(self, b: Self) -> I {
        self.x.mul(b.y).sub(self.y.mul(b.x))
    }
}

/// Homogeneous controls retain the original parameter during restriction. Re-normalising
/// a conic with Seg2::split_at would change that parameter and invalidate the certificate.
#[derive(Clone)]
struct H {
    n: Vec<Vec2>,
    d: Vec<f64>,
}
impl H {
    fn normal_end(&self, at_end: bool, distance: f64) -> Result<Vec2> {
        let last = self.n.len() - 1;
        let (i, j) = if at_end { (last, last - 1) } else { (0, 1) };
        let position = self.n[i] / self.d[i];
        let sign = if at_end { -1.0 } else { 1.0 };
        let dn = (self.n[j] - self.n[i]) * (last as f64 * sign);
        let dd = (self.d[j] - self.d[i]) * (last as f64 * sign);
        let tangent = ((dn - position * dd) / self.d[i]).normalized().ok_or_else(|| bad("offset source has a stationary tangent"))?;
        Ok(position + tangent.perp() * distance)
    }
    fn from_seg(seg: &Seg2) -> Result<Self> {
        let (points, weights) = match *seg {
            Seg2::Line { a, b } => (vec![a, b], vec![1.0; 2]),
            Seg2::Cubic { p0, p1, p2, p3 } => (vec![p0, p1, p2, p3], vec![1.0; 4]),
            Seg2::Bezier { n, p } if (1..=solidcraft_geom::MAX_BEZIER_DEGREE as u8).contains(&n) => {
                (p[..=usize::from(n)].to_vec(), vec![1.0; usize::from(n) + 1])
            }
            Seg2::Conic { a, apex, b, w } if w.is_finite() && w > 0.0 => (vec![a, apex, b], vec![1.0, w, 1.0]),
            _ => return Err(bad("unsupported exact free-form offset piece")),
        };
        if points.iter().any(|p| !p.is_finite()) {
            return Err(bad("nonfinite offset source"));
        }
        let scale = weights.iter().copied().fold(1.0_f64, f64::max);
        let d = weights.iter().map(|w| w / scale).collect::<Vec<_>>();
        Ok(Self { n: points.iter().zip(&d).map(|(p, w)| *p * *w).collect(), d })
    }
    fn split(&self, t: f64) -> (Self, Self) {
        let (mut n, mut d) = (self.n.clone(), self.d.clone());
        let (mut ln, mut rn, mut ld, mut rd) = (vec![n[0]], vec![n[n.len() - 1]], vec![d[0]], vec![d[d.len() - 1]]);
        for width in (1..n.len()).rev() {
            for i in 0..width {
                n[i] = n[i].lerp(n[i + 1], t);
                d[i] = d[i] * (1.0 - t) + d[i + 1] * t;
            }
            ln.push(n[0]);
            rn.push(n[width - 1]);
            ld.push(d[0]);
            rd.push(d[width - 1]);
        }
        rn.reverse();
        rd.reverse();
        (Self { n: ln, d: ld }, Self { n: rn, d: rd })
    }
    fn range(&self, a: f64, b: f64) -> Self {
        let left = if b < 1.0 { self.split(b).0 } else { self.clone() };
        if a > 0.0 { left.split(a / b).1 } else { left }
    }
    fn derivatives(&self) -> Result<[V; 4]> {
        let origin = self.n[0] / self.d[0];
        let mut n = self.n.iter().zip(&self.d).map(|(n, d)| *n - origin * *d).collect::<Vec<_>>();
        let mut d = self.d.clone();
        let mut nh = Vec::new();
        let mut dh = Vec::new();
        for _ in 0..4 {
            nh.push(V::hull(&n));
            dh.push(I::hull(d.iter().copied()));
            let degree = n.len().saturating_sub(1) as f64;
            n = if n.len() > 1 { n.windows(2).map(|p| (p[1] - p[0]) * degree).collect() } else { vec![Vec2::ZERO] };
            d = if d.len() > 1 { d.windows(2).map(|p| (p[1] - p[0]) * degree).collect() } else { vec![0.0] };
        }
        let r = nh[0].div(dh[0])?;
        let v = nh[1].sub(r.mul(dh[1])).div(dh[0])?;
        let a = nh[2].sub(v.mul(dh[1].scale(2.0))).sub(r.mul(dh[2])).div(dh[0])?;
        let j = nh[3].sub(a.mul(dh[1].scale(3.0))).sub(v.mul(dh[2].scale(3.0))).sub(r.mul(dh[3])).div(dh[0])?;
        Ok([r, v, a, j])
    }
    fn offset_second_bound(&self, distance: f64) -> Result<f64> {
        let [_, v, a, j] = self.derivatives()?;
        let speed = v.lower();
        if speed <= 1e-12 {
            return Err(bad("offset source has an unresolved stationary tangent"));
        }
        let curvature = v.cross(a).div(I { lo: speed.powi(3), hi: v.upper().powi(3) })?;
        if curvature.scale(distance).hi >= 1.0 - 1e-8 {
            return Err(bad("offset crosses a curvature cusp"));
        }
        let bound = a.upper() + distance.abs() * (2.0 * j.upper() / speed + 5.0 * (a.upper() / speed).powi(2));
        if !bound.is_finite() {
            return Err(bad("offset derivative bound overflow"));
        }
        Ok(bound)
    }
}

/// Subdivision tightens the proof, not the stored geometry. Endpoint discrepancies
/// are measured against the exact rational normal, then the second derivative
/// interpolation bound certifies the full intervening interval.
fn certify(h: &H, cubic: &[Vec2; 4], distance: f64, proof: Certificate, depth: u8, budget: &mut usize) -> Result<()> {
    let tolerance = proof.tolerance;
    if *budget == 0 {
        return Err(bad("offset certificate work limit exceeded"));
    }
    *budget -= 1;
    let endpoint_error = cubic[0].dist(h.normal_end(false, distance)?).max(cubic[3].dist(h.normal_end(true, distance)?));
    if endpoint_error + 1e-10 > tolerance {
        return Err(bad("offset spline exceeds its endpoint error certificate"));
    }
    let cubic_second = ((cubic[2] - cubic[1] * 2.0 + cubic[0]) * 6.0).len().max(((cubic[3] - cubic[2] * 2.0 + cubic[1]) * 6.0).len());
    if h.offset_second_bound(distance).is_ok_and(|second| endpoint_error + (second + cubic_second) / 8.0 * (1.0 + 1e-10) + 1e-10 <= tolerance) {
        return Ok(());
    }
    if depth >= 8 {
        return Err(bad("offset spline exceeds its whole-span error or regularity certificate"));
    }
    let (left, right) = h.split(0.5);
    let curve = Seg2::Cubic { p0: cubic[0], p1: cubic[1], p2: cubic[2], p3: cubic[3] };
    // A chord-length fit need not have the source's parameter speed. When node
    // refinement is exhausted, pair the exact offset midpoint with a point on
    // the fitted cubic instead of assuming its parameter is 0.5. Each interior
    // split defines an increasing, piecewise-affine correspondence. The same
    // endpoint/second-derivative theorem applies on every pair of restricted
    // spans, proving a whole-curve distance bound in both directions. Projection
    // only chooses the split; its accuracy is checked by the certificate itself.
    let split = if proof.align_midpoints { curve.closest_param(left.normal_end(true, distance)?) } else { 0.5 };
    if !split.is_finite() || !(1e-9..=1.0 - 1e-9).contains(&split) {
        return Err(bad("offset spline has no interior certificate correspondence"));
    }
    let (a, b) = curve.split_at(split);
    let controls = |s: Seg2| match s {
        Seg2::Cubic { p0, p1, p2, p3 } => Ok([p0, p1, p2, p3]),
        _ => Err(bad("invalid cubic certificate")),
    };
    certify(&left, &controls(a)?, distance, proof, depth + 1, budget)?;
    certify(&right, &controls(b)?, distance, proof, depth + 1, budget)
}

fn normal_point(seg: &Seg2, t: f64, d: f64) -> Result<Vec2> {
    let normal = seg.derivative(t).normalized().ok_or_else(|| bad("offset source has a stationary tangent"))?.perp();
    let p = seg.point_at(t) + normal * d;
    if !p.is_finite() {
        return Err(bad("nonfinite offset point"));
    }
    Ok(p)
}

fn verify_simple(points: &[Vec2], cubics: &[[Vec2; 4]], closed: bool) -> Result<()> {
    // Each cubic's derivative controls must lie in the forward half-plane of its
    // chord. This proves local injectivity; separated convex hulls prove distant
    // spans cannot intersect. Near or ambiguous contacts are refused atomically.
    for c in cubics {
        let chord = c[3] - c[0];
        if c.windows(2).any(|p| (p[1] - p[0]).dot(chord) <= 1e-18) {
            return Err(bad("offset spline cannot be certified locally simple"));
        }
    }
    // Adjacent spans need their own proof: individual monotonicity does not
    // preclude them crossing each other away from their common endpoint.
    let separates_joint = |a: &[Vec2; 4], b: &[Vec2; 4]| {
        let joint = a[3];
        let direction = (a[3] - a[0]) + (b[3] - b[0]);
        joint.dist(b[0]) < 1e-7
            && a[..3].iter().all(|p| (*p - joint).dot(direction) < -1e-18)
            && b[1..].iter().all(|p| (*p - joint).dot(direction) > 1e-18)
    };
    for pair in cubics.windows(2) {
        if !separates_joint(&pair[0], &pair[1]) {
            return Err(bad("offset junction cannot be certified simple"));
        }
    }
    if closed && cubics.first().zip(cubics.last()).is_some_and(|(first, last)| !separates_joint(last, first)) {
        return Err(bad("offset seam cannot be certified simple"));
    }
    for i in 0..cubics.len() {
        for j in i + 2..cubics.len() {
            if closed && i == 0 && j + 1 == cubics.len() {
                continue;
            }
            let a = V::hull(&cubics[i]);
            let b = V::hull(&cubics[j]);
            if a.x.hi < b.x.lo || b.x.hi < a.x.lo || a.y.hi < b.y.lo || b.y.hi < a.y.lo {
                continue;
            }
            // A separating axis from either chord also separates both convex hulls.
            let separated = [points[i + 1] - points[i], points[j + 1] - points[j]].iter().any(|d| {
                let n = d.perp();
                let a = I::hull(cubics[i].iter().map(|p| p.dot(n)));
                let b = I::hull(cubics[j].iter().map(|p| p.dot(n)));
                a.hi < b.lo || b.hi < a.lo
            });
            if !separated {
                return Err(bad("offset self-intersection or ambiguous close contact"));
            }
        }
    }
    Ok(())
}

fn geometry(sk: &Sketch, source: usize, forward: bool, d: f64, divisions: usize, fit: Fit, work: &mut usize) -> Result<(Vec<Vec2>, usize, bool)> {
    if *work == 0 {
        return Err(bad("offset transaction certificate work limit exceeded"));
    }
    let Fit { tolerance, periodic } = fit;
    let mut pieces = sk.exact_segs(source).ok_or_else(|| bad("offset source changed shape"))?;
    if pieces.is_empty()
        || pieces.len() > MAX_PIECES
        || divisions == 0
        || !divisions.is_power_of_two()
        || divisions > 256
        || pieces.len().saturating_mul(divisions) >= crate::MAX_SPLINE_POINTS
        || !d.is_finite()
        || d.abs() < 1e-9
        || !tolerance.is_finite()
        || tolerance <= 0.0
        || tolerance > OFFSET_TOLERANCE
    {
        return Err(bad("invalid or excessive certified offset"));
    }
    if !forward {
        pieces.reverse();
        pieces = pieces.into_iter().map(|p| p.reversed()).collect();
    }
    let mut points = Vec::new();
    let mut restricted = Vec::new();
    for piece in &pieces {
        let homogeneous = H::from_seg(piece)?;
        let first = normal_point(piece, 0.0, d)?;
        if points.last().is_some_and(|p: &Vec2| p.dist(first) > 1e-7) {
            return Err(bad("free-form offset has a nonsmooth source junction"));
        }
        if points.is_empty() {
            points.push(first);
        }
        for i in 0..divisions {
            let (a, b) = (i as f64 / divisions as f64, (i + 1) as f64 / divisions as f64);
            restricted.push(homogeneous.range(a, b));
            let end = normal_point(piece, b, d)?;
            points.push(end);
        }
    }
    let closed = pieces.first().is_some_and(|a| pieces.last().is_some_and(|b| a.point_at(0.0).dist(b.point_at(1.0)) < 1e-7));
    if closed && points[0].dist(points[points.len() - 1]) > 1e-7 {
        return Err(bad("free-form closed offset has a nonsmooth seam"));
    }
    if closed {
        let first = points[0];
        if let Some(last) = points.last_mut() {
            *last = first;
        }
    }
    let cubics = fit_beziers_mode(&points, periodic && closed)?;
    let allowance = (*work).min(65_536);
    let mut budget = allowance;
    let mut result =
        restricted.iter().zip(&cubics).try_for_each(|(h, c)| certify(h, c, d, Certificate { tolerance, align_midpoints: false }, 0, &mut budget));
    if result.is_err() && (divisions == 256 || pieces.len().saturating_mul(divisions.saturating_mul(2)) >= crate::MAX_SPLINE_POINTS) {
        result =
            restricted.iter().zip(&cubics).try_for_each(|(h, c)| certify(h, c, d, Certificate { tolerance, align_midpoints: true }, 0, &mut budget));
    }
    *work -= allowance - budget;
    result?;
    verify_simple(&points, &cubics, closed)?;
    Ok((points, pieces.len(), closed))
}

impl Sketch {
    /// Validate persisted association roles without solving or certifying geometry.
    pub fn validate_freeform_offsets(&self) -> Result<()> {
        if self.freeform_offsets.is_empty() {
            return Ok(());
        }
        if self.freeform_offsets.len() > MAX_ASSOCIATIONS {
            return Err(bad("too many associative free-form offsets"));
        }
        if self.points.len() > 100_000 || self.curves.len() > 100_000 {
            return Err(bad("offset sketch exceeds ownership validation limit"));
        }
        let mut owners = vec![0usize; self.points.len()];
        let mut references = 0usize;
        for curve in &self.curves {
            if matches!(&curve.kind, CurveKind::Spline {pts,..} if pts.len() > crate::MAX_SPLINE_POINTS) {
                return Err(bad("offset sketch contains an excessive spline"));
            }
            for index in curve.kind.point_ids() {
                references += 1;
                if references > 1_000_000 {
                    return Err(bad("offset point ownership work limit exceeded"));
                }
                if let Some(count) = owners.get_mut(index) {
                    *count += 1;
                }
            }
        }
        let mut outputs = std::collections::BTreeSet::new();
        for record in &self.freeform_offsets {
            if record.source.is_empty()
                || record.source.len() > 512
                || record.output.is_empty()
                || record.output.len() > 512
                || record.source == record.output
                || !matches!(record.certificate, 1 | 2)
                || record.pieces == 0
                || record.pieces > MAX_PIECES
                || record.divisions == 0
                || !record.divisions.is_power_of_two()
                || record.divisions > 256
                || record.pieces.saturating_mul(record.divisions) >= crate::MAX_SPLINE_POINTS
                || !record.distance.is_finite()
                || record.distance.abs() < 1e-9
                || !record.tolerance.is_finite()
                || record.tolerance <= 0.0
                || record.tolerance > OFFSET_TOLERANCE
                || !outputs.insert(record.output.as_str())
                || self.freeform_offsets.iter().any(|other| other.output == record.source && !outputs.contains(other.output.as_str()))
            {
                return Err(bad("invalid cyclic or unbounded offset association"));
            }
            let source = self.curve_index(&record.source).ok_or_else(|| bad("offset source vanished"))?;
            if !matches!(self.curves[source].kind, CurveKind::Ellipse { .. } | CurveKind::Conic { .. } | CurveKind::Spline { .. }) {
                return Err(bad("offset source changed shape"));
            }
            let output = self.curve_index(&record.output).ok_or_else(|| bad("offset output vanished"))?;
            match &self.curves[output].kind {
                CurveKind::Spline { pts, control: false, degree: 3, periodic, .. }
                    if *periodic == (record.certificate == 2)
                        && (record.certificate != 2 || record.closed)
                        && pts.len() == record.pieces * record.divisions + 1
                        && pts.iter().all(|i| *i < self.points.len())
                        && pts.iter().copied().collect::<std::collections::BTreeSet<_>>().len() == pts.len()
                        && pts
                            .iter()
                            .all(|i| self.points.get(*i).is_some_and(|point| point.fixed && point.link.is_none()) && owners.get(*i) == Some(&1)) => {}
                _ => return Err(bad("invalid offset output point roles")),
            }
        }
        Ok(())
    }
    pub fn add_freeform_offset(&mut self, source: usize, forward: bool, distance: f64) -> Result<usize> {
        if self.freeform_offsets.len() >= MAX_ASSOCIATIONS {
            return Err(bad("too many associative free-form offsets"));
        }
        let source_id = self.curves.get(source).ok_or_else(|| bad("missing offset source"))?.id.clone();
        let source_pieces = self.exact_segs(source).ok_or_else(|| bad("offset source changed shape"))?.len();
        let mut result = None;
        let mut failure = None;
        let mut work = MAX_OFFSET_WORK;
        for divisions in [1, 2, 4, 8, 16, 32, 64, 128, 256] {
            if source_pieces.saturating_mul(divisions) >= crate::MAX_SPLINE_POINTS {
                break;
            }
            match geometry(self, source, forward, distance, divisions, Fit { tolerance: OFFSET_TOLERANCE, periodic: true }, &mut work) {
                Ok(g) => {
                    result = Some((divisions, g));
                    break;
                }
                Err(error) => {
                    failure = Some(error);
                }
            }
        }
        let (divisions, (points, pieces, closed)) = result.ok_or_else(|| failure.unwrap_or_else(|| bad("offset subdivision limit exceeded")))?;
        let mut candidate = self.clone();
        let indices = points.iter().map(|p| candidate.add_point(*p, None)).collect::<Result<Vec<_>>>()?;
        for i in &indices {
            if let Some(p) = candidate.points.get_mut(*i) {
                p.fixed = true;
            }
        }
        let output = candidate.add_curve(CurveKind::Spline { pts: indices, control: false, degree: 3, periodic: closed, nurbs: None }, None)?;
        candidate.freeform_offsets.push(FreeformOffset {
            source: source_id,
            output: candidate.curves[output].id.clone(),
            forward,
            distance,
            divisions,
            pieces,
            closed,
            tolerance: OFFSET_TOLERANCE,
            certificate: if closed { 2 } else { 1 },
        });
        *self = candidate;
        Ok(output)
    }

    pub(crate) fn refresh_freeform_offsets(&mut self) -> Result<()> {
        self.validate_freeform_offsets()?;
        let mut candidate = self.clone();
        let mut seen = std::collections::BTreeSet::new();
        let mut work = MAX_OFFSET_WORK;
        for (record_index, record) in self.freeform_offsets.clone().into_iter().enumerate() {
            if !matches!(record.certificate, 1 | 2)
                || record.source == record.output
                || !seen.insert(record.output.clone())
                || candidate.freeform_offsets.iter().any(|later| later.output == record.source && !seen.contains(&later.output))
            {
                return Err(bad("invalid cyclic offset association"));
            }
            let source = candidate.curve_index(&record.source).ok_or_else(|| bad("offset source vanished"))?;
            let output = candidate.curve_index(&record.output).ok_or_else(|| bad("offset output vanished"))?;
            let source_pieces = candidate.exact_segs(source).ok_or_else(|| bad("offset source changed shape"))?.len();
            let mut divisions = record.divisions;
            if source_pieces != record.pieces {
                let lineage = candidate.freeform_offsets.iter().any(|parent| parent.output == record.source);
                if !lineage || source_pieces < record.pieces || source_pieces % record.pieces != 0 {
                    return Err(bad("offset source topology changed outside its refinement lineage"));
                }
                let ratio = source_pieces / record.pieces;
                if !ratio.is_power_of_two() {
                    return Err(bad("offset source refinement has incompatible point roles"));
                }
                divisions = (record.divisions / ratio).max(1);
            }
            let (points, pieces, closed) = loop {
                match geometry(
                    &candidate,
                    source,
                    record.forward,
                    record.distance,
                    divisions,
                    Fit { tolerance: record.tolerance, periodic: record.certificate == 2 },
                    &mut work,
                ) {
                    Ok(geometry) => break geometry,
                    Err(error) => {
                        if divisions >= 256 || source_pieces.saturating_mul(divisions.saturating_mul(2)) >= crate::MAX_SPLINE_POINTS {
                            return Err(bad(&format!("offset refinement limit reached while certifying its chain: {error}")));
                        }
                        divisions *= 2;
                    }
                }
            };
            if closed != record.closed {
                return Err(bad("offset source topology changed"));
            }
            let old_indices = match &candidate.curves[output].kind {
                CurveKind::Spline { pts, control: false, degree: 3, periodic, .. }
                    if *periodic == (record.certificate == 2)
                        && (record.certificate != 2 || record.closed)
                        && pts.len() == record.pieces * record.divisions + 1 =>
                {
                    pts.clone()
                }
                _ => return Err(bad("offset output was edited; break its association first")),
            };
            // Refinement never reuses or removes an old node ID. Existing dyadic
            // roles remain at the same normalized source parameters; new nodes
            // append to the point table, so dimensions keep their point indices.
            let old_spans = record.pieces * record.divisions;
            let new_spans = pieces * divisions;
            if new_spans < old_spans || new_spans % old_spans != 0 || !(new_spans / old_spans).is_power_of_two() {
                return Err(bad("offset refinement would change existing normalized point roles"));
            }
            let factor = new_spans / old_spans;
            let mut indices = Vec::with_capacity(points.len());
            for (role, pos) in points.iter().enumerate() {
                let index = if role % factor == 0 {
                    *old_indices.get(role / factor).ok_or_else(|| bad("offset node role vanished"))?
                } else {
                    candidate.add_point(*pos, None)?
                };
                indices.push(index);
            }
            for (i, pos) in indices.iter().zip(points) {
                let p = candidate.points.get_mut(*i).ok_or_else(|| bad("offset output point vanished"))?;
                p.pos = pos;
                p.fixed = true;
            }
            candidate.curves[output].kind =
                CurveKind::Spline { pts: indices, control: false, degree: 3, periodic: record.certificate == 2, nurbs: None };
            candidate.freeform_offsets[record_index].divisions = divisions;
            candidate.freeform_offsets[record_index].pieces = pieces;
        }
        *self = candidate;
        Ok(())
    }

    /// Give every other curve that took a generated offset node (a line drawn from an open
    /// offset's end) its own point there, coincident with the node: the output keeps sole
    /// ownership of its nodes, and the curve follows the offset when its source moves.
    /// Returns the ids of the constraints added.
    pub fn detach_offset_points(&mut self) -> Result<Vec<String>> {
        if self.freeform_offsets.is_empty() {
            return Ok(Vec::new());
        }
        let outputs: std::collections::BTreeSet<&str> = self.freeform_offsets.iter().map(|o| o.output.as_str()).collect();
        let nodes: std::collections::BTreeSet<usize> =
            self.curves.iter().filter(|c| outputs.contains(c.id.as_str())).flat_map(|c| c.kind.point_ids()).collect();
        let taken: Vec<(usize, usize)> = self
            .curves
            .iter()
            .enumerate()
            .filter(|(_, c)| !outputs.contains(c.id.as_str()))
            .flat_map(|(ci, c)| c.kind.point_ids().into_iter().filter(|p| nodes.contains(p)).map(move |p| (ci, p)))
            .collect();
        let mut added = Vec::new();
        for (ci, node) in taken {
            let at = self.points.get(node).map(|p| p.pos).ok_or_else(|| bad("offset output point vanished"))?;
            let own = self.add_point(at, None)?;
            let curve = self.curves.get_mut(ci).ok_or_else(|| bad("offset neighbour curve vanished"))?;
            curve.kind.map_points(&|p| if p == node { own } else { p });
            added.push(self.add_constraint(crate::ConstraintKind::Coincident { p: node, q: own }, None)?);
        }
        Ok(added)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certified_offset_irregular_closed_spline_at_node_limit() {
        let mut sk = Sketch::new();
        let pts = [(-17.0, 15.0), (0.0, 8.0), (17.0, 13.0), (15.0, 0.0), (5.0, -7.0), (-1.0, -14.0), (-9.0, -1.0), (-21.0, -1.0), (-17.0, 15.0)]
            .into_iter()
            .map(|(x, y)| sk.add_point(Vec2::new(x, y), None).unwrap())
            .collect();
        let source = sk.add_curve(CurveKind::Spline { pts, control: false, degree: 3, periodic: true, nurbs: None }, None).unwrap();
        let original = sk.clone();
        for (forward, distance) in [(true, 2.0), (false, -2.0)] {
            let mut sk = original.clone();
            let output = sk.add_freeform_offset(source, forward, distance).unwrap();
            assert_eq!(sk.freeform_offsets[0].divisions, 32);
            assert_eq!(&sk.points[..original.points.len()], &original.points);
            assert_eq!(sk.curves[source], original.curves[source]);
            let exact = sk.exact_segs(source).unwrap();
            let fitted = sk.exact_segs(output).unwrap();
            assert_eq!(fitted.first().unwrap().start(), fitted.last().unwrap().end());
            let mut max_error = 0.0_f64;
            for (i, piece) in exact.iter().enumerate() {
                for j in 0..=128 {
                    let t = j as f64 / 128.0;
                    let want = piece.point_at(t) + piece.derivative(t).normalized().unwrap().perp() * 2.0;
                    let span = i * 32 + (j / 4).min(31);
                    let span = if forward { span } else { fitted.len() - 1 - span };
                    let curve = fitted[span];
                    max_error = max_error.max(curve.point_at(curve.closest_param(want)).dist(want));
                }
            }
            assert!(max_error < OFFSET_TOLERANCE, "normal-offset error {max_error}");
            for curve in &fitted {
                for j in 0..=8 {
                    let p = curve.point_at(j as f64 / 8.0);
                    let distance = exact.iter().map(|s| s.point_at(s.closest_param(p)).dist(p)).fold(f64::INFINITY, f64::min);
                    assert!((distance - 2.0).abs() < OFFSET_TOLERANCE, "normal distance {distance}");
                }
            }
            let saved = serde_json::to_string(&sk).unwrap();
            let mut reopened: Sketch = serde_json::from_str(&saved).unwrap();
            reopened.refresh_offsets().unwrap();
            assert_eq!(reopened, sk);
        }
    }

    #[test]
    fn geometric_certificate_aligns_speed_but_keeps_error_regularity_and_work_limits() {
        let source = H::from_seg(&Seg2::Line { a: Vec2::ZERO, b: Vec2::X }).unwrap();
        // Analytically identical straight lines, traversed at different speeds.
        let cubic = [Vec2::new(0.0, 2.0), Vec2::new(0.1, 2.0), Vec2::new(0.2, 2.0), Vec2::new(1.0, 2.0)];
        let mut budget = 65_536;
        assert!(certify(&source, &cubic, 2.0, Certificate { tolerance: OFFSET_TOLERANCE, align_midpoints: false }, 0, &mut budget).is_err());
        let proof = Certificate { tolerance: OFFSET_TOLERANCE, align_midpoints: true };
        certify(&source, &cubic, 2.0, proof, 0, &mut budget).unwrap();
        assert!(certify(&source, &cubic, 2.0, proof, 0, &mut 0).is_err());
        let bowed = [cubic[0], cubic[1] + Vec2::Y, cubic[2] + Vec2::Y, cubic[3]];
        assert!(certify(&source, &bowed, 2.0, proof, 0, &mut budget).is_err());
        let stationary = H::from_seg(&Seg2::Line { a: Vec2::ZERO, b: Vec2::ZERO }).unwrap();
        assert!(certify(&stationary, &cubic, 2.0, proof, 0, &mut budget).is_err());
    }

    fn ellipse() -> (Sketch, usize) {
        let mut sketch = Sketch::new();
        let center = sketch.add_point(Vec2::ZERO, None).unwrap();
        let major = sketch.add_point(Vec2::new(10.0, 0.0), None).unwrap();
        let source = sketch.add_curve(CurveKind::Ellipse { c: center, m: major, r: 5.0 }, None).unwrap();
        (sketch, source)
    }

    #[test]
    fn certified_offset_ellipse_refreshes_exact_normals_and_identity() {
        let (mut sk, source) = ellipse();
        let output = sk.add_freeform_offset(source, true, -1.0).unwrap();
        let record = sk.freeform_offsets[0].clone();
        let original_ids = sk.curves[output].kind.point_ids().iter().map(|i| sk.points[*i].id.clone()).collect::<Vec<_>>();
        let verify = |sk: &Sketch| {
            let record = &sk.freeform_offsets[0];
            let base = sk.exact_segs(source).unwrap();
            let curve = sk.exact_segs(output).unwrap();
            for (piece, source_piece) in base.iter().enumerate() {
                for span in 0..record.divisions {
                    for t in [0.0, 0.13, 0.41, 0.79, 1.0] {
                        let want = normal_point(source_piece, (span as f64 + t) / record.divisions as f64, -1.0).unwrap();
                        let got = curve[piece * record.divisions + span].point_at(t);
                        assert!(got.dist(want) <= OFFSET_TOLERANCE, "{got:?} vs {want:?}");
                    }
                }
            }
        };
        verify(&sk);
        // Independent Steiner area check for the convex ellipse expanded by 1mm.
        // Simpson integration evaluates the analytic ellipse speed, not offset samples.
        let n = 4096usize;
        let h = std::f64::consts::TAU / n as f64;
        let speed = |t: f64| (100.0 * t.sin().powi(2) + 25.0 * t.cos().powi(2)).sqrt();
        let perimeter = (0..=n)
            .map(|i| {
                speed(i as f64 * h)
                    * if i == 0 || i == n {
                        1.0
                    } else if i % 2 == 0 {
                        2.0
                    } else {
                        4.0
                    }
            })
            .sum::<f64>()
            * h
            / 3.0;
        let expected_area = 50.0 * std::f64::consts::PI + perimeter + std::f64::consts::PI;
        let actual_area = sk.exact_segs(output).unwrap().iter().map(Seg2::area_term).sum::<f64>();
        assert!((actual_area - expected_area).abs() < 0.15, "area {actual_area} vs {expected_area}");
        if let CurveKind::Ellipse { m, .. } = sk.curves[source].kind {
            sk.points[m].pos = Vec2::new(12.0, 0.0);
        }
        sk.refresh_offsets().unwrap();
        assert!(sk.freeform_offsets[0].divisions >= record.divisions);
        let factor = sk.freeform_offsets[0].divisions / record.divisions;
        assert_eq!(sk.curves[output].kind.point_ids().iter().step_by(factor).map(|i| sk.points[*i].id.clone()).collect::<Vec<_>>(), original_ids);
        verify(&sk);
        let first = sk.segs(output)[0].point_at(0.0);
        assert!(first.dist(Vec2::new(13.0, 0.0)) < 1e-10);
        let encoded = serde_json::to_string(&sk).unwrap();
        let mut reopened: Sketch = serde_json::from_str(&encoded).unwrap();
        reopened.refresh_offsets().unwrap();
        assert_eq!(reopened, sk);
    }

    #[test]
    fn certified_offset_refuses_cusp_and_hostile_records_atomically() {
        let (mut sk, source) = ellipse();
        let before = sk.clone();
        // At (10,0), curvature radius is b²/a=2.5. An inward offset of 3
        // necessarily crosses the evolute and reverses its tangent orientation.
        assert!(sk.add_freeform_offset(source, true, 3.0).is_err());
        assert_eq!(sk, before);
        sk.add_freeform_offset(source, true, -1.0).unwrap();
        for divisions in [0, 3, usize::MAX] {
            let mut candidate = sk.clone();
            candidate.freeform_offsets[0].divisions = divisions;
            let before = candidate.clone();
            assert!(candidate.refresh_offsets().is_err());
            assert_eq!(candidate, before);
        }
        let mut cyclic = sk.clone();
        cyclic.freeform_offsets[0].source = cyclic.freeform_offsets[0].output.clone();
        let before = cyclic.clone();
        assert!(cyclic.refresh_offsets().is_err());
        assert_eq!(cyclic, before);
        let source_points = sk.curves[source].kind.point_ids();
        let output = sk.curve_index(&sk.freeform_offsets[0].output).unwrap();
        let mut aliased = sk.clone();
        if let CurveKind::Spline { pts, .. } = &mut aliased.curves[output].kind {
            pts[0] = source_points[0];
        }
        let before = aliased.clone();
        assert!(aliased.validate_freeform_offsets().is_err());
        assert!(aliased.refresh_offsets().is_err());
        assert_eq!(aliased, before);
    }

    #[test]
    fn certified_offset_chain_preserves_roles_during_upstream_refinement() {
        let mut sk = Sketch::new();
        let start = sk.add_point(Vec2::new(10.0, 0.0), None).unwrap();
        let apex = sk.add_point(Vec2::new(10.0, 5.0), None).unwrap();
        let end = sk.add_point(Vec2::new(0.0, 5.0), None).unwrap();
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let source = sk.add_curve(CurveKind::Conic { a: start, b: end, apex, rho: w / (1.0 + w) }, None).unwrap();
        let a = sk.add_freeform_offset(source, true, -1.0).unwrap();
        let b = sk.add_freeform_offset(a, true, -1.0).unwrap();
        let original = [a, b].map(|i| sk.curves[i].kind.point_ids().iter().map(|point| sk.points[*point].id.clone()).collect::<Vec<_>>());
        let records = sk.freeform_offsets.clone();
        for major in [12.0, 16.0, 20.0, 24.0] {
            sk.points[start].pos.x = major;
            sk.points[apex].pos.x = major;
            sk.refresh_offsets().unwrap();
            if sk.freeform_offsets[0].divisions > records[0].divisions {
                break;
            }
        }
        assert!(sk.freeform_offsets[0].divisions > records[0].divisions, "test must exercise real upstream refinement");
        for (record_index, output) in [a, b].into_iter().enumerate() {
            let old = &records[record_index];
            let current = &sk.freeform_offsets[record_index];
            let factor = current.pieces * current.divisions / (old.pieces * old.divisions);
            assert_eq!(
                sk.curves[output].kind.point_ids().iter().step_by(factor).map(|i| sk.points[*i].id.clone()).collect::<Vec<_>>(),
                original[record_index]
            );
            let source = sk.curve_index(&current.source).unwrap();
            let source_pieces = sk.exact_segs(source).unwrap();
            let output_pieces = sk.exact_segs(output).unwrap();
            for (piece, source) in source_pieces.iter().enumerate() {
                for span in 0..current.divisions {
                    let t = 0.371;
                    let want = normal_point(source, (span as f64 + t) / current.divisions as f64, current.distance).unwrap();
                    assert!(output_pieces[piece * current.divisions + span].point_at(t).dist(want) < OFFSET_TOLERANCE);
                }
            }
        }
        let saved = serde_json::to_string(&sk).unwrap();
        let mut reopened: Sketch = serde_json::from_str(&saved).unwrap();
        reopened.refresh_offsets().unwrap();
        assert_eq!(reopened, sk);
    }

    #[test]
    fn certified_offset_closed_chain_refuses_natural_spline_seam_atomically() {
        let (mut sk, source) = ellipse();
        let first = sk.add_freeform_offset(source, true, -1.0).unwrap();
        // Explicitly reconstruct the old saved semantics, not the new periodic output.
        if let CurveKind::Spline { periodic, .. } = &mut sk.curves[first].kind {
            *periodic = false;
        }
        sk.freeform_offsets[0].certificate = 1;
        sk.refresh_offsets().unwrap();
        let before = sk.clone();
        let error = sk.add_freeform_offset(first, true, -0.5).unwrap_err();
        assert!(error.to_string().contains("nonsmooth seam"), "{error}");
        assert_eq!(sk, before);
    }

    #[test]
    fn certified_offset_periodic_closed_chain_has_c2_seams_and_stable_roles() {
        let (mut sk, source) = ellipse();
        let source_points = sk.curves[source].kind.point_ids();
        let unchanged = source_points.iter().map(|i| sk.points[*i].clone()).collect::<Vec<_>>();
        let a = sk.add_freeform_offset(source, true, -1.0).unwrap();
        let b = sk.add_freeform_offset(a, true, -0.5).unwrap();
        assert_eq!(source_points.iter().map(|i| sk.points[*i].clone()).collect::<Vec<_>>(), unchanged);
        let old = sk.freeform_offsets.clone();
        let ids = [a, b].map(|i| sk.curves[i].kind.point_ids().iter().map(|p| sk.points[*p].id.clone()).collect::<Vec<_>>());
        let verify = |sk: &Sketch| {
            for output in [a, b] {
                let record = sk.freeform_offsets.iter().find(|r| r.output == sk.curves[output].id).unwrap();
                assert_eq!(record.certificate, 2);
                let points = sk.curves[output].kind.point_ids().iter().map(|p| sk.point(*p).unwrap()).collect::<Vec<_>>();
                let cubics = crate::periodic_beziers(&points).unwrap();
                let first = cubics[0];
                let last = *cubics.last().unwrap();
                let (h0, h1) = (points[0].dist(points[1]), points[points.len() - 2].dist(points[points.len() - 1]));
                assert_eq!(first[0], last[3]);
                assert!(((first[1] - first[0]) / h0 - (last[3] - last[2]) / h1).len() < 1e-10);
                assert!(((first[2] - first[1] * 2.0 + first[0]) / (h0 * h0) - (last[3] - last[2] * 2.0 + last[1]) / (h1 * h1)).len() < 1e-9);
                let source = sk.curve_index(&record.source).unwrap();
                for (i, piece) in sk.exact_segs(source).unwrap().iter().enumerate() {
                    for j in 0..record.divisions {
                        for t in [0.0, 0.13, 0.41, 0.79, 1.0] {
                            let exact = normal_point(piece, (j as f64 + t) / record.divisions as f64, record.distance).unwrap();
                            let got = sk.exact_segs(output).unwrap()[i * record.divisions + j].point_at(t);
                            assert!(got.dist(exact) <= OFFSET_TOLERANCE);
                            if source == 0 {
                                // Independent ellipse implicit-gradient normal, rather
                                // than using the offset implementation as its own oracle.
                                let base = piece.point_at((j as f64 + t) / record.divisions as f64);
                                let major = sk.point(source_points[1]).unwrap().x;
                                let normal = Vec2::new(base.x / (major * major), base.y / 25.0).normalized().unwrap();
                                assert!(got.dist(base + normal) <= OFFSET_TOLERANCE);
                            }
                        }
                    }
                }
            }
        };
        verify(&sk);
        // Steiner's formula is independent of fitted output: both offsets expand
        // the original ellipse by 1.5mm, within the accumulated two certificates.
        let n = 4096;
        let h = std::f64::consts::TAU / n as f64;
        let perimeter = (0..=n)
            .map(|i| {
                let t = i as f64 * h;
                (100.0 * t.sin().powi(2) + 25.0 * t.cos().powi(2)).sqrt()
                    * if i == 0 || i == n {
                        1.0
                    } else if i % 2 == 0 {
                        2.0
                    } else {
                        4.0
                    }
            })
            .sum::<f64>()
            * h
            / 3.0;
        let expected = 50.0 * std::f64::consts::PI + 1.5 * perimeter + 2.25 * std::f64::consts::PI;
        let area = sk.exact_segs(b).unwrap().iter().map(Seg2::area_term).sum::<f64>();
        assert!((area - expected).abs() < 0.03, "{area} vs {expected}");
        let profiles = crate::find_profiles(&sk);
        assert_eq!(profiles.len(), 3);
        if let CurveKind::Ellipse { m, .. } = sk.curves[source].kind {
            sk.points[m].pos.x = 12.0;
        }
        sk.refresh_offsets().unwrap();
        verify(&sk);
        for (j, i) in [a, b].into_iter().enumerate() {
            let now = &sk.freeform_offsets[j];
            let factor = now.pieces * now.divisions / (old[j].pieces * old[j].divisions);
            assert_eq!(sk.curves[i].kind.point_ids().iter().step_by(factor).map(|p| sk.points[*p].id.clone()).collect::<Vec<_>>(), ids[j]);
        }
        let encoded = serde_json::to_string(&sk).unwrap();
        let mut reopened: Sketch = serde_json::from_str(&encoded).unwrap();
        reopened.validate_periodic_splines().unwrap();
        reopened.refresh_offsets().unwrap();
        assert_eq!(sk, reopened);
    }

    #[test]
    fn certified_offset_periodic_circle_and_reverse_have_analytic_normals() {
        for forward in [true, false] {
            let mut sk = Sketch::new();
            let center = sk.add_point(Vec2::ZERO, None).unwrap();
            let major = sk.add_point(Vec2::new(5.0, 0.0), None).unwrap();
            let source = sk.add_curve(CurveKind::Ellipse { c: center, m: major, r: 5.0 }, None).unwrap();
            let distance = if forward { -1.0 } else { 1.0 };
            let a = sk.add_freeform_offset(source, forward, distance).unwrap();
            let b = sk.add_freeform_offset(a, true, distance * 0.5).unwrap();
            for (output, radius) in [(a, 6.0), (b, 6.5)] {
                let pieces = sk.exact_segs(output).unwrap();
                for curve in &pieces {
                    for t in [0.0, 0.13, 0.41, 0.79, 1.0] {
                        assert!((curve.point_at(t).len() - radius).abs() <= 2.0 * OFFSET_TOLERANCE);
                    }
                }
                let area = pieces.iter().map(Seg2::area_term).sum::<f64>().abs();
                assert!((area - std::f64::consts::PI * radius * radius).abs() < 0.03);
            }
            sk.refresh_offsets().unwrap();
            sk.validate_periodic_splines().unwrap();
        }
    }
}
