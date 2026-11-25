//! Input checks shared by several generators' ports of the C++ reference wrappers' input
//! contracts: B-spline, Hermite and line curve data as upstream's constructors take them
//! (the wrappers' `bspline` / `line` checks), clamped knots, transforms and bounded planes.
//! The generators redraw until their contract holds, so their cases stay on the working
//! path; hostile data is covered by the fixture and fuzz files.

use solidcore::nurbs::math::{NOW, NOZ};

/// The size bounds of a unit's curve blocks.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// The largest last control point index `n`.
    pub max_index: usize,
    /// The largest degree `p`.
    pub max_degree: usize,
    /// The largest magnitude of a coordinate, knot or scalar.
    pub max_coord: f64,
}

/// Whether `v` is finite and at most `max` in magnitude.
#[must_use]
pub fn finite(v: f64, max: f64) -> bool {
    v.is_finite() && v.abs() <= max
}

/// Whether every coordinate of `v` is [`finite`].
#[must_use]
pub fn vec3(v: &[f64; 3], max: f64) -> bool {
    v.iter().all(|&c| finite(c, max))
}

/// The squared length of `v`.
#[must_use]
pub fn len_sq(v: [f64; 3]) -> f64 {
    v.iter().map(|c| c * c).sum()
}

/// Whether `v` is a [`vec3`] of non-zero length.
#[must_use]
pub fn nonzero3(v: &[f64; 3], max: f64) -> bool {
    vec3(v, max) && len_sq(*v) >= 1e-12
}

/// Whether `x` and `y` are non-zero and far from parallel (`|x × y|² ≥ 1e-6 |x|² |y|²`).
#[must_use]
pub fn frame(x: &[f64; 3], y: &[f64; 3], max: f64) -> bool {
    let cross = [
        x[1] * y[2] - x[2] * y[1],
        x[2] * y[0] - x[0] * y[2],
        x[0] * y[1] - x[1] * y[0],
    ];
    nonzero3(x, max) && nonzero3(y, max) && len_sq(cross) >= 1e-6 * len_sq(*x) * len_sq(*y)
}

/// The Euclidean point of a stored control point (`z = NOZ` is 2D, `w = NOW` polynomial).
#[must_use]
pub fn euclid([x, y, z, w]: [f64; 4]) -> [f64; 3] {
    let z = if z == NOZ { 0.0 } else { z };
    if w == NOW {
        [x, y, z]
    } else {
        [x / w, y / w, z / w]
    }
}

/// What a B-spline block that keeps the contract tells the rest of a case.
#[derive(Debug, Clone)]
pub struct BSpline {
    /// The first knot.
    pub a: f64,
    /// The last knot.
    pub b: f64,
    /// The distinct knots, in order.
    pub knots: Vec<f64>,
    /// The Euclidean first control point.
    pub start: [f64; 3],
    /// The Euclidean last control point.
    pub end: [f64; 3],
}

/// A B-spline of control points `cpts` (stored `[x, y, z, w]`), degree `p` and knots `u`, if
/// it keeps the contract: `1 ≤ p ≤ n` within the limits and `n + p + 2` knots, one
/// dimension and rationality, finite control points with weights above `1e-3` and
/// Euclidean points within range, clamped non-decreasing knots within range over a
/// parameter range of at least `1e-6` relative, and no interior knot of multiplicity above
/// `p`.
#[must_use]
pub fn bspline(cpts: &[[f64; 4]], p: usize, u: &[f64], lim: &Limits) -> Option<BSpline> {
    let n = cpts.len().checked_sub(1)?;
    let m = u.len().checked_sub(1)?;
    if n > lim.max_index || p > lim.max_degree || m > lim.max_index + lim.max_degree + 1 {
        return None;
    }
    if p == 0 || p > n || m != n + p + 1 {
        return None;
    }
    let [_, _, z0, w0] = *cpts.first()?;
    for &[x, y, z, w] in cpts {
        if (z == NOZ) != (z0 == NOZ) || (w == NOW) != (w0 == NOW) {
            return None;
        }
        if !x.is_finite()
            || !y.is_finite()
            || (z != NOZ && !z.is_finite())
            || (w != NOW && !w.is_finite())
        {
            return None;
        }
        if w != NOW && w <= 1e-3 {
            return None;
        }
        if !euclid([x, y, z, w])
            .iter()
            .all(|&c| finite(c, lim.max_coord))
        {
            return None;
        }
    }
    if !u.iter().all(|&k| finite(k, lim.max_coord)) {
        return None;
    }
    let (a, b) = clamped(u, p)?;
    let mut knots: Vec<f64> = Vec::new();
    for &k in u {
        if knots.last() != Some(&k) {
            knots.push(k);
        }
    }
    Some(BSpline {
        a,
        b,
        knots,
        start: euclid(*cpts.first()?),
        end: euclid(*cpts.last()?),
    })
}

