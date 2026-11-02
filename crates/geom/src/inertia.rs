//! Inertia tensors (#8095): built from how a body's volume spreads about its centre of mass,
//! taken about another point (parallel axes), and their principal moments and axes.

use crate::{MassProperties, Vec3};

/// A symmetric 3 × 3 tensor, row by row.
pub type Tensor3 = [[f64; 3]; 3];

/// Jacobi sweeps before [`principal`] gives up; a 3 × 3 tensor settles in well under ten.
const MAX_SWEEPS: usize = 50;

const IDENTITY3: Tensor3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

impl MassProperties {
    /// Second moments about the centroid per unit volume, ∫ (x − c)(x − c)ᵀ dV / V. Unlike the
    /// inertia it follows any linear map L of the body as L G Lᵀ (a scale or a mirror too), and
    /// [`from_gyration`] turns it into the inertia of a mass spread that way.
    pub fn gyration(&self) -> Option<Tensor3> {
        // I = tr(Σ) E − Σ, so tr(I) = 2 tr(Σ) and Σ = tr(I)/2 E − I.
        let i = &self.inertia;
        let half = (i[0][0] + i[1][1] + i[2][2]) / 2.0;
        let mut g = [[0.0f64; 3]; 3];
        for (row_out, (a, row)) in g.iter_mut().zip(i.iter().enumerate()) {
            for (cell, (b, x)) in row_out.iter_mut().zip(row.iter().enumerate()) {
                *cell = (if a == b { half } else { 0.0 } - x) / self.volume;
            }
        }
        g.iter().flatten().all(|x| x.is_finite()).then_some(g)
    }
}

/// The inertia tensor of `mass` spread as `gyration` about its centre of mass: m (tr(G) E − G).
pub fn from_gyration(gyration: &Tensor3, mass: f64) -> Tensor3 {
    let trace = gyration[0][0] + gyration[1][1] + gyration[2][2];
    let mut out = [[0.0f64; 3]; 3];
    for (i, (row_out, row)) in out.iter_mut().zip(gyration).enumerate() {
        for (j, (cell, x)) in row_out.iter_mut().zip(row).enumerate() {
            *cell = mass * (if i == j { trace } else { 0.0 } - x);
        }
    }
    out
}

/// `g` of a body moved by the linear map `l` (row by row): L G Lᵀ. A gyration follows any
/// linear map; an inertia tensor follows a rotation this way.
pub fn transformed(g: &Tensor3, l: &Tensor3) -> Tensor3 {
    mul(&mul(l, g), &transpose(l))
}

/// `inertia` of `mass` about its centre of mass, taken about a point `d` from that centre
/// instead (parallel-axis theorem): I + m (|d|² E − d dᵀ).
pub fn shifted(inertia: &Tensor3, mass: f64, d: Vec3) -> Tensor3 {
    let dv = [d.x, d.y, d.z];
    let d2 = d.dot(d);
    let mut out = *inertia;
    for (i, (row, di)) in out.iter_mut().zip(dv).enumerate() {
        for (j, (cell, dj)) in row.iter_mut().zip(dv).enumerate() {
            *cell += mass * (if i == j { d2 } else { 0.0 } - di * dj);
        }
    }
    out
}

