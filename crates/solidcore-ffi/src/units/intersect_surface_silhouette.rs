//! FFI wrappers for unit `intersect.surface_silhouette`.
//!
//! Shim: `harness/cpp/shim/intersect_surface_silhouette.cpp`
//! (`sc_intersect_surface_silhouette_run`). Upstream: `source/SMLib/src/SmSurfaceSilhouette.cpp`,
//! `source/SMLib/inc/SmSurfaceSilhouette.h` (`SmSurfaceSilhouette`).
//!
//! [`run`] builds a surface (a real `SmBSplineSurface` or `SmPlane`), an `SmSurfaceSilhouette`
//! over a domain for a view direction or eye point, applies the optional tolerances, runs the
//! [`Op`]s in order and returns the record:
//!
//! * the constructor's approximation and angle tolerances;
//! * per op `op status [values]` (see the table);
//! * the final trace direction.
//!
//! Statuses are `0` (`SM_SUCCESS`) or the `SmStatus`. A trace point is `type c3d[0](3)
//! c3d[1](3) cuv[0](2) cuv[1](2)` then the surface matrix entries `[0][0] [1][0] [0][1] [1][1]
//! [2][0] [0][2] [2][2]` (3 each). A solution is `type nvars nobjs has_obj0 start_value
//! start[np] end_value end[np]` with `np = clamp(nvars, 2, 10)`.
//!
//! | [`Op`]     | upstream                                                       | values |
//! |------------|----------------------------------------------------------------|--------|
//! | `Cpv`      | `m_dCurveTraceDirection = dir`, then `ComputePointValues(uv, pnt, prev)` on a new point (zero vectors, the given type and 3D tangent); `prev` is a copy of the last successful `Cpv` point when asked for and there is one, else `NULL` | the point (status 0 only) |
//! | `Lps`      | `LocalPointSolve(pnt, guess, next, found, uv)`                | `found [uv]` (status 0 only) |
//! | `OnCurve`  | `DoesPointLieOnCurve(uv, refine)` (status always 0)            | `flag uv` |
//! | `Boundary` | `FindSolutionsOnBoundaryCurve(curve, [a, b], 0, SM_SP_U, sols)` | `count` solutions (status 0 only) |
//! | `Branch`   | a hand-built tree of a surface cache over the surface's natural domain, then `BranchMayContainAnswers({node, NULL..})` per queried node (status always 0) | one flag per query |
//!
//! [`run`] accepts only finite inputs of magnitude at most [`MAX_COORD`]; well-formed clamped
//! B-splines (at most [`MAX_INDEX`] + 1 control points per surface direction, 33 per curve,
//! degree at most [`MAX_DEGREE`], positive weights or none); planes with independent axes and
//! positive scales; a domain with positive extent, inside the surface's natural domain for a
//! B-spline; for a B-spline, `Cpv`/`OnCurve` points inside the natural domain (upstream
//! evaluates them unclamped; a plane is linear and defined everywhere, so plane points and
//! tracer domains may lie outside its extent);
//! boundary intervals `a < b` inside the curve's knot range; at most [`MAX_OPS`] ops. Trees
//! have 1 to [`MAX_NODES`] nodes (node 0 the top), acyclic parent links, node domains of
//! positive extent inside the natural domain, and a null patch pointer only on a node no other
//! node names as its parent (upstream's polar box build walks the parents' data).

use core::cmp::Ordering;

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_intersect_surface_silhouette_run(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Largest control point index `n` of a surface direction.
pub const MAX_INDEX: usize = 16;
/// Largest control point index `n` of a curve.
pub const MAX_CURVE_INDEX: usize = 32;
/// Largest degree.
pub const MAX_DEGREE: usize = 8;
/// Largest input magnitude.
pub const MAX_COORD: f64 = 1e9;
/// Most ops per program.
pub const MAX_OPS: usize = 64;
/// Most nodes per tree, vectors per polar box, points per pseudo box and queries per `Branch`.
pub const MAX_NODES: usize = 16;
/// `NL_NOW`: the weight of a non-rational control point.
pub const NOW: f64 = -f64::MAX;
/// `SM_AD_SURFACE_DATA` and below are the `SmAuxDataType` codes accepted (`0..=7`).
pub const MAX_AUX_TYPE: u8 = 7;
/// `SM_AD_BEZIER_SURFACE`.
pub const AUX_BEZIER_SURFACE: u8 = 3;

/// A clamped B-spline surface: `SmBSplineSurface(gw_SURFACE*, FALSE, ctx)`.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceData {
    /// u degree.
    pub p: usize,
    /// v degree.
    pub q: usize,
    /// Control net, row `i` (u index) of `x y z w` points.
    pub net: Vec<Vec<[f64; 4]>>,
    /// u knots.
    pub u: Vec<f64>,
    /// v knots.
    pub v: Vec<f64>,
}