/// The parameter range `(a, b)` of knots `u` of degree `p`, if they are non-decreasing and
/// clamped (`p + 1` equal knots at each end and no more), span at least `1e-6` relative, and
/// have no interior knot of multiplicity above `p`.
#[must_use]
pub fn clamped(u: &[f64], p: usize) -> Option<(f64, f64)> {
    let m = u.len().checked_sub(1)?;
    if u.windows(2).any(|w| w.first() > w.last()) || m < 2 * p + 1 {
        return None;
    }
    let (&a, &b) = (u.first()?, u.last()?);
    let head = u.get(..=p)?;
    let tail = u.get(m - p..)?;
    let inner = u.get(p + 1..m - p)?;
    if a >= b || head.iter().any(|&k| k != a) || tail.iter().any(|&k| k != b) {
        return None;
    }
    if b - a < 1e-6 * a.abs().max(b.abs()).max(1.0) {
        return None;
    }
    if inner.iter().any(|&k| k == a || k == b) {
        return None;
    }
    let mut run = 1;
    for w in inner.windows(2) {
        run = if w.first() == w.last() { run + 1 } else { 1 };
        if run > p {
            return None;
        }
    }
    Some((a, b))
}

/// The domain `[min, max]` of a line through `p` along `v` over `domain` with `scale`, if it
/// keeps the contract: finite data, a non-zero direction, a domain of at least `1e-6` and a
/// scale above `1e-6`.
#[must_use]
pub fn line(
    p: &[f64; 3],
    v: &[f64; 3],
    domain: [f64; 2],
    scale: f64,
    max: f64,
) -> Option<(f64, f64)> {
    let [d0, d1] = domain;
    let ok = vec3(p, max)
        && nonzero3(v, max)
        && finite(d0, max)
        && finite(d1, max)
        && (d1 - d0).abs() >= 1e-6
        && finite(scale, max)
        && scale > 1e-6;
    ok.then(|| (d0.min(d1), d0.max(d1)))
}

/// Whether the four vectors of a Hermite segment are [`vec3`]s.
#[must_use]
pub fn hermite(vs: [&[f64; 3]; 4], max: f64) -> bool {
    vs.iter().all(|v| vec3(v, max))
}

/// Whether one of at most `max_transforms` transforms applied to a curve keeps the
/// contract: an origin within `max / max_transforms`, two non-parallel axes ([`frame`]) and,
/// when given, scales in `[1e-3, 1]`.
#[must_use]
pub fn transform(
    o: &[f64; 3],
    x: &[f64; 3],
    y: &[f64; 3],
    scale: Option<[f64; 3]>,
    max: f64,
    max_transforms: u64,
) -> bool {
    let s = scale.unwrap_or([1.0; 3]);
    vec3(o, max)
        && o.iter().all(|c| c.abs() <= max / max_transforms as f64)
        && frame(x, y, max)
        && vec3(&s, max)
        && (scale.is_none() || s.iter().all(|c| (1e-3..=1.0).contains(&c.abs())))
}

/// Whether a bounded plane `SmPlane(o, x, y, scale, dom)` keeps the contract: finite data,
/// axes of squared length in `[1e-6, 1e6]` far from parallel, scales in `[1e-3, 1e3]`, and
/// an ordered domain of sides at least `1e-6` within `1e4`.
#[must_use]
pub fn plane(
    o: &[f64; 3],
    x: &[f64; 3],
    y: &[f64; 3],
    scale: [f64; 2],
    dom: [f64; 4],
    max: f64,
) -> bool {
    let [lx, ly] = [len_sq(*x), len_sq(*y)];
    vec3(o, max)
        && vec3(x, max)
        && vec3(y, max)
        && (1e-6..=1e6).contains(&lx)
        && (1e-6..=1e6).contains(&ly)
        && frame(x, y, max)
        && scale
            .iter()
            .all(|&s| finite(s, max) && (1e-3..=1e3).contains(&s))
        && dom.iter().all(|&v| finite(v, max) && v.abs() <= 1e4)
        && dom[2] - dom[0] >= 1e-6
        && dom[3] - dom[1] >= 1e-6
}