/// Principal moments of `inertia`, smallest first, and the unit axis each is about: the axes
/// with no products of inertia between them. The first two point along the world axis each is
/// nearest to, and the third is the first × the second (right-handed). Moments within `equal`
/// (a share of the largest) of each other count as equal, as a round body's do though its
/// tessellation tells them apart: their axes are free in the plane (or space) they share, and
/// the world axes nearest it are given. `None` for a tensor with non-finite entries.
pub fn principal(inertia: &Tensor3, equal: f64) -> Option<([f64; 3], [Vec3; 3])> {
    if !inertia.iter().flatten().all(|x| x.is_finite()) {
        return None;
    }
    // The symmetric part: products of inertia summed with round-off can differ in the last bit.
    let mut a = *inertia;
    for (i, row) in a.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = 0.5 * (inertia[i][j] + inertia[j][i]);
        }
    }
    let scale = a.iter().flatten().fold(0.0f64, |m, x| m.max(x.abs()));
    let mut v = IDENTITY3;
    // Cyclic Jacobi: each rotation zeroes one product of inertia; the sweep repeats until all are
    // round-off small against the largest entry.
    for _ in 0..MAX_SWEEPS {
        if a[0][1].abs() + a[0][2].abs() + a[1][2].abs() <= scale * 1e-15 {
            break;
        }
        for (p, q) in [(0, 1), (0, 2), (1, 2)] {
            let apq = a[p][q];
            if apq == 0.0 {
                continue;
            }
            let theta = (a[q][q] - a[p][p]) / (2.0 * apq);
            let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            let mut j = IDENTITY3;
            j[p][p] = c;
            j[q][q] = c;
            j[p][q] = s;
            j[q][p] = -s;
            a = mul(&mul(&transpose(&j), &a), &j);
            v = mul(&v, &j);
        }
    }
    let mut order = [0usize, 1, 2];
    order.sort_by(|&x, &y| a[x][x].total_cmp(&a[y][y]));
    let moments = order.map(|k| a[k][k]);
    let [e0, e1, _] = order.map(|k| Vec3::new(v[0][k], v[1][k], v[2][k]));
    let largest = moments.iter().fold(0.0f64, |m, x| m.max(x.abs()));
    let same = |x: f64, y: f64| (y - x).abs() <= equal * largest;
    let (low, high) = (same(moments[0], moments[1]), same(moments[1], moments[2]));
    let (first, second) = if low && high {
        (Vec3::X, Vec3::Y)
    } else if low {
        // The first two share a plane: the world axis nearest it, then its perpendicular there.
        let normal = e0.cross(e1).normalized()?;
        let first = nearest_in_plane(normal)?;
        (first, normal.cross(first).normalized()?)
    } else if high {
        (e0, nearest_in_plane(e0)?)
    } else {
        (e0, e1)
    };
    let (first, second) = (forward(first), forward(second));
    let axes = [first, second, first.cross(second)];
    (moments.iter().all(|x| x.is_finite()) && axes.iter().all(|e| e.is_finite())).then_some((moments, axes))
}

/// `e` turned to point along the world axis it is nearest to, so the same body gives the same
/// axes every time.
fn forward(e: Vec3) -> Vec3 {
    let lead = [e.x, e.y, e.z].into_iter().fold(0.0f64, |m, x| if x.abs() > m.abs() { x } else { m });
    if lead < 0.0 { e * -1.0 } else { e }
}

/// The unit direction in the plane through the origin with unit `normal` nearest a world axis
/// (X first when two are as near).
fn nearest_in_plane(normal: Vec3) -> Option<Vec3> {
    let along = |w: Vec3| w - normal * normal.dot(w);
    let best = [Vec3::X, Vec3::Y, Vec3::Z].into_iter().map(along).fold(Vec3::ZERO, |b, p| if p.len2() > b.len2() * (1.0 + 1e-9) { p } else { b });
    best.normalized()
}

fn mul(a: &Tensor3, b: &Tensor3) -> Tensor3 {
    let mut out = [[0.0f64; 3]; 3];
    for (row_out, row) in out.iter_mut().zip(a) {
        for (j, cell) in row_out.iter_mut().enumerate() {
            *cell = row.iter().zip(b).map(|(x, b_row)| x * b_row[j]).sum();
        }
    }
    out
}

fn transpose(a: &Tensor3) -> Tensor3 {
    [[a[0][0], a[1][0], a[2][0]], [a[0][1], a[1][1], a[2][1]], [a[0][2], a[1][2], a[2][2]]]
}
