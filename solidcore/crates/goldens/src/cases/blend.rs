//! Input checks shared by the `blend.fillet_geom_p*` generators: ports of the reference
//! wrapper's input contract for the flat pseudo-B-rep programs those runners encode (layout
//! in that module's docs): clamped B-spline curves and surfaces within the size bounds,
//! enum codes in their upstream ranges, indices inside the block and the magnitude bounds
//! of an op. The generators redraw until their contract holds, so their cases stay on the
//! working path; hostile data is covered by the fixture and fuzz files.

use solidcore::nurbs::math::{NOW, NOZ};

/// Largest control point index.
pub const MAX_INDEX: usize = 64;
/// Largest NURB degree.
pub const MAX_DEGREE: usize = 9;
/// Largest number of objects in any list (vertices, edges, geometries, candidates, ...).
pub const MAX_LIST: usize = 64;
/// Largest magnitude of a value of an op reaching `sm_ClassifyFacePoint` (control points
/// as `Pw / w`).
pub const MAX_MAGNITUDE: f64 = 1e150;
/// Largest magnitude of a value of an op running a global solver over a surface.
pub const MAX_SOLVER_MAGNITUDE: f64 = 1e307;

/// A sequential reader over a flat program with the op's magnitude bound. Every read
/// returns `None` once the program breaks the contract.
pub struct Reader<'a> {
    input: &'a [f64],
    at: usize,
    /// The magnitude bound of this op's values.
    pub bound: f64,
}

impl<'a> Reader<'a> {
    /// A reader over `input` with the magnitude bound `bound`.
    #[must_use]
    pub fn new(input: &'a [f64], bound: f64) -> Self {
        Self {
            input,
            at: 0,
            bound,
        }
    }

    /// The next value.
    pub fn num(&mut self) -> Option<f64> {
        let v = *self.input.get(self.at)?;
        self.at += 1;
        Some(v)
    }

    /// An integer in `0..=max`.
    pub fn index(&mut self, max: usize) -> Option<usize> {
        let v = self.num()?;
        (v.fract() == 0.0 && v >= 0.0 && v <= max as f64).then_some(v as usize)
    }

    /// `0` or `1`.
    pub fn flag(&mut self) -> Option<bool> {
        Some(self.index(1)? == 1)
    }

    /// A finite value within the bound.
    pub fn finite(&mut self) -> Option<f64> {
        let v = self.num()?;
        (v.is_finite() && v.abs() <= self.bound).then_some(v)
    }

    /// Exactly `marker` (`NL_NOZ` / `NL_NOW`), or a [`finite`](Self::finite) value.
    pub fn marker_or_finite(&mut self, marker: f64) -> Option<f64> {
        let v = self.num()?;
        (v == marker || (v.is_finite() && v.abs() <= self.bound)).then_some(v)
    }

    /// `k` [`finite`](Self::finite) values.
    pub fn finites(&mut self, k: usize) -> Option<Vec<f64>> {
        (0..k).map(|_| self.finite()).collect()
    }

    /// An integer in `-1..len` as `None` / `Some`; the outer `None` breaks the contract.
    pub fn opt_index(&mut self, len: usize) -> Option<Option<usize>> {
        let v = self.num()?;
        if v == -1.0 {
            return Some(None);
        }
        (v.fract() == 0.0 && v >= 0.0 && v < len as f64).then_some(Some(v as usize))
    }

    /// An index in `0..len` (`-1` breaks the contract).
    pub fn some_index(&mut self, len: usize) -> Option<usize> {
        self.opt_index(len)?
    }

    /// Whether the whole program was read.
    #[must_use]
    pub fn done(&self) -> bool {
        self.at == self.input.len()
    }
}

/// A clamped knot vector of degree `p`: non-decreasing, end multiplicity exactly `p + 1`,
/// interior multiplicity at most `p`.
#[must_use]
pub fn knots_ok(knots: &[f64], p: usize) -> bool {
    let Some(m) = knots.len().checked_sub(1) else {
        return false;
    };
    if knots.windows(2).any(|w| w.first() > w.last()) || m < 2 * p + 1 {
        return false;
    }
    let (Some(&a), Some(&b)) = (knots.first(), knots.last()) else {
        return false;
    };
    let (Some(head), Some(tail), Some(inner)) =
        (knots.get(..=p), knots.get(m - p..), knots.get(p + 1..m - p))
    else {
        return false;
    };
    if a >= b || head.iter().any(|&k| k != a) || tail.iter().any(|&k| k != b) {
        return false;
    }
    if inner.iter().any(|&k| k == a || k == b) {
        return false;
    }
    let mut run = 1;
    for w in inner.windows(2) {
        run = if w.first() == w.last() { run + 1 } else { 1 };
        if run > p {
            return false;
        }
    }
    true
}