/// Whether control points stored `[x, y, z, w]` are 3D (`z` not `NOZ`), of one rationality
/// with weights in `[1e-3, 1e3]`, with coordinates within `max` (the surface wrappers'
/// `check_points`).
#[must_use]
pub fn points3<'a>(pts: impl IntoIterator<Item = &'a [f64; 4]>, max: f64) -> bool {
    let mut rat = None;
    pts.into_iter().all(|&[x, y, z, w]| {
        let r = w != NOW;
        z != NOZ
            && *rat.get_or_insert(r) == r
            && (!r || (1e-3..=1e3).contains(&w))
            && [x, y, z].iter().all(|&c| finite(c, max))
    })
}

/// What a B-spline surface block that keeps the contract tells the rest of a case.
#[derive(Debug, Clone, Copy)]
pub struct SurfaceBlock {
    /// The parameter domain `[u0, v0, u1, v1]`.
    pub dom: [f64; 4],
    /// The number of distinct u knots.
    pub u_distinct: usize,
    /// The number of distinct v knots.
    pub v_distinct: usize,
}

fn distinct(k: &[f64]) -> usize {
    1 + k.windows(2).filter(|w| w.first() != w.last()).count()
}

/// A B-spline surface of control net `net` (rows in u), degrees `p`, `q` and knots `u`, `v`,
/// if it keeps the surface wrappers' contract: `1 ≤ p ≤ n`, `1 ≤ q ≤ m` within the limits,
/// full rows, `n + p + 2` and `m + q + 2` knots, [`points3`] over the whole net, and
/// [`clamped`] knots within range.
#[must_use]
pub fn surface_block(
    net: &[Vec<[f64; 4]>],
    p: usize,
    q: usize,
    u: &[f64],
    v: &[f64],
    lim: &Limits,
) -> Option<SurfaceBlock> {
    let n = net.len().checked_sub(1)?;
    let cols = net.first()?.len();
    let m = cols.checked_sub(1)?;
    let ok = n <= lim.max_index
        && m <= lim.max_index
        && (1..=n.min(lim.max_degree)).contains(&p)
        && (1..=m.min(lim.max_degree)).contains(&q)
        && u.len() == n + p + 2
        && v.len() == m + q + 2
        && net.iter().all(|row| row.len() == cols)
        && points3(net.iter().flatten(), lim.max_coord)
        && u.iter().chain(v).all(|&k| finite(k, lim.max_coord));
    if !ok {
        return None;
    }
    let (u0, u1) = clamped(u, p)?;
    let (v0, v1) = clamped(v, q)?;
    Some(SurfaceBlock {
        dom: [u0, v0, u1, v1],
        u_distinct: distinct(u),
        v_distinct: distinct(v),
    })
}

/// Whether a B-spline curve of dimension `dim`, control points `cpts`, degree `p` and knots
/// `u` keeps the surface wrappers' curve contract: dimension 2 or 3, `1 ≤ p ≤ n` within the
/// limits, `n + p + 2` knots, [`points3`] and [`clamped`] knots within range.
#[must_use]
pub fn curve_block(dim: u64, cpts: &[[f64; 4]], p: usize, u: &[f64], lim: &Limits) -> bool {
    let Some(n) = cpts.len().checked_sub(1) else {
        return false;
    };
    (2..=3).contains(&dim)
        && n <= lim.max_index
        && (1..=n.min(lim.max_degree)).contains(&p)
        && u.len() == n + p + 2
        && points3(cpts, lim.max_coord)
        && u.iter().all(|&k| finite(k, lim.max_coord))
        && clamped(u, p).is_some()
}

/// Whether `d` is a finite domain `[umin, vmin, umax, vmax]` within `max`, ordered.
#[must_use]
pub fn domain(d: &[f64; 4], max: f64) -> bool {
    d.iter().all(|&c| finite(c, max)) && d[0] <= d[2] && d[1] <= d[3]
}

/// Whether `d` is a [`domain`] inside `outer`, and when `strict` has sides of at least
/// `1e-6` of `outer`'s.
#[must_use]
pub fn domain_in(d: &[f64; 4], outer: &[f64; 4], strict: bool, max: f64) -> bool {
    let o = outer;
    domain(d, max)
        && d[0] >= o[0]
        && d[1] >= o[1]
        && d[2] <= o[2]
        && d[3] <= o[3]
        && (!strict || (d[2] - d[0] >= 1e-6 * (o[2] - o[0]) && d[3] - d[1] >= 1e-6 * (o[3] - o[1])))
}