/// The surface.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceIn {
    /// A B-spline surface.
    BSpline(SurfaceData),
    /// `SmPlane(o, x, y, SmVector2d(scale), SmExtent2d(domain), ctx)`.
    Plane {
        /// Origin.
        o: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// u and v scales.
        scale: [f64; 2],
        /// `umin vmin umax vmax`.
        domain: [f64; 4],
    },
}

/// A clamped 3D B-spline curve: `SmBSplineCurve(3, n, m, p, Pw, U)`.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveData {
    /// Degree.
    pub p: usize,
    /// Control points as stored (`x y z w`, `w` = [`NOW`] when non-rational).
    pub cpts: Vec<[f64; 4]>,
    /// Knots (`n + p + 2` of them).
    pub knots: Vec<f64>,
}

/// A node of a hand-built surface cache tree.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeIn {
    /// Parent index (`None`: no parent).
    pub parent: Option<usize>,
    /// First child index.
    pub child1: Option<usize>,
    /// Second child index.
    pub child2: Option<usize>,
    /// `m_eAuxDataType` code (`0..=`[`MAX_AUX_TYPE`]).
    pub aux: u8,
    /// `m_pData = NULL`.
    pub null_data: bool,
    /// The patch's UV domain `umin vmin umax vmax`.
    pub domain: [f64; 4],
    /// Explicit normal box: the vectors added to an empty `SmPolarBox` (`None`: built on
    /// demand from the surface).
    pub polar: Option<Vec<[f64; 3]>>,
    /// Explicit pseudo box of a patch (`None`: `SmPseudoBox()`).
    pub pseudo: Option<PseudoIn>,
}

/// An explicit pseudo box: `SmPseudoBox()`, `SetBasis(basis)`, then `AddPoint3d` of each point.
#[derive(Debug, Clone, PartialEq)]
pub struct PseudoIn {
    /// The three basis vectors.
    pub basis: [[f64; 3]; 3],
    /// The points added.
    pub points: Vec<[f64; 3]>,
}

/// One operation (see the module table).
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// `ComputePointValues`.
    Cpv {
        /// `m_dCurveTraceDirection`.
        dir: f64,
        /// Pass the last successful point as `pPrevPnt`.
        use_prev: bool,
        /// Initial `m_ePointType` (`0..=4`).
        kind: u8,
        /// Initial `m_v3DCurvePV[1]`.
        t: [f64; 3],
        /// The parameter point.
        uv: [f64; 2],
    },
    /// `LocalPointSolve` from a guess.
    Lps([f64; 2]),
    /// `DoesPointLieOnCurve`.
    OnCurve {
        /// `bRefinePoint`.
        refine: bool,
        /// The parameter point.
        uv: [f64; 2],
    },
    /// `FindSolutionsOnBoundaryCurve`.
    Boundary {
        /// The boundary curve.
        curve: CurveData,
        /// The interval `[a, b]`.
        interval: [f64; 2],
    },
    /// `BranchMayContainAnswers` on nodes of a tree.
    Branch {
        /// The tree, node 0 the top.
        nodes: Vec<NodeIn>,
        /// The queried node indices.
        queries: Vec<usize>,
    },
}

/// A whole case.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    /// The surface.
    pub surface: SurfaceIn,
    /// The tracer domain `umin vmin umax vmax`.
    pub domain: [f64; 4],
    /// `bPerspective`.
    pub perspective: bool,
    /// The eye point (perspective) or view direction.
    pub eye: [f64; 3],
    /// `m_dThisApproxTol3d` override.
    pub tol: Option<f64>,
    /// `m_dThisAngTolRad` override.
    pub ang: Option<f64>,
    /// The operations.
    pub ops: Vec<Op>,
}