/// Control points of one rationality with positive weights, 3D when `dim == 3`.
fn points_ok(pts: &[[f64; 4]], dim: usize) -> bool {
    let Some(&[_, _, _, w0]) = pts.first() else {
        return false;
    };
    pts.iter().all(|&[_, _, z, w]| {
        !(dim == 3 && z == NOZ) && (w == NOW) == (w0 == NOW) && (w == NOW || w > 0.0)
    })
}

/// `k` control points `x y z w`, the Euclidean point within the bound.
fn points(rd: &mut Reader<'_>, k: usize) -> Option<Vec<[f64; 4]>> {
    (0..k)
        .map(|_| {
            let pt = [
                rd.finite()?,
                rd.finite()?,
                rd.marker_or_finite(NOZ)?,
                rd.marker_or_finite(NOW)?,
            ];
            let w = pt[3];
            if rd.bound.is_finite() && w != NOW {
                let oversized = |c: f64| {
                    let e = (c / w).abs();
                    e.is_nan() || e > rd.bound
                };
                if oversized(pt[0]) || oversized(pt[1]) || (pt[2] != NOZ && oversized(pt[2])) {
                    return None;
                }
            }
            Some(pt)
        })
        .collect()
}

/// A curve `dim n p m cpts knots` of dimension `want_dim`.
pub fn curve(rd: &mut Reader<'_>, want_dim: usize) -> Option<()> {
    let dim = rd.index(3)?;
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if dim != want_dim || p == 0 || p > n || m != n + p + 1 {
        return None;
    }
    let pts = points(rd, n + 1)?;
    let knots = rd.finites(m + 1)?;
    (points_ok(&pts, dim) && knots_ok(&knots, p)).then_some(())
}

/// A surface `n m p q r s cpts u v`.
pub fn surface(rd: &mut Reader<'_>) -> Option<()> {
    let n = rd.index(MAX_INDEX)?;
    let m = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let q = rd.index(MAX_DEGREE)?;
    let r = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    let s = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || q == 0 || q > m || r != n + p + 1 || s != m + q + 1 {
        return None;
    }
    let pts = points(rd, (n + 1) * (m + 1))?;
    let u = rd.finites(r + 1)?;
    let v = rd.finites(s + 1)?;
    (points_ok(&pts, 3) && knots_ok(&u, p) && knots_ok(&v, q)).then_some(())
}

/// A surface `kind [data]`: `0` a B-spline surface, `1` a plane (`O X Y scale domain
/// make_nurb`), `2` a surface of revolution (`a b axis_point axis_dir swap`).
pub fn any_surface(rd: &mut Reader<'_>, kind: usize) -> Option<()> {
    match kind {
        0 => surface(rd),
        1 => {
            rd.finites(15)?;
            rd.flag().map(|_| ())
        }
        _ => {
            rd.finites(12)?;
            rd.flag().map(|_| ())
        }
    }
}

/// What the block declares, for checking the op's arguments.
pub struct Block {
    /// Target wire edges.
    pub nt: usize,
    /// Target faces.
    pub nf: usize,
    /// Fillet B-rep edges.
    pub nfb: usize,
    /// Per vertex: the classification object kind (`pc_kind`).
    pub pc_kinds: Vec<usize>,
    /// Per edge: whether it has a curve.
    pub edge_curves: Vec<bool>,
    /// Per edge: its two vertices.
    pub edge_vertices: Vec<[usize; 2]>,
    /// Per geometry: whether it has a surface.
    pub geom_surfaces: Vec<bool>,
    /// Per geometry: whether both rails are set.
    pub geom_rails: Vec<bool>,
}

impl Block {
    /// Vertices.
    #[must_use]
    pub fn nv(&self) -> usize {
        self.pc_kinds.len()
    }
    /// Edges.
    #[must_use]
    pub fn ne(&self) -> usize {
        self.edge_curves.len()
    }
    /// Geometries.
    #[must_use]
    pub fn ng(&self) -> usize {
        self.geom_surfaces.len()
    }
}