/// Whether a curve's control points stored `[x, y, z, w]` keep [`points3`], except that a
/// 2D curve (`planar`) may store `z = NOZ` (counted as 0); the `check_points` of the
/// `surface_p5` and `surface_p6` wrappers.
#[must_use]
pub fn curve_points(pts: &[[f64; 4]], planar: bool, max: f64) -> bool {
    let mut rat = None;
    pts.iter().all(|&[x, y, z, w]| {
        let r = w != NOW;
        (z != NOZ || planar)
            && *rat.get_or_insert(r) == r
            && (!r || (1e-3..=1e3).contains(&w))
            && [x, y, if z == NOZ { 0.0 } else { z }]
                .iter()
                .all(|&c| finite(c, max))
    })
}

/// The knot range of a B-spline curve of dimension `dim`, control points `cpts`, degree `p`
/// and knots `u`, if it keeps the `surface_p5` / `surface_p6` wrappers' curve contract:
/// dimension 2 or 3, `1 ≤ p ≤ n` within the limits, `n + p + 2` knots, [`curve_points`]
/// and [`clamped`] knots within range.
#[must_use]
pub fn planar_curve_block(
    dim: u64,
    cpts: &[[f64; 4]],
    p: usize,
    u: &[f64],
    lim: &Limits,
) -> Option<(f64, f64)> {
    let n = cpts.len().checked_sub(1)?;
    let ok = (2..=3).contains(&dim)
        && n <= lim.max_index
        && (1..=n.min(lim.max_degree)).contains(&p)
        && u.len() == n + p + 2
        && curve_points(cpts, dim == 2, lim.max_coord)
        && u.iter().all(|&k| finite(k, lim.max_coord));
    if !ok {
        return None;
    }
    clamped(u, p)
}

/// The B-rep blocks of the `topology.vertexuse` fixture (`.smb` file, wire edge, face,
/// network, shell vertices and edges, face list, exact ellipse face, edge-loop face; the
/// layout is documented with that runner's `brep_block`), as the reference wrapper
/// accepts them before it builds the B-rep: an existing `.smb` file, B-spline curves and
/// surfaces of at most 33 control points per direction and degree 5 with one rationality,
/// weights in `[1e-3, 1e3]`, finite values within `1e300` and clamped knots, plane
/// normals of non-zero length, wire intervals inside the curve's, ordered face domains
/// inside a B-spline surface's or within `1e4` on a plane, the analytic, ellipse and
/// edge-loop face ranges, network points within `1e6` and edges between two distinct
/// points, and no value of the block above `1e150` other than `±f64::MAX` (HANG-29:
/// building such a face never returns upstream). `block` must be exactly one block.
pub mod vertexuse_block {
    use std::path::Path;

    use solidcore::nurbs::math::{NOW, NOZ};

    /// Largest control point index.
    const MAX_INDEX: usize = 32;
    /// Largest degree.
    const MAX_DEGREE: usize = 5;
    /// Largest coordinate magnitude.
    const MAX_COORD: f64 = 1.0e300;
    /// Largest block value before HANG-29.
    const MAX_BLOCK_MAGNITUDE: f64 = 1.0e150;
    /// Most faces of a face list.
    const MAX_FACES: usize = 4;
    /// Most points of a network block.
    const MAX_NETWORK_POINTS: usize = 64;
    /// Most edges of a network block.
    const MAX_NETWORK_EDGES: usize = 128;
    /// Most edges of an edge-loop face.
    const MAX_EDGE_LOOP_EDGES: usize = 64;

    /// Whether `block` (of an `.smb` file at `path` for kind 0) keeps the contract.
    #[must_use]
    pub fn holds(path: &str, block: &[f64]) -> bool {
        check(path, block).is_ok()
    }