fn finite(x: f64) -> Result<f64, FfiError> {
    if x.is_finite() && x.abs() <= MAX_COORD {
        Ok(x)
    } else {
        Err(bad("non-finite or huge value"))
    }
}

/// Checks a clamped knot vector for `n1` control points of degree `p`.
fn check_knots(knots: &[f64], n1: usize, p: usize) -> Result<(), FfiError> {
    if knots.len() != n1 + p + 1 {
        return Err(bad("knot count"));
    }
    if knots.windows(2).any(|w| w.first() > w.last()) {
        return Err(bad("knots decreasing"));
    }
    let (Some(&k0), Some(&km)) = (knots.first(), knots.last()) else {
        return Err(bad("knot count"));
    };
    if km <= k0
        || knots.iter().take(p + 1).any(|&k| k != k0)
        || knots.iter().rev().take(p + 1).any(|&k| k != km)
    {
        return Err(bad("knots not clamped"));
    }
    let mut run = 0usize;
    let mut last = f64::NAN;
    for &k in knots.iter().filter(|&&k| k != k0 && k != km) {
        run = if k == last { run + 1 } else { 1 };
        last = k;
        if run > p {
            return Err(bad("interior knot multiplicity"));
        }
    }
    Ok(())
}

/// Checks the weights: all [`NOW`] or all in `(0, MAX_COORD]`.
fn check_weights<'a>(mut ws: impl Iterator<Item = &'a [f64; 4]>) -> Result<(), FfiError> {
    let Some(first) = ws.next() else {
        return Err(bad("no control points"));
    };
    let rational = first[3] != NOW;
    let ok = |w: f64| {
        if rational {
            w > 0.0 && w <= MAX_COORD
        } else {
            w == NOW
        }
    };
    if !ok(first[3]) || ws.any(|c| !ok(c[3])) {
        return Err(bad("weights"));
    }
    Ok(())
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// `d` has positive extent (and, unless `nat` is `None`, lies inside `nat`; both
/// `umin vmin umax vmax`).
fn check_domain(d: [f64; 4], nat: Option<[f64; 4]>) -> Result<(), FfiError> {
    if !(d[0] < d[2] && d[1] < d[3]) {
        return Err(bad("domain extent"));
    }
    let Some(nat) = nat else {
        return Ok(());
    };
    if d[0] < nat[0] || d[1] < nat[1] || d[2] > nat[2] || d[3] > nat[3] {
        return Err(bad("domain outside the surface domain"));
    }
    Ok(())
}

/// `uv` inside `nat` (closed); anything finite when `nat` is `None`.
fn check_uv(uv: [f64; 2], nat: Option<[f64; 4]>) -> Result<(), FfiError> {
    let Some(nat) = nat else {
        return Ok(());
    };
    if uv[0] < nat[0] || uv[0] > nat[2] || uv[1] < nat[1] || uv[1] > nat[3] {
        return Err(bad("point outside the surface domain"));
    }
    Ok(())
}

/// Checks the tree links: indices in range, acyclic parent chains, null data only on nodes
/// that are nobody's parent.
fn check_tree(nodes: &[NodeIn], nat: [f64; 4]) -> Result<(), FfiError> {
    let n = nodes.len();
    if n == 0 || n > MAX_NODES {
        return Err(bad("tree size"));
    }
    for nd in nodes {
        if [nd.parent, nd.child1, nd.child2]
            .iter()
            .flatten()
            .any(|&k| k >= n)
        {
            return Err(bad("tree link out of range"));
        }
        if nd.aux > MAX_AUX_TYPE {
            return Err(bad("aux type"));
        }
        check_domain(nd.domain, Some(nat))?;
        if nd.polar.as_ref().is_some_and(|v| v.len() > MAX_NODES) {
            return Err(bad("too many polar vectors"));
        }
        if nd
            .pseudo
            .as_ref()
            .is_some_and(|ps| ps.points.len() > MAX_NODES)
        {
            return Err(bad("too many pseudo box points"));
        }
    }
    // acyclic parent chains
    for start in 0..n {
        let mut cur = start;
        let mut steps = 0usize;
        while let Some(p) = nodes.get(cur).and_then(|nd| nd.parent) {
            steps += 1;
            if steps > n {
                return Err(bad("cyclic parent links"));
            }
            cur = p;
        }
    }
    for (k, nd) in nodes.iter().enumerate() {
        if nd.null_data && nodes.iter().any(|o| o.parent == Some(k)) {
            return Err(bad("null data on a parent node"));
        }
    }
    Ok(())
}

struct Enc(Vec<f64>);

impl Enc {
    fn num(&mut self, x: f64) -> Result<(), FfiError> {
        self.0.push(finite(x)?);
        Ok(())
    }
    fn int(&mut self, x: usize) {
        self.0.push(x as f64);
    }
    fn link(&mut self, x: Option<usize>) {
        self.0.push(x.map_or(-1.0, |k| k as f64));
    }
    fn flag(&mut self, b: bool) {
        self.0.push(if b { 1.0 } else { 0.0 });
    }
    fn nums(&mut self, v: &[f64]) -> Result<(), FfiError> {
        v.iter().try_for_each(|&x| self.num(x))
    }
    fn cpt(&mut self, c: &[f64; 4]) -> Result<(), FfiError> {
        self.nums(&c[..3])?;
        // the weight was checked (NL_NOW or positive)
        self.0.push(c[3]);
        Ok(())
    }

    /// The surface block; returns its natural domain.
    fn surface(&mut self, s: &SurfaceIn) -> Result<[f64; 4], FfiError> {
        match s {
            SurfaceIn::BSpline(d) => {
                let n1 = d.net.len();
                let m1 = d.net.first().map_or(0, Vec::len);
                if !(2..=MAX_INDEX + 1).contains(&n1)
                    || !(2..=MAX_INDEX + 1).contains(&m1)
                    || d.net.iter().any(|r| r.len() != m1)
                {
                    return Err(bad("control net size"));
                }
                if d.p == 0 || d.q == 0 || d.p > MAX_DEGREE || d.q > MAX_DEGREE {
                    return Err(bad("degree"));
                }
                if d.p >= n1 || d.q >= m1 {
                    return Err(bad("degree too high for the net"));
                }
                check_knots(&d.u, n1, d.p)?;
                check_knots(&d.v, m1, d.q)?;
                check_weights(d.net.iter().flatten())?;
                self.0.push(0.0);
                self.int(n1 - 1);
                self.int(m1 - 1);
                self.int(d.p);
                self.int(d.q);
                self.int(d.u.len() - 1);
                self.int(d.v.len() - 1);
                for c in d.net.iter().flatten() {
                    self.cpt(c)?;
                }
                self.nums(&d.u)?;
                self.nums(&d.v)?;
                let (Some(&u0), Some(&u1), Some(&v0), Some(&v1)) =
                    (d.u.first(), d.u.last(), d.v.first(), d.v.last())
                else {
                    return Err(bad("knot count"));
                };
                Ok([u0, v0, u1, v1])
            }
            SurfaceIn::Plane {
                o,
                x,
                y,
                scale,
                domain,
            } => {
                let c = norm(cross(*x, *y));
                let independent =
                    c.partial_cmp(&(1e-6 * norm(*x) * norm(*y))) == Some(Ordering::Greater);
                if !independent || norm(*x) < 1e-9 || norm(*y) < 1e-9 {
                    return Err(bad("plane axes"));
                }
                if !(scale[0] > 1e-9 && scale[1] > 1e-9) {
                    return Err(bad("plane scale"));
                }
                if !(domain[0] < domain[2] && domain[1] < domain[3]) {
                    return Err(bad("plane domain"));
                }
                self.0.push(1.0);
                self.nums(o)?;
                self.nums(x)?;
                self.nums(y)?;
                self.nums(scale)?;
                self.nums(domain)?;
                Ok(*domain)
            }
        }
    }

    fn curve(&mut self, d: &CurveData) -> Result<(f64, f64), FfiError> {
        let n1 = d.cpts.len();
        if !(2..=MAX_CURVE_INDEX + 1).contains(&n1) || d.p == 0 || d.p > MAX_DEGREE || d.p >= n1 {
            return Err(bad("curve size"));
        }
        check_knots(&d.knots, n1, d.p)?;
        check_weights(d.cpts.iter())?;
        self.int(n1 - 1);
        self.int(d.p);
        self.int(d.knots.len() - 1);
        for c in &d.cpts {
            self.cpt(c)?;
        }
        self.nums(&d.knots)?;
        match (d.knots.first(), d.knots.last()) {
            (Some(&a), Some(&b)) => Ok((a, b)),
            _ => Err(bad("knot count")),
        }
    }

    fn tree(&mut self, nodes: &[NodeIn], nat: [f64; 4]) -> Result<(), FfiError> {
        check_tree(nodes, nat)?;
        self.int(nodes.len());
        for nd in nodes {
            self.link(nd.parent);
            self.link(nd.child1);
            self.link(nd.child2);
            self.int(usize::from(nd.aux));
            self.flag(nd.null_data);
            self.nums(&nd.domain)?;
            self.flag(nd.polar.is_some());
            if let Some(v) = &nd.polar {
                self.int(v.len());
                for x in v {
                    self.nums(x)?;
                }
            }
            if nd.aux == AUX_BEZIER_SURFACE {
                self.flag(nd.pseudo.is_some());
                if let Some(ps) = &nd.pseudo {
                    for b in &ps.basis {
                        self.nums(b)?;
                    }
                    self.int(ps.points.len());
                    for p in &ps.points {
                        self.nums(p)?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// Encodes and validates `prog` (see the module docs for what is accepted).
fn encode(prog: &Program) -> Result<Vec<f64>, FfiError> {
    let mut e = Enc(Vec::new());
    let nat = e.surface(&prog.surface)?;
    // A plane evaluates linearly anywhere, so only B-spline points and domains are bounded.
    let bound = match prog.surface {
        SurfaceIn::BSpline(_) => Some(nat),
        SurfaceIn::Plane { .. } => None,
    };
    check_domain(prog.domain, bound)?;
    e.nums(&prog.domain)?;
    e.flag(prog.perspective);
    e.nums(&prog.eye)?;
    e.flag(prog.tol.is_some());
    e.num(prog.tol.unwrap_or(0.0))?;
    e.flag(prog.ang.is_some());
    e.num(prog.ang.unwrap_or(0.0))?;
    if prog.ops.len() > MAX_OPS {
        return Err(bad("too many ops"));
    }
    e.int(prog.ops.len());
    for op in &prog.ops {
        match op {
            Op::Cpv {
                dir,
                use_prev,
                kind,
                t,
                uv,
            } => {
                if *kind > 4 {
                    return Err(bad("point type"));
                }
                check_uv(*uv, bound)?;
                e.int(1);
                e.num(*dir)?;
                e.flag(*use_prev);
                e.int(usize::from(*kind));
                e.nums(t)?;
                e.nums(uv)?;
            }
            Op::Lps(guess) => {
                e.int(2);
                e.nums(guess)?;
            }
            Op::OnCurve { refine, uv } => {
                check_uv(*uv, bound)?;
                e.int(3);
                e.flag(*refine);
                e.nums(uv)?;
            }
            Op::Boundary { curve, interval } => {
                e.int(4);
                let (k0, km) = e.curve(curve)?;
                let [a, b] = *interval;
                if !(a < b && a >= k0 && b <= km) {
                    return Err(bad("boundary interval"));
                }
                e.nums(interval)?;
            }
            Op::Branch { nodes, queries } => {
                e.int(5);
                e.tree(nodes, nat)?;
                if queries.len() > MAX_NODES || queries.iter().any(|&q| q >= nodes.len()) {
                    return Err(bad("branch queries"));
                }
                e.int(queries.len());
                for &q in queries {
                    e.int(q);
                }
            }
        }
    }
    Ok(e.0)
}

/// Runs `prog` through the shim and returns the record (module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for inputs outside what the module docs accept;
/// [`FfiError::Status`] for a shim failure.
pub fn run(prog: &Program) -> Result<Vec<f64>, FfiError> {
    let input = encode(prog)?;
    let n_in = i64::try_from(input.len()).map_err(|_| bad("input too long"))?;
    call_with_buffer(16_384, |buf, len| {
        // SAFETY: input valid for n_in reads; buf valid for buf.len() writes; len valid.
        unsafe {
            sys::sc_intersect_surface_silhouette_run(
                input.as_ptr(),
                n_in,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