/// The pseudo-B-rep block.
pub fn block(rd: &mut Reader<'_>) -> Option<Block> {
    let nt = rd.index(MAX_LIST)?;
    for _ in 0..nt {
        curve(rd, 3)?;
    }
    let nf = rd.index(MAX_LIST)?;
    for _ in 0..nf {
        surface(rd)?;
    }
    let nfb = rd.index(MAX_LIST)?;
    for _ in 0..nfb {
        curve(rd, 3)?;
        if rd.finite()? < 0.0 {
            return None;
        }
    }

    let nv = rd.index(MAX_LIST)?;
    let mut pc_kinds = Vec::with_capacity(nv);
    for _ in 0..nv {
        rd.finites(3)?;
        rd.index(13)?; // SmFilletVertexType
        rd.index(12)?; // SmFilStatus
        rd.opt_index(MAX_LIST)?; // corner tag
        let pc_class = rd.index(110)?; // SM_PC_UNKNOWN (100) ..= SM_PC_NOTHING (110)
        if pc_class < 100 {
            return None;
        }
        let pc_kind = rd.index(5)?;
        let pc_index = rd.opt_index(MAX_LIST)?;
        let pc_sub = rd.index(3)?;
        match (pc_kind, pc_index) {
            (0, _) => {}
            (1 | 2 | 4, Some(i)) if i < nt && pc_sub <= 1 => {}
            (3, Some(i)) if i < nf && pc_sub == 0 => {}
            (5, Some(i)) if i < nf => {}
            _ => return None,
        }
        rd.opt_index(nf)?; // pc_face
        rd.finites(3)?; // t_param u v
        pc_kinds.push(pc_kind);
    }

    let ng = rd.index(MAX_LIST)?;
    let mut geom_surfaces = Vec::with_capacity(ng);
    for _ in 0..ng {
        rd.index(6)?; // SmFilletGeomType
        rd.index(12)?; // SmFilStatus
        if rd.flag()? {
            curve(rd, 3)?;
        }
        let has_surface = rd.flag()?;
        if has_surface {
            surface(rd)?;
        }
        geom_surfaces.push(has_surface);
    }

    let ne = rd.index(MAX_LIST)?;
    let mut edge_curves = Vec::with_capacity(ne);
    let mut edge_vertices = Vec::with_capacity(ne);
    for _ in 0..ne {
        let vs = rd.index(MAX_LIST)?;
        let ve = rd.index(MAX_LIST)?;
        if vs >= nv || ve >= nv {
            return None;
        }
        rd.index(14)?; // SmFilletEdgeType
        rd.index(12)?; // SmFilStatus
        rd.opt_index(MAX_LIST)?; // corner tag
        let has_curve = rd.flag()?;
        if has_curve {
            curve(rd, 3)?;
        }
        if rd.flag()? {
            rd.finites(2)?; // any interval, as SetInterval
        }
        rd.opt_index(nfb)?;
        rd.opt_index(nfb)?;
        for _ in 0..2 {
            if rd.flag()? {
                curve(rd, 2)?;
            }
            rd.opt_index(ng)?;
        }
        rd.finites(4)?;
        edge_curves.push(has_curve);
        edge_vertices.push([vs, ve]);
    }

    let mut geom_rails = Vec::with_capacity(ng);
    for _ in 0..ng {
        let r0 = rd.opt_index(ne)?;
        let r1 = rd.opt_index(ne)?;
        geom_rails.push(r0.is_some() && r1.is_some());
        for _ in 0..2 {
            let n = rd.index(MAX_LIST)?;
            for _ in 0..n {
                rd.some_index(ne)?;
            }
        }
        let ns = rd.index(MAX_LIST)?;
        for _ in 0..ns {
            rd.some_index(ne)?;
            rd.index(1)?;
        }
    }

    for _ in 0..nv {
        rd.opt_index(nv)?;
        rd.opt_index(nv)?;
    }

    Some(Block {
        nt,
        nf,
        nfb,
        pc_kinds,
        edge_curves,
        edge_vertices,
        geom_surfaces,
        geom_rails,
    })
}