    /// [`holds`], with the rule `block` breaks.
    ///
    /// # Errors
    ///
    /// The broken rule: a malformed block, or a value above `1e150` (HANG-29).
    pub fn check(path: &str, block: &[f64]) -> Result<(), &'static str> {
        let mut rd = Reader {
            input: block,
            at: 0,
        };
        if brep(&mut rd, path).is_none() {
            return Err("malformed B-rep block");
        }
        if rd.at != block.len() {
            return Err("trailing input after the B-rep block");
        }
        if block.iter().any(|v| {
            let a = v.abs();
            a > MAX_BLOCK_MAGNITUDE && a < f64::MAX
        }) {
            return Err("B-rep value above 1e150 (HANG-29)");
        }
        Ok(())
    }

    struct Reader<'a> {
        input: &'a [f64],
        at: usize,
    }

    impl Reader<'_> {
        fn num(&mut self) -> Option<f64> {
            let v = *self.input.get(self.at)?;
            self.at += 1;
            Some(v)
        }
        fn finite(&mut self) -> Option<f64> {
            let v = self.num()?;
            (v.is_finite() && v.abs() <= MAX_COORD).then_some(v)
        }
        fn finites(&mut self, k: usize) -> Option<Vec<f64>> {
            (0..k).map(|_| self.finite()).collect()
        }
        fn vec3(&mut self) -> Option<[f64; 3]> {
            Some([self.finite()?, self.finite()?, self.finite()?])
        }
        fn nonzero3(&mut self) -> Option<[f64; 3]> {
            let v = self.vec3()?;
            (v.iter().map(|c| c * c).sum::<f64>() >= 1e-12).then_some(v)
        }
        fn index(&mut self, max: usize) -> Option<usize> {
            let v = self.num()?;
            (v.fract() == 0.0 && v >= 0.0 && v <= max as f64).then_some(v as usize)
        }
        fn flag(&mut self) -> Option<bool> {
            Some(self.index(1)? == 1)
        }
    }

    /// `k` control points of one rationality (`z = NOZ` only when `planar`).
    fn points(rd: &mut Reader<'_>, k: usize, planar: bool) -> Option<()> {
        let mut w0 = None;
        for _ in 0..k {
            let (x, y, z, w) = (rd.num()?, rd.num()?, rd.num()?, rd.num()?);
            if z == NOZ && !planar {
                return None;
            }
            let rational = w != NOW;
            if *w0.get_or_insert(rational) != rational {
                return None;
            }
            if rational && !(1e-3..=1e3).contains(&w) {
                return None;
            }
            let z = if z == NOZ { 0.0 } else { z };
            if [x, y, z]
                .iter()
                .any(|c| !c.is_finite() || c.abs() > MAX_COORD)
            {
                return None;
            }
        }
        Some(())
    }

    /// The parameter domain of a surface block.
    enum Domain {
        /// A plane (unbounded).
        Plane,
        /// A B-spline surface's `[u0, v0, u1, v1]`.
        BSpline([f64; 4]),
    }

    /// A surface block; its domain.
    fn surface(rd: &mut Reader<'_>) -> Option<Domain> {
        if rd.index(1)? == 1 {
            rd.vec3()?;
            rd.nonzero3()?;
            return Some(Domain::Plane);
        }
        rd.flag()?;
        rd.index(11)?;
        let n = rd.index(MAX_INDEX)?;
        let m = rd.index(MAX_INDEX)?;
        let p = rd.index(MAX_DEGREE)?;
        let q = rd.index(MAX_DEGREE)?;
        let r = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
        let s = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
        if p == 0 || p > n || q == 0 || q > m || r != n + p + 1 || s != m + q + 1 {
            return None;
        }
        points(rd, (n + 1) * (m + 1), false)?;
        let u = rd.finites(r + 1)?;
        let v = rd.finites(s + 1)?;
        let (u0, u1) = super::clamped(&u, p)?;
        let (v0, v1) = super::clamped(&v, q)?;
        Some(Domain::BSpline([u0, v0, u1, v1]))
    }

    /// A B-spline curve block; its parameter range.
    fn curve(rd: &mut Reader<'_>) -> Option<(f64, f64)> {
        let dim = rd.index(3)?;
        if dim < 2 {
            return None;
        }
        let n = rd.index(MAX_INDEX)?;
        let p = rd.index(MAX_DEGREE)?;
        let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
        if p == 0 || p > n || m != n + p + 1 {
            return None;
        }
        points(rd, n + 1, dim == 2)?;
        let knots = rd.finites(m + 1)?;
        super::clamped(&knots, p)
    }

    /// The `surface u0 v0 u1 v1` face of a face block.
    fn face(rd: &mut Reader<'_>) -> Option<()> {
        let dom = surface(rd)?;
        let d = rd.finites(4)?;
        let [a, b, c, e] = <[f64; 4]>::try_from(d.as_slice()).ok()?;
        if a >= c || b >= e {
            return None;
        }
        match dom {
            Domain::BSpline([u0, v0, u1, v1]) => {
                (a >= u0 && c <= u1 && b >= v0 && e <= v1).then_some(())
            }
            Domain::Plane => d.iter().all(|x| x.abs() <= 1e4).then_some(()),
        }
    }

    /// A canonical analytic face of a face list (kinds 10 to 13).
    fn analytic_face(rd: &mut Reader<'_>) -> Option<()> {
        let kind = rd.index(13)?;
        let (scale, a, b, c) = (rd.finite()?, rd.finite()?, rd.finite()?, rd.finite()?);
        if !(1e-3..=1e3).contains(&scale) || !(1e-3..=1e2).contains(&a) || !(0.0..=1e2).contains(&c)
        {
            return None;
        }
        let shape = match kind {
            10 | 11 => true,
            12 => b > 0.0 && b < a,
            13 => b > 0.0 && b < 90.0,
            _ => false,
        };
        if !shape || rd.vec3()?.iter().any(|x| x.abs() > 1e4) {
            return None;
        }
        rd.index(2)?;
        let f = rd.finites(4)?;
        let [f0u, f0v, f1u, f1v] = <[f64; 4]>::try_from(f.as_slice()).ok()?;
        (f.iter().all(|t| (0.0..=1.0).contains(t)) && f0u < f1u && f0v < f1v).then_some(())
    }

    /// The exact ellipse face (kind 9).
    fn ellipse_face(rd: &mut Reader<'_>) -> Option<()> {
        let circle = rd.flag()?;
        for _ in 0..3 {
            if rd.finite()?.abs() > 1e4 {
                return None;
            }
        }
        rd.index(2)?;
        let (rx, ry) = (rd.finite()?, rd.finite()?);
        if !(1e-3..=1e4).contains(&rx) || !(1e-3..=1e4).contains(&ry) || (circle && rx != ry) {
            return None;
        }
        let (a, b) = (rd.finite()?, rd.finite()?);
        if a.abs() > 360.0 || b.abs() > 720.0 || !(1.0..=360.0).contains(&(b - a)) {
            return None;
        }
        let (a, b) = (rd.finite()?, rd.finite()?);
        if a.abs() > 1e4 || b.abs() > 1e4 || b - a < 1e-3 {
            return None;
        }
        rd.flag()?;
        rd.flag()?;
        Some(())
    }

    /// The edge-loop face (kind 14).
    fn edge_loop_face(rd: &mut Reader<'_>) -> Option<()> {
        rd.vec3()?;
        rd.nonzero3()?;
        let d = rd.finites(4)?;
        let [u0, v0, u1, v1] = <[f64; 4]>::try_from(d.as_slice()).ok()?;
        if u0 >= u1 || v0 >= v1 {
            return None;
        }
        rd.finite()?;
        let n = rd.index(MAX_EDGE_LOOP_EDGES)?;
        if n == 0 {
            return None;
        }
        for _ in 0..n {
            let vectors = match rd.index(3)? {
                1 => 2,
                3 => 4,
                _ => return None,
            };
            for _ in 0..vectors {
                rd.vec3()?;
            }
            let (t0, t1) = (rd.finite()?, rd.finite()?);
            if t0 >= t1 {
                return None;
            }
            rd.finite()?;
        }
        Some(())
    }

    fn brep(rd: &mut Reader<'_>, path: &str) -> Option<()> {
        match rd.index(14)? {
            0 => {
                let p = Path::new(path);
                if p.extension().and_then(|e| e.to_str()) != Some("smb") || !p.is_file() {
                    return None;
                }
                rd.flag()?;
            }
            1 => {
                let (a, b) = curve(rd)?;
                let (t0, t1) = (rd.finite()?, rd.finite()?);
                if !(a <= t0 && t0 < t1 && t1 <= b) {
                    return None;
                }
            }
            kind @ (2 | 4) => {
                face(rd)?;
                if kind == 4 {
                    for _ in 0..2 {
                        if !(0.0..=1.0).contains(&rd.finite()?) {
                            return None;
                        }
                    }
                }
            }
            10 => {
                let nf = rd.index(MAX_FACES)?;
                if nf == 0 {
                    return None;
                }
                for _ in 0..nf {
                    if rd.input.get(rd.at).is_some_and(|&k| k >= 10.0) {
                        analytic_face(rd)?;
                    } else {
                        face(rd)?;
                    }
                }
            }
            5 => {
                let np = rd.index(MAX_NETWORK_POINTS)?;
                for _ in 0..np {
                    if rd.vec3()?.iter().any(|c| c.abs() > 1e6) {
                        return None;
                    }
                }
                for _ in 0..rd.index(MAX_NETWORK_EDGES)? {
                    let a = rd.index(np.saturating_sub(1))?;
                    let b = rd.index(np.saturating_sub(1))?;
                    if np == 0 || a == b {
                        return None;
                    }
                    if rd.flag()? {
                        curve(rd)?;
                    }
                }
            }
            9 => ellipse_face(rd)?,
            14 => edge_loop_face(rd)?,
            3 => {}
            _ => return None,
        }
        rd.flag()?;
        rd.flag()?;
        Some(())
    }
}

