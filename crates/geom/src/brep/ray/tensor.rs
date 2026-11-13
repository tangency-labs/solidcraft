//! Conservative ray proof for one trim-clear, material rational Bézier tile.
//!
//! The supplied coefficient intervals enclose the authoritative tile. No
//! inference from sampled positions or display geometry enters an answer.

pub(super) const MAX_DEGREE: usize = 16;
const MAX_DEPTH: usize = 24;
const REFINEMENTS: usize = 8;
const REFINEMENT_WIDTH: f64 = 1e-12;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RayProof {
    Miss,
    /// Exactly one transverse hit. UV is in the input tile's unit square.
    /// Depth is t in `origin + t * direction`, even for a non-unit direction.
    Unique {
        uv: [[f64; 2]; 2],
        depth: [f64; 2],
    },
    Unresolved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProofError {
    InvalidTile,
    InvalidRay,
    WorkExhausted,
    Allocation,
}

type Result<T> = std::result::Result<T, ProofError>;

#[derive(Clone, Copy, Debug)]
pub(super) struct Interval {
    pub(super) lo: f64,
    pub(super) hi: f64,
}

impl Interval {
    pub(super) const ZERO: Self = Self::point(0.0);
    const WHOLE: Self = Self { lo: f64::NEG_INFINITY, hi: f64::INFINITY };
    pub(super) const UNIT: Self = Self { lo: 0.0, hi: 1.0 };
    const CENTERED: Self = Self { lo: -0.5, hi: 0.5 };

    pub(super) const fn point(value: f64) -> Self {
        Self { lo: value, hi: value }
    }

    fn rounded(lo: f64, hi: f64) -> Self {
        if lo.is_finite() && hi.is_finite() && lo <= hi { Self { lo: lo.next_down(), hi: hi.next_up() } } else { Self::WHOLE }
    }

    pub(super) fn is_finite(self) -> bool {
        self.lo.is_finite() && self.hi.is_finite() && self.lo <= self.hi
    }

    fn is_zero(self) -> bool {
        self.lo == 0.0 && self.hi == 0.0
    }

    fn contains_zero(self) -> bool {
        self.lo <= 0.0 && self.hi >= 0.0
    }

    pub(super) fn add(self, other: Self) -> Self {
        if self.is_zero() {
            other
        } else if other.is_zero() {
            self
        } else {
            Self::rounded(self.lo + other.lo, self.hi + other.hi)
        }
    }

    fn neg(self) -> Self {
        Self { lo: -self.hi, hi: -self.lo }
    }

    pub(super) fn sub(self, other: Self) -> Self {
        if self.is_finite() && self.lo == self.hi && self.lo == other.lo && self.lo == other.hi { Self::ZERO } else { self.add(other.neg()) }
    }

    fn products(values: [f64; 4]) -> Self {
        if values.iter().any(|value| !value.is_finite()) {
            return Self::WHOLE;
        }
        Self::rounded(values.into_iter().fold(f64::INFINITY, f64::min), values.into_iter().fold(f64::NEG_INFINITY, f64::max))
    }

    pub(super) fn mul(self, other: Self) -> Self {
        if !self.is_finite() || !other.is_finite() {
            Self::WHOLE
        } else if self.is_zero() || other.is_zero() {
            Self::ZERO
        } else {
            Self::products([self.lo * other.lo, self.lo * other.hi, self.hi * other.lo, self.hi * other.hi])
        }
    }

    pub(super) fn scale(self, factor: f64) -> Self {
        if factor == 1.0 {
            self
        } else if factor == -1.0 {
            self.neg()
        } else {
            self.mul(Self::point(factor))
        }
    }

    pub(super) fn div(self, other: Self) -> Self {
        if !self.is_finite() || !other.is_finite() || other.contains_zero() {
            Self::WHOLE
        } else if other.lo == 1.0 && other.hi == 1.0 {
            self
        } else if other.lo == -1.0 && other.hi == -1.0 {
            self.neg()
        } else if self.is_zero() {
            Self::ZERO
        } else {
            Self::products([self.lo / other.lo, self.lo / other.hi, self.hi / other.lo, self.hi / other.hi])
        }
    }

    pub(super) fn hull(self, other: Self) -> Self {
        Self { lo: self.lo.min(other.lo), hi: self.hi.max(other.hi) }
    }

    pub(super) fn intersect(self, other: Self) -> Option<Self> {
        let result = Self { lo: self.lo.max(other.lo), hi: self.hi.min(other.hi) };
        (result.lo <= result.hi).then_some(result)
    }

    pub(super) fn midpoint(self) -> f64 {
        self.lo * 0.5 + self.hi * 0.5
    }

    pub(super) fn pair(self) -> [f64; 2] {
        [self.lo, self.hi]
    }
}

// Two homogeneous line equations, dominant-coordinate depth numerator,
// and its denominator. U is the fast index, V the slow index.
pub(super) type Control = [Interval; 4];
type Matrix = [[Interval; 2]; 2];

pub(super) fn spend(work: &mut usize, cost: usize) -> Result<()> {
    *work = work.checked_sub(cost).ok_or(ProofError::WorkExhausted)?;
    Ok(())
}

pub(super) fn allocate(count: usize) -> Result<Vec<Control>> {
    let mut controls = Vec::new();
    controls.try_reserve_exact(count).map_err(|_| ProofError::Allocation)?;
    controls.resize(count, [Interval::ZERO; 4]);
    Ok(controls)
}

/// Prove the ray intersection with a single bounded trim-clear material tile.
/// `controls` is `[x*w, y*w, z*w, w]` in row-major order, U varying fastest.
/// Boundary roots and tangencies may be unresolved. No incomplete search is a miss.
#[cfg(test)]
pub fn prove(
    degree: [usize; 2],
    controls: &[[f64; 4]],
    origin: [f64; 3],
    direction: [f64; 3],
    ray_parameter: [f64; 2],
    work: &mut usize,
) -> Result<RayProof> {
    prove_with(degree, controls.iter().map(|p| p.map(Interval::point)), origin, direction, ray_parameter, work)
}

pub(super) fn prove_enclosed(
    degree: [usize; 2],
    controls: &[Control],
    origin: [f64; 3],
    direction: [f64; 3],
    ray_parameter: [f64; 2],
    work: &mut usize,
) -> Result<RayProof> {
    prove_with(degree, controls.iter().copied(), origin, direction, ray_parameter, work)
}

fn prove_with(
    degree: [usize; 2],
    controls: impl ExactSizeIterator<Item = Control>,
    origin: [f64; 3],
    direction: [f64; 3],
    ray_parameter: [f64; 2],
    work: &mut usize,
) -> Result<RayProof> {
    if degree.iter().any(|&n| !(1..=MAX_DEGREE).contains(&n)) {
        return Err(ProofError::InvalidTile);
    }
    let size = degree.map(|n| n + 1);
    if controls.len() != size[0] * size[1] {
        return Err(ProofError::InvalidTile);
    }
    let axis = (0..3).max_by(|&a, &b| direction[a].abs().total_cmp(&direction[b].abs())).ok_or(ProofError::InvalidRay)?;
    if origin.into_iter().chain(direction).chain(ray_parameter).any(|v| !v.is_finite())
        || direction[axis] == 0.0
        || ray_parameter[0] < 0.0
        || ray_parameter[0] > ray_parameter[1]
    {
        return Err(ProofError::InvalidRay);
    }

    spend(work, 17 * controls.len())?;
    let mut net = allocate(controls.len())?;
    let other = [(axis + 1) % 3, (axis + 2) % 3];
    for (output, p) in net.iter_mut().zip(controls) {
        if p[3].lo < f64::MIN_POSITIVE || p.iter().any(|v| !v.is_finite()) {
            return Err(ProofError::InvalidTile);
        }
        let [x, y, z, w] = p;
        let xyz = [x, y, z];
        let shifted: [Interval; 3] = std::array::from_fn(|i| xyz[i].sub(w.scale(origin[i])));
        // d_k * (P_i - O_i) - d_i * (P_k - O_k) = 0, with exact f64
        // ray coefficients interpreted as reals. No rounded perpendicular basis.
        let [a, b] = other.map(|i| shifted[i].scale(direction[axis]).sub(shifted[axis].scale(direction[i])));
        *output = [a, b, shifted[axis], w.scale(direction[axis])];
    }
    let query = Search { size, segment: Interval { lo: ray_parameter[0], hi: ray_parameter[1] } };
    query.visit(&net, [Interval::UNIT; 2], 0, work)
}

pub(super) fn halve_curve(controls: &[Control], work: &mut usize) -> Result<[Vec<Control>; 2]> {
    if !(2..=MAX_DEGREE + 1).contains(&controls.len()) {
        return Err(ProofError::InvalidTile);
    }
    Search { size: [controls.len(), 1], segment: Interval::UNIT }.split(controls, 0, work)
}

struct Search {
    size: [usize; 2],
    segment: Interval,
}

impl Search {
    fn visit(&self, net: &[Control], uv: [Interval; 2], depth: usize, work: &mut usize) -> Result<RayProof> {
        spend(work, 6 * net.len())?;
        let [a, b] = [0, 1].map(|i| net.iter().map(|p| p[i]).reduce(Interval::hull).unwrap_or(Interval::WHOLE));
        let t = net.iter().map(|p| p[2].div(p[3])).reduce(Interval::hull).unwrap_or(Interval::WHOLE);
        // Each equation is polynomial. The rational depth is a convex combination
        // of control quotients because every denominator has the same sign.
        if !a.contains_zero() || !b.contains_zero() || t.intersect(self.segment).is_none() {
            return Ok(RayProof::Miss);
        }
        if !a.is_finite() || !b.is_finite() || !t.is_finite() || a.is_zero() || b.is_zero() {
            return Ok(RayProof::Unresolved);
        }

        match self.root(net, work)? {
            Root::Empty => return Ok(RayProof::Miss),
            Root::Unique(local) => {
                let p = self.evaluate(net, local, work)?;
                // Both the full control hull and restricted evaluation enclose
                // the root depth. Intersect the two, preserving inconclusiveness.
                let Some(depth) = t.intersect(p[2].div(p[3])) else { return Ok(RayProof::Unresolved) };
                if depth.intersect(self.segment).is_none() {
                    return Ok(RayProof::Miss);
                }
                if self.segment.lo < depth.lo && depth.hi < self.segment.hi {
                    let uv = std::array::from_fn(|i| {
                        // The domain endpoints are exact dyadics up to MAX_DEPTH.
                        Interval::point(uv[i].lo).add(local[i].scale(uv[i].hi - uv[i].lo)).pair()
                    });
                    return Ok(RayProof::Unique { uv, depth: depth.pair() });
                }
                return Ok(RayProof::Unresolved);
            }
            Root::Unknown => {}
        }
        if depth >= MAX_DEPTH {
            return Ok(RayProof::Unresolved);
        }

        let axis = depth % 2;
        let [left, right] = self.split(net, axis, work)?;
        let midpoint = uv[axis].midpoint();
        let mut left_uv = uv;
        let mut right_uv = uv;
        left_uv[axis].hi = midpoint;
        right_uv[axis].lo = midpoint;
        let first = self.visit(&left, left_uv, depth + 1, work)?;
        if first == RayProof::Unresolved {
            return Ok(first);
        }
        let second = self.visit(&right, right_uv, depth + 1, work)?;
        // Closed children cover the parent. A witness is strictly interior to
        // its proving box, so two witnesses cannot be a duplicated seam root.
        Ok(match (first, second) {
            (RayProof::Miss, proof) | (proof, RayProof::Miss) => proof,
            _ => RayProof::Unresolved,
        })
    }

    fn evaluate(&self, net: &[Control], uv: [Interval; 2], work: &mut usize) -> Result<Control> {
        let [nu, nv] = self.size;
        let mut rows = [[Interval::ZERO; 4]; MAX_DEGREE + 1];
        for (out, row) in rows.iter_mut().zip(net.chunks_exact(nu)) {
            *out = curve_value(row, uv[0], work)?;
        }
        curve_value(&rows[..nv], uv[1], work)
    }

    fn derivative_bounds(&self, net: &[Control], work: &mut usize) -> Result<Matrix> {
        let [nu, _] = self.size;
        spend(work, 12 * net.len())?;
        let mut bounds = [[None; 2]; 2];
        for axis in 0..2 {
            let stride = if axis == 0 { 1 } else { nu };
            let degree = (self.size[axis] - 1) as f64;
            for (i, p) in net.iter().enumerate() {
                let index = if axis == 0 { i % nu } else { i / nu };
                if index + 1 >= self.size[axis] {
                    continue;
                }
                let Some(next) = net.get(i + stride) else { return Err(ProofError::InvalidTile) };
                for (row, (a, b)) in bounds.iter_mut().zip(p.iter().zip(next)) {
                    let delta = b.sub(*a).scale(degree);
                    row[axis] = Some(row[axis].map_or(delta, |hull: Interval| hull.hull(delta)));
                }
            }
        }
        Ok(bounds.map(|row| row.map(|range| range.unwrap_or(Interval::WHOLE))))
    }

    fn derivative_on(&self, net: &[Control], uv: [Interval; 2], work: &mut usize) -> Result<Matrix> {
        let [nu, nv] = self.size;
        spend(work, 8 * net.len())?;
        let mut partials = [[Interval::ZERO; 2]; 2];
        for (axis, partial) in partials.iter_mut().enumerate() {
            let stride = if axis == 0 { 1 } else { nu };
            let columns = nu - usize::from(axis == 0);
            let rows = nv - usize::from(axis == 1);
            let degree = (self.size[axis] - 1) as f64;
            let mut column = [[Interval::ZERO; 4]; MAX_DEGREE + 1];
            for (v, out) in column.iter_mut().enumerate().take(rows) {
                let mut row = [[Interval::ZERO; 4]; MAX_DEGREE + 1];
                for (u, dst) in row.iter_mut().enumerate().take(columns) {
                    let i = v * nu + u;
                    let (Some(a), Some(b)) = (net.get(i), net.get(i + stride)) else { return Err(ProofError::InvalidTile) };
                    for j in 0..2 {
                        dst[j] = b[j].sub(a[j]).scale(degree);
                    }
                }
                *out = curve_value(&row[..columns], uv[0], work)?;
            }
            let value = curve_value(&column[..rows], uv[1], work)?;
            *partial = [value[0], value[1]];
        }
        let [[du_a, du_b], [dv_a, dv_b]] = partials;
        Ok([[du_a, dv_a], [du_b, dv_b]])
    }

    fn root(&self, net: &[Control], work: &mut usize) -> Result<Root> {
        let derivative = self.derivative_bounds(net, work)?;
        let Some(preconditioner) = approximate_inverse(derivative) else { return Ok(Root::Unknown) };
        let value = self.evaluate(net, [Interval::point(0.5); 2], work)?;
        spend(work, 32)?;
        let first_map = FixedPoint::new(preconditioner, derivative, value, [0.5; 2]);
        let first = first_map.image([Interval::CENTERED; 2]);
        if first.iter().any(|part| part.intersect(Interval::UNIT).is_none()) {
            // Every root is a fixed point even when the map is not a contraction.
            return Ok(Root::Empty);
        }
        if !first_map.contracts() || first.iter().any(|part| !(0.0 < part.lo && part.hi < 1.0)) {
            return Ok(Root::Unknown);
        }

        // A strict self-map and ||I - B J||_infinity < 1 give a unique fixed
        // point. They also imply that B and every J in the box are nonsingular.
        let mut enclosure = first;
        for _ in 0..REFINEMENTS {
            if enclosure.iter().all(|i| i.hi - i.lo <= REFINEMENT_WIDTH) {
                break;
            }
            let center = enclosure.map(|i| i.midpoint().clamp(i.lo, i.hi));
            let value = self.evaluate(net, center.map(Interval::point), work)?;
            let local_derivative = self.derivative_on(net, enclosure, work)?;
            spend(work, 32)?;
            let local_inverse = approximate_inverse(local_derivative).unwrap_or(preconditioner);
            let local_map = FixedPoint::new(local_inverse, local_derivative, value, center);
            let delta = std::array::from_fn(|i| enclosure[i].sub(Interval::point(center[i])));
            let first_next = first_map.image(enclosure.map(|i| i.sub(Interval::point(0.5))));
            let local_next = local_map.image(delta);
            let (Some(u), Some(v)) = (first_next[0].intersect(local_next[0]), first_next[1].intersect(local_next[1])) else {
                return Ok(Root::Unknown);
            };
            let next = [u, v];
            let (Some(u), Some(v)) = (enclosure[0].intersect(next[0]), enclosure[1].intersect(next[1])) else { return Ok(Root::Unknown) };
            if [u.pair(), v.pair()] == enclosure.map(Interval::pair) {
                break;
            }
            enclosure = [u, v];
        }
        Ok(Root::Unique(enclosure))
    }

    fn split(&self, net: &[Control], axis: usize, work: &mut usize) -> Result<[Vec<Control>; 2]> {
        let count = self.size[axis];
        let lines = self.size[1 - axis];
        spend(work, 8 * lines * count * count)?;
        let mut left = allocate(net.len())?;
        let mut right = allocate(net.len())?;
        let [nu, _] = self.size;
        for line in 0..lines {
            let index = |i| if axis == 0 { line * nu + i } else { i * nu + line };
            let mut row = [[Interval::ZERO; 4]; MAX_DEGREE + 1];
            for (i, value) in row.iter_mut().enumerate().take(count) {
                *value = net[index(i)];
            }
            left[index(0)] = row[0];
            right[index(count - 1)] = row[count - 1];
            for level in 1..count {
                let n = count - level;
                blend(&mut row[..=n], Interval::point(0.5));
                left[index(level)] = row[0];
                right[index(n - 1)] = row[n - 1];
            }
        }
        Ok([left, right])
    }
}

fn approximate_inverse(derivative: Matrix) -> Option<[[f64; 2]; 2]> {
    let [[a, b], [c, d]] = derivative.map(|row| row.map(Interval::midpoint));
    let det = a * d - b * c;
    if !det.is_finite() || det == 0.0 {
        return None;
    }
    let inverse = [[d / det, -b / det], [-c / det, a / det]];
    inverse.iter().flatten().all(|x| x.is_finite()).then_some(inverse)
}

struct FixedPoint {
    offset: [Interval; 2],
    error: Matrix,
}

impl FixedPoint {
    fn new(b: [[f64; 2]; 2], derivative: Matrix, value: Control, center: [f64; 2]) -> Self {
        let offset = std::array::from_fn(|i| Interval::point(center[i]).sub(value[0].scale(b[i][0]).add(value[1].scale(b[i][1]))));
        let error = std::array::from_fn(|i| {
            std::array::from_fn(|j| Interval::point(f64::from(i == j)).sub(derivative[0][j].scale(b[i][0]).add(derivative[1][j].scale(b[i][1]))))
        });
        Self { offset, error }
    }

    fn image(&self, delta: [Interval; 2]) -> [Interval; 2] {
        std::array::from_fn(|i| self.offset[i].add(self.error[i][0].mul(delta[0]).add(self.error[i][1].mul(delta[1]))))
    }

    fn contracts(&self) -> bool {
        self.error.iter().all(|row| {
            let [a, b] = row.map(|i| i.lo.abs().max(i.hi.abs()));
            (a + b).next_up() < 1.0
        })
    }
}

enum Root {
    Empty,
    Unique([Interval; 2]),
    Unknown,
}

fn blend(row: &mut [Control], t: Interval) {
    let low = Interval::point(t.lo);
    let high = Interval::point(t.hi);
    let low_complement = Interval::point(1.0).sub(low);
    let high_complement = Interval::point(1.0).sub(high);
    for i in 1..row.len() {
        let [a, b] = [row[i - 1], row[i]];
        row[i - 1] = std::array::from_fn(|j| {
            let first = a[j].mul(low_complement).add(b[j].mul(low));
            if t.lo == t.hi {
                return first;
            }
            // For every pair of coefficients, the affine blend stays between
            // its values at the two parameter endpoints. Using the endpoints
            // also retains the dependency between t and its complement.
            first.hull(a[j].mul(high_complement).add(b[j].mul(high)))
        });
    }
}

pub(super) fn curve_value(controls: &[Control], t: Interval, work: &mut usize) -> Result<Control> {
    let endpoints = if t.lo == t.hi { 1 } else { 2 };
    spend(work, 16 * endpoints * controls.len() * controls.len().saturating_sub(1))?;
    let mut values = [[Interval::ZERO; 4]; MAX_DEGREE + 1];
    let row = &mut values[..controls.len()];
    row.copy_from_slice(controls);
    for n in (2..=row.len()).rev() {
        blend(&mut row[..n], t);
    }
    Ok(row.first().copied().unwrap_or([Interval::WHOLE; 4]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plane(f: impl Fn(f64, f64) -> [f64; 3]) -> [[f64; 4]; 4] {
        [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]].map(|[u, v]| {
            let [x, y, z] = f(u, v);
            [x, y, z, 1.0]
        })
    }

    fn run(degree: [usize; 2], controls: &[[f64; 4]], origin: [f64; 3], direction: [f64; 3], ray: [f64; 2]) -> RayProof {
        prove(degree, controls, origin, direction, ray, &mut 1_000_000).unwrap()
    }

    fn contains_hit(proof: RayProof, expected: [f64; 3]) {
        let RayProof::Unique { uv, depth } = proof else { panic!("expected unique hit, got {proof:?}") };
        for ([lo, hi], value) in uv.into_iter().chain([depth]).zip(expected) {
            assert!(lo <= value && value <= hi, "{lo:.16e}..{hi:.16e} excludes {value:.16e}");
            assert!(hi - lo < 1e-7, "enclosure too wide: {lo:.16e}..{hi:.16e}");
        }
    }

    #[test]
    fn affine_tile_certifies_depth_along_negative_nonunit_and_oblique_rays() {
        let p = plane(|u, v| [u, v, 2.0 + u + 2.0 * v]);
        contains_hit(run([1, 1], &p, [0.25, 0.75, 10.0], [0.0, 0.0, -2.0], [0.0, 10.0]), [0.25, 0.75, 3.125]);
        let flat = plane(|u, v| [u, v, 3.0]);
        contains_hit(run([1, 1], &flat, [-0.25, 0.5, 0.0], [0.5, 0.25, 3.0], [0.0, 2.0]), [0.25, 0.75, 1.0]);
        let side = plane(|u, v| [3.0, u, v]);
        contains_hit(run([1, 1], &side, [0.0, -0.25, 0.5], [3.0, 0.5, 0.25], [0.0, 2.0]), [0.25, 0.75, 1.0]);
    }

    #[test]
    fn rational_coefficients_keep_the_homogeneous_uv_and_depth() {
        // x = 3u/(1+2u), y = v and z = 2+2v.
        let p = [[0.0, 0.0, 2.0, 1.0], [3.0, 0.0, 6.0, 3.0], [0.0, 1.0, 4.0, 1.0], [3.0, 3.0, 12.0, 3.0]];
        contains_hit(run([1, 1], &p, [0.5, 0.75, 0.0], [0.0, 0.0, 1.0], [0.0, 5.0]), [0.25, 0.75, 3.5]);
    }

    #[test]
    fn curved_tensor_hit_is_certified_inside_a_narrow_depth_interval() {
        let quadratic = [0.0, 0.0, 1.0, 3.0];
        let gain = 0.125;
        let controls: [[f64; 4]; 16] = std::array::from_fn(|index| {
            let (i, j) = (index % 4, index / 4);
            let (x, y) = (i as f64, j as f64);
            [x + gain * quadratic[i] * y, y + gain * x * quadratic[j], 2.0 + x + 2.0 * y + x * y * 0.5, 1.0]
        });
        let (u, v) = (0.375, 0.625);
        let origin = [3.0 * u + 9.0 * gain * u * u * v, 3.0 * v + 9.0 * gain * u * v * v, 0.0];
        let depth = 2.0 + 3.0 * u + 6.0 * v + 4.5 * u * v;
        let direction = [0.0, 0.0, 1.0];
        contains_hit(run([3, 3], &controls, origin, direction, [depth - 0.01, depth + 0.01]), [u, v, depth]);
        assert_eq!(run([3, 3], &controls, origin, direction, [depth + 0.01, depth + 0.02]), RayProof::Miss);
    }

    fn fold(x: [f64; 3]) -> [[f64; 4]; 6] {
        std::array::from_fn(|i| [x[i % 3], (i / 3) as f64, (i % 3) as f64 * 0.5, 1.0])
    }

    #[test]
    fn folded_surface_uses_the_segment_to_qualify_one_root_or_neither() {
        // x = (u-3/16)(u-13/16), y = v and z = u.
        let k = 39.0 / 256.0;
        let p = fold([k, k - 0.5, k]);
        let (origin, dir) = ([0.0, 0.375, 0.0], [0.0, 0.0, 1.0]);
        contains_hit(run([2, 1], &p, origin, dir, [0.0, 0.5]), [0.1875, 0.375, 0.1875]);
        contains_hit(run([2, 1], &p, origin, dir, [0.5, 1.0]), [0.8125, 0.375, 0.8125]);
        assert_eq!(run([2, 1], &p, origin, dir, [0.25, 0.75]), RayProof::Miss);
        assert_eq!(run([2, 1], &p, origin, dir, [0.0, 1.0]), RayProof::Unresolved);
    }

    #[test]
    fn tangent_contact_cannot_become_a_miss_or_a_transverse_hit() {
        // x = (u-0.5)^2, a double root at a subdivision seam.
        let p = fold([0.25, -0.25, 0.25]);
        let dir = [0.0, 0.0, 1.0];
        assert_eq!(run([2, 1], &p, [0.0, 0.375, 0.0], dir, [0.0, 1.0]), RayProof::Unresolved);
        assert_eq!(run([2, 1], &p, [-0.0001, 0.375, 0.0], dir, [0.0, 1.0]), RayProof::Miss);
        assert_eq!(run([2, 1], &p, [0.0001, 0.375, 0.0], dir, [0.0, 1.0]), RayProof::Unresolved);
    }

    #[test]
    fn adjoining_tile_boundary_is_neither_a_duplicate_hit_nor_an_empty_space_click() {
        let left = plane(|u, v| [u - 1.0, v, 1.0]);
        let right = plane(|u, v| [u, v, 1.0]);
        let (origin, dir) = ([0.0, 0.375, 0.0], [0.0, 0.0, 1.0]);
        assert_eq!(run([1, 1], &left, origin, dir, [0.0, 2.0]), RayProof::Unresolved);
        assert_eq!(run([1, 1], &right, origin, dir, [0.0, 2.0]), RayProof::Unresolved);
        contains_hit(run([1, 1], &right, [0.5, 0.5, 0.0], dir, [0.0, 2.0]), [0.5, 0.5, 1.0]);
    }

    #[test]
    fn depth_endpoint_and_line_coincidence_are_conservative() {
        let p = plane(|u, v| [u, v, 1.0]);
        let dir = [0.0, 0.0, 1.0];
        assert_eq!(run([1, 1], &p, [0.25, 0.375, 0.0], dir, [1.01, 2.0]), RayProof::Miss);
        assert_eq!(run([1, 1], &p, [0.25, 0.375, 0.0], dir, [1.0, 2.0]), RayProof::Unresolved);
        assert_eq!(run([1, 1], &p, [0.0, 0.375, 1.0], [1.0, 0.0, 0.0], [0.0, 2.0]), RayProof::Unresolved);
    }

    #[test]
    fn rounded_split_stores_both_sides_of_an_unrepresentable_midpoint() {
        let [a, b] = [1.0_f64, 1.0_f64.next_up()].map(Interval::point);
        let net = [[a; 4], [b; 4], [a; 4], [b; 4]];
        let search = Search { size: [2, 2], segment: Interval::UNIT };
        let [left, right] = search.split(&net, 0, &mut 1_000).unwrap();
        for mid in [left[1], left[3], right[0], right[2]].iter().flatten() {
            assert!(mid.lo <= a.lo && b.hi <= mid.hi);
        }
    }

    #[test]
    fn malformed_input_overflow_and_budget_exhaustion_never_produce_false_misses() {
        let p = plane(|u, v| [u, v, 1.0]);
        let (origin, dir, ray) = ([0.25, 0.375, 0.0], [0.0, 0.0, 1.0], [0.0, 2.0]);
        assert_eq!(prove([1, 1], &p, origin, dir, ray, &mut 100), Err(ProofError::WorkExhausted));
        assert_eq!(prove([1, 1], &p, origin, [0.0; 3], ray, &mut 1_000), Err(ProofError::InvalidRay));
        assert_eq!(prove([usize::MAX, 1], &p, origin, dir, ray, &mut 1_000), Err(ProofError::InvalidTile));
        assert_eq!(prove([1, 1], &p[..3], origin, dir, ray, &mut 1_000), Err(ProofError::InvalidTile));
        let mut invalid = p;
        invalid[2][3] = 0.0;
        assert_eq!(prove([1, 1], &invalid, origin, dir, ray, &mut 1_000), Err(ProofError::InvalidTile));
        let large = plane(|u, v| [(u - 0.5) * f64::MAX, v, 1.0]);
        assert_eq!(run([1, 1], &large, [0.0, 0.375, 0.0], [0.0, 0.0, 4.0], ray), RayProof::Unresolved);
    }
}