/// The block followed by the solvers (`ns`, per solver `approx ang tang`, per geometry
/// its solver or `-1`) of `blend.fillet_geom_p2` / `p4` / `p5`.
pub fn block_and_solvers(rd: &mut Reader<'_>) -> Option<Block> {
    let b = block(rd)?;
    let ns = rd.index(MAX_LIST)?;
    for _ in 0..ns {
        rd.finites(3)?;
    }
    for _ in 0..b.ng() {
        rd.opt_index(ns)?;
    }
    Some(b)
}

/// The input contract of the fillet solver wrappers (`blend.fillet_solver_p4`,
/// `blend.fillet_intersector`): values finite and at most [`solver::MAX_COORD`] in
/// magnitude, clamped B-spline surfaces of degree 1..=8 with 2..=17 control points a side
/// and one rationality, intersection points of [`solver::POINT_LEN`] values with their type
/// codes in range.
pub mod solver {
    use solidcore::nurbs::math::NOW;

    /// Largest magnitude of a value.
    pub const MAX_COORD: f64 = 1e9;
    /// Values of an intersection point.
    pub const POINT_LEN: usize = 80;

    /// Whether `x` is finite and at most [`MAX_COORD`] in magnitude.
    #[must_use]
    pub fn num(x: f64) -> bool {
        x.is_finite() && x.abs() <= MAX_COORD
    }

    /// Whether every value of `xs` is a [`num`].
    #[must_use]
    pub fn nums(xs: &[f64]) -> bool {
        xs.iter().all(|&x| num(x))
    }

    /// `ncp + p + 1` clamped, non-decreasing knots with interior multiplicity at most `p`.
    #[must_use]
    pub fn knots(knots: &[f64], ncp: usize, p: usize) -> bool {
        if knots.len() != ncp + p + 1 || knots.windows(2).any(|w| w.first() > w.last()) {
            return false;
        }
        let (Some(&k0), Some(&km)) = (knots.first(), knots.last()) else {
            return false;
        };
        if km <= k0
            || knots.iter().take(p + 1).any(|&k| k != k0)
            || knots.iter().rev().take(p + 1).any(|&k| k != km)
        {
            return false;
        }
        let mut run = 0usize;
        let mut last = f64::NAN;
        for &k in knots.iter().filter(|&&k| k != k0 && k != km) {
            run = if k == last { run + 1 } else { 1 };
            last = k;
            if run > p {
                return false;
            }
        }
        true
    }

    /// Weights all `NOW`, or all in `(0, MAX_COORD]`.
    fn weights(net: &[[f64; 4]]) -> bool {
        let Some(first) = net.first() else {
            return false;
        };
        let rational = first[3] != NOW;
        net.iter().all(|c| {
            if rational {
                c[3] > 0.0 && c[3] <= MAX_COORD
            } else {
                c[3] == NOW
            }
        })
    }

    /// A B-spline surface of degrees `p`, `q` on the net `net` (rows in u, `[x, y, z, w]`
    /// with `w` = `NOW` when non-rational) and knots `u`, `v`.
    #[must_use]
    pub fn bspline(p: usize, q: usize, net: &[Vec<[f64; 4]>], u: &[f64], v: &[f64]) -> bool {
        let (nu, nv) = (net.len(), net.first().map_or(0, Vec::len));
        let flat: Vec<[f64; 4]> = net.iter().flatten().copied().collect();
        (1..=8).contains(&p)
            && (1..=8).contains(&q)
            && (2..=17).contains(&nu)
            && (2..=17).contains(&nv)
            && p < nu
            && q < nv
            && net.iter().all(|r| r.len() == nv)
            && knots(u, nu, p)
            && knots(v, nv, q)
            && weights(&flat)
            && flat.iter().all(|c| nums(&c[..3]))
            && nums(u)
            && nums(v)
    }

    /// An intersection point of [`POINT_LEN`] [`num`]s, its type code in `0..=7` and its
    /// four extent point types in `0..=5`.
    #[must_use]
    pub fn point(p: &[f64]) -> bool {
        let code = |x: f64, hi: f64| (0.0..=hi).contains(&x) && x.fract() == 0.0;
        p.len() == POINT_LEN
            && p.iter()
                .take(5)
                .enumerate()
                .all(|(k, &t)| code(t, if k == 0 { 7.0 } else { 5.0 }))
            && nums(p)
    }
}