// ---- topology fixture B-rep blocks ----------------------------------------------------------

/// The size bounds of the topology fixture's B-rep blocks (`topology_vertexuse`'s
/// `push_curve` / `push_surface` layout): control point indices up to 32, degrees up to 5,
/// coordinates up to `1e300`.
pub const BREP_BLOCK: Limits = Limits {
    max_index: 32,
    max_degree: 5,
    max_coord: 1.0e300,
};

/// Largest magnitude of a B-rep block value below `f64::MAX` (HANG-29: building a face
/// from such a block never returns upstream).
pub const MAX_BLOCK_MAGNITUDE: f64 = 1.0e150;

/// A sequential reader over a flat input record, for the contracts that walk one.
#[derive(Debug, Clone)]
pub struct Values<'a> {
    /// The record.
    pub input: &'a [f64],
    /// The index of the next value.
    pub at: usize,
}

impl<'a> Values<'a> {
    /// A reader at `at` in `input`.
    #[must_use]
    pub fn new(input: &'a [f64], at: usize) -> Self {
        Self { input, at }
    }

    /// The next value.
    pub fn num(&mut self) -> Option<f64> {
        let v = *self.input.get(self.at)?;
        self.at += 1;
        Some(v)
    }

    /// The next value, if it is [`finite`] within `max`.
    pub fn finite(&mut self, max: f64) -> Option<f64> {
        self.num().filter(|&v| finite(v, max))
    }

    /// The next `k` values, each [`Values::finite`].
    pub fn finites(&mut self, k: usize, max: f64) -> Option<Vec<f64>> {
        (0..k).map(|_| self.finite(max)).collect()
    }

    /// The next three values, each [`Values::finite`].
    pub fn vec3(&mut self, max: f64) -> Option<[f64; 3]> {
        Some([self.finite(max)?, self.finite(max)?, self.finite(max)?])
    }

    /// The next value as an integer in `0..=max`.
    pub fn index(&mut self, max: usize) -> Option<usize> {
        let v = self.num()?;
        (v.fract() == 0.0 && v >= 0.0 && v <= max as f64).then_some(v as usize)
    }

    /// The next value as a flag, `0` or `1`.
    pub fn flag(&mut self) -> Option<bool> {
        Some(self.index(1)? == 1)
    }

    /// The next value as an index below `count`.
    pub fn below(&mut self, count: usize) -> Option<usize> {
        self.index(usize::MAX >> 12).filter(|&i| i < count)
    }

    /// The next value as an integer in `lo..=hi`.
    pub fn int(&mut self, lo: i64, hi: i64) -> Option<i64> {
        let v = self.num()?;
        (v.fract() == 0.0 && v >= lo as f64 && v <= hi as f64).then_some(v as i64)
    }

    /// The next value as `-1` (`Some(None)`) or an index below `n`.
    pub fn opt_below(&mut self, n: usize) -> Option<Option<usize>> {
        let v = self.int(-1, i64::from(u32::MAX))?;
        match usize::try_from(v) {
            Err(_) => Some(None),
            Ok(i) if i < n => Some(Some(i)),
            Ok(_) => None,
        }
    }

    /// Whether the whole record was read.
    #[must_use]
    pub fn done(&self) -> bool {
        self.at == self.input.len()
    }

    /// The next `k` control points, stored `[x, y, z, w]`, if they keep [`curve_points`].
    fn points(&mut self, k: usize, planar: bool) -> Option<()> {
        let mut pts = Vec::with_capacity(k.min(1024));
        for _ in 0..k {
            pts.push([self.num()?, self.num()?, self.num()?, self.num()?]);
        }
        curve_points(&pts, planar, BREP_BLOCK.max_coord).then_some(())
    }

    /// A B-spline curve block `dim n p m cpts knots` with at most `max_index + 1` control
    /// points: dimension 2 or 3, `1 ≤ p ≤ n`, `m = n + p + 1`, [`curve_points`] and
    /// [`clamped`] knots. Its parameter range.
    pub fn brep_curve(&mut self, max_index: usize) -> Option<(f64, f64)> {
        let lim = BREP_BLOCK;
        let dim = self.index(3)?;
        if dim < 2 {
            return None;
        }
        let n = self.index(max_index)?;
        let p = self.index(lim.max_degree)?;
        let m = self.index(max_index + lim.max_degree + 1)?;
        if p == 0 || p > n || m != n + p + 1 {
            return None;
        }
        self.points(n + 1, dim == 2)?;
        let knots = self.finites(m + 1, lim.max_coord)?;
        clamped(&knots, p)
    }

    /// A surface block: a plane `1 O[3] N[3]` with a non-zero normal (`Some(None)`), or a
    /// B-spline `0 oob form n m p q r s cpts u v` with `1 ≤ p ≤ n`, `1 ≤ q ≤ m`,
    /// `r = n + p + 1`, `s = m + q + 1`, 3D [`curve_points`] and [`clamped`] knots
    /// (`Some(Some(domain))`).
    pub fn brep_surface(&mut self) -> Option<Option<[f64; 4]>> {
        let lim = BREP_BLOCK;
        if self.index(1)? == 1 {
            self.vec3(lim.max_coord)?;
            let n = self.vec3(lim.max_coord)?;
            return (len_sq(n) >= 1e-12).then_some(None);
        }
        self.flag()?;
        self.index(11)?;
        let n = self.index(lim.max_index)?;
        let m = self.index(lim.max_index)?;
        let p = self.index(lim.max_degree)?;
        let q = self.index(lim.max_degree)?;
        let r = self.index(lim.max_index + lim.max_degree + 1)?;
        let s = self.index(lim.max_index + lim.max_degree + 1)?;
        if p == 0 || p > n || q == 0 || q > m || r != n + p + 1 || s != m + q + 1 {
            return None;
        }
        self.points((n + 1) * (m + 1), false)?;
        let u = self.finites(r + 1, lim.max_coord)?;
        let v = self.finites(s + 1, lim.max_coord)?;
        let (u0, u1) = clamped(&u, p)?;
        let (v0, v1) = clamped(&v, q)?;
        Some(Some([u0, v0, u1, v1]))
    }

    /// A one-face block `surface u0 v0 u1 v1`: an ordered domain inside a B-spline's
    /// domain, or within `1e4` on a plane.
    fn brep_face(&mut self) -> Option<()> {
        let dom = self.brep_surface()?;
        let d = self.finites(4, BREP_BLOCK.max_coord)?;
        let [a, b, c, e] = <[f64; 4]>::try_from(d).ok()?;
        if a >= c || b >= e {
            return None;
        }
        let ok = match dom {
            Some([u0, v0, u1, v1]) => a >= u0 && c <= u1 && b >= v0 && e <= v1,
            None => [a, b, c, e].iter().all(|x| x.abs() <= 1e4),
        };
        ok.then_some(())
    }

    /// A topology fixture B-rep block of kind `0` (an existing `.smb` file at `path`, then
    /// its rebuild flag), `1` (a wire edge: a curve, then `t0 < t1` inside its range), `2`
    /// (one face) or `3` (the wire network), then the validate-only and cached-gap flags.
    pub fn brep_block(&mut self, path: &str) -> Option<()> {
        match self.index(14)? {
            0 => {
                let p = std::path::Path::new(path);
                if p.extension().and_then(|e| e.to_str()) != Some("smb") || !p.is_file() {
                    return None;
                }
                self.flag()?;
            }
            1 => {
                let (a, b) = self.brep_curve(BREP_BLOCK.max_index)?;
                let t0 = self.finite(BREP_BLOCK.max_coord)?;
                let t1 = self.finite(BREP_BLOCK.max_coord)?;
                if !(a <= t0 && t0 < t1 && t1 <= b) {
                    return None;
                }
            }
            2 => self.brep_face()?,
            3 => {}
            // the kinds the other fixture writers (nets, analytic faces) emit
            _ => return None,
        }
        self.flag()?;
        self.flag()?;
        Some(())
    }
}

/// Whether the topology fixture B-rep block at the start of `input` keeps the contract
/// ([`Values::brep_block`]) and holds no value of magnitude above
/// [`MAX_BLOCK_MAGNITUDE`] and below `f64::MAX` (the non-rational weight).
#[must_use]
pub fn brep_block(path: &str, input: &[f64]) -> bool {
    let mut rd = Values::new(input, 0);
    rd.brep_block(path).is_some()
        && input.get(..rd.at).is_some_and(|block| {
            !block.iter().any(|v| {
                let a = v.abs();
                a > MAX_BLOCK_MAGNITUDE && a < f64::MAX
            })
        })
}
