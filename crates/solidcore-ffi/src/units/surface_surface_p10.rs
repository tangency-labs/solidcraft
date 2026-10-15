//! FFI wrappers for unit `surface.surface_p10`.
//!
//! Shim: `harness/cpp/shim/surface_surface_p10.cpp` (`sc_surface_surface_p10_run`). Upstream:
//! `source/SMLib/src/SmSurface.cpp` (`SmLSIGlobalSolver::LocalSolve`,
//! `sm_GlobalCurveIntersectCombineSolutions`).
//!
//! [`run`] builds a surface (a real `SmBSplineSurface` or `SmPlane`, with a context) and runs
//! one [`Op`]:
//!
//! * [`Op::Lsi`]: a surface cache of the surface over its natural domain with the given
//!   flags and a hand-built tree, an `SmLSIGlobalSolver` (`SM_SO_INTERSECT`, `SM_SR_ALL`, the
//!   given 3D tolerance) and `LocalSolve({node, NULL..})`. Record: `status [more n sol*n]`.
//! * [`Op::Combine`]: a B-spline curve, hand-built curve tree nodes (`SmTreeNode` with an
//!   `SmBezierAux1d` of the given interval), a hand-built surface cache tree, the solutions
//!   (objects: the curve and the surface) and the verbatim
//!   `sm_GlobalCurveIntersectCombineSolutions`. Record: `status [n sol*n]`.
//!
//! Statuses are `0` (`SM_SUCCESS`) or the `SmStatus`. A solution is `type nvars nobjs
//! start_value start[3] end_value end[3] node0 node1 obj0 obj1`: nodes are indices into the
//! curve nodes (`node0` of a combination) or the surface tree (`node1` of a combination,
//! `node0` of a line solve), `-1` for none; objects `0` the curve, `1` the surface, `-1` none.
//!
//! [`run`] accepts only finite inputs of magnitude at most [`MAX_COORD`]; well-formed clamped
//! B-splines (at most [`MAX_INDEX`] + 1 control points per surface direction, 33 per curve,
//! degree at most [`MAX_DEGREE`], positive weights or none); planes with independent axes and
//! positive scales. Trees have 1 to [`MAX_NODES`] nodes with in-range acyclic links, node
//! domains of positive extent inside the surface's natural domain (upstream evaluates a
//! B-spline there unclamped) and boxes with `min <= max`; the line solve's node is a patch
//! node (`SM_AD_BEZIER_SURFACE`: upstream reads its `SmBezierPatch` fields) and its polar
//! vectors are nonzero. Solver domains and intervals have `min <= max`, tolerances are not
//! negative. Combination solutions name nodes in range and have their curve parameters inside
//! the curve's knot range and, on a B-spline surface, their surface parameters inside the
//! natural domain (upstream evaluates there); the curve interval lies inside the knot range
//! with positive length.

use core::cmp::Ordering;

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_surface_surface_p10_run(
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
/// Most nodes per tree, vectors per polar box and points per pseudo box.
pub const MAX_NODES: usize = 16;
/// Most solutions of a combination.
pub const MAX_SOLUTIONS: usize = 64;
/// `NL_NOW`: the weight of a non-rational control point.
pub const NOW: f64 = -f64::MAX;
/// Largest `SmAuxDataType` code accepted.
pub const MAX_AUX_TYPE: u8 = 7;
/// `SM_AD_BEZIER_SURFACE`.
pub const AUX_BEZIER_SURFACE: u8 = 3;
/// Largest `SmContinuityType` code (`SM_CT_CINFINITY`).
pub const MAX_CONTINUITY: u8 = 13;
/// Largest `SmNodeClassType` code (`SM_NC_ON_BOUNDARY`).
pub const MAX_NODE_CLASS: u8 = 3;
/// Largest `SmSolutionType` code (`SM_ST_UNKNOWN`).
pub const MAX_SOLUTION_TYPE: u8 = 4;

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

/// An explicit pseudo box: `SmPseudoBox()`, `SetBasis(basis)`, then `AddPoint3d` of each point.
#[derive(Debug, Clone, PartialEq)]
pub struct PseudoIn {
    /// The three basis vectors.
    pub basis: [[f64; 3]; 3],
    /// The points added.
    pub points: Vec<[f64; 3]>,
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
    /// The UV domain `umin vmin umax vmax`.
    pub domain: [f64; 4],
    /// `m_eNodeClass` code.
    pub class: u8,
    /// `m_sBBox` `xmin ymin zmin xmax ymax zmax`.
    pub bbox: [f64; 6],
    /// `m_eUCurveConts` codes (patches only).
    pub u_conts: [u8; 2],
    /// `m_eVCurveConts` codes (patches only).
    pub v_conts: [u8; 2],
    /// Explicit normal box: the vectors added to an empty `SmPolarBox` (`None`: built on
    /// demand from the surface).
    pub polar: Option<Vec<[f64; 3]>>,
    /// Explicit pseudo box of a patch (`None`: `SmPseudoBox()`).
    pub pseudo: Option<PseudoIn>,
}

/// The surface cache flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(clippy::struct_excessive_bools)] // upstream's four SmSurfaceCache flags
pub struct CacheFlags {
    /// `m_bPointTestEnabled`.
    pub point_test: bool,
    /// `m_bHaveTSurfaceCache`.
    pub have_t_surface_cache: bool,
    /// `m_bFaceWasModified`.
    pub face_was_modified: bool,
    /// `SetProcessBoundaryCurves`.
    pub process_boundary_curves: bool,
}

/// A line/surface leaf solve.
#[derive(Debug, Clone, PartialEq)]
pub struct LsiIn {
    /// The cache flags.
    pub flags: CacheFlags,
    /// The tree, node 0 the top.
    pub nodes: Vec<NodeIn>,
    /// The solved node.
    pub node: usize,
    /// `m_crUVDomain`.
    pub uv_domain: [f64; 4],
    /// `m_vLinePoint`.
    pub point: [f64; 3],
    /// `m_vLineVector`.
    pub vector: [f64; 3],
    /// `m_cpLineInterval` (`None`: NULL).
    pub interval: Option<[f64; 2]>,
    /// `m_bRayFire`.
    pub ray_fire: bool,
    /// `m_dDistanceTolerance`.
    pub distance_tolerance: f64,
    /// `Set3DTolerance`.
    pub tolerance_3d: f64,
}

/// A curve tree node: `m_eAuxDataType` and its `SmBezierAux1d::m_sIvl`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveNodeIn {
    /// `m_eAuxDataType` code.
    pub aux: u8,
    /// The interval.
    pub interval: [f64; 2],
}

/// An input solution.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolutionIn {
    /// `m_eSolutionType` code.
    pub kind: u8,
    /// `m_lNumVariables` (`0..=3`).
    pub num_variables: u8,
    /// `m_vStart.m_dSolutionValue`.
    pub start_value: f64,
    /// `m_vStart[0..3]`.
    pub start: [f64; 3],
    /// `m_vEnd.m_dSolutionValue`.
    pub end_value: f64,
    /// `m_vEnd[0..3]`.
    pub end: [f64; 3],
    /// `m_apNodes[0]`: a curve node index.
    pub node0: Option<usize>,
    /// `m_apNodes[1]`: a surface tree node index.
    pub node1: Option<usize>,
}

/// A combination.
#[derive(Debug, Clone, PartialEq)]
pub struct CombineIn {
    /// The curve.
    pub curve: CurveData,
    /// `crUVDomain`.
    pub uv_domain: [f64; 4],
    /// `crInterval`.
    pub interval: [f64; 2],
    /// `dDistanceTolerance`.
    pub tolerance: f64,
    /// The curve tree nodes.
    pub curve_nodes: Vec<CurveNodeIn>,
    /// The surface cache tree (default flags), node 0 the top.
    pub surface_nodes: Vec<NodeIn>,
    /// The solutions.
    pub solutions: Vec<SolutionIn>,
}

/// The operation.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// `SmLSIGlobalSolver::LocalSolve`.
    Lsi(LsiIn),
    /// `sm_GlobalCurveIntersectCombineSolutions`.
    Combine(CombineIn),
}

/// A whole case.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    /// The surface.
    pub surface: SurfaceIn,
    /// The operation.
    pub op: Op,
}

fn bad(msg: &str) -> FfiError {
    FfiError::InvalidArgument(msg.into())
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

/// `d` has `min <= max` (`strict`: `min < max`) and, unless `nat` is `None`, lies inside
/// `nat` (both `umin vmin umax vmax`).
fn check_domain(d: [f64; 4], nat: Option<[f64; 4]>, strict: bool) -> Result<(), FfiError> {
    let ordered = if strict {
        d[0] < d[2] && d[1] < d[3]
    } else {
        d[0] <= d[2] && d[1] <= d[3]
    };
    if !ordered {
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

/// Checks the tree links and node data.
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
        if nd.aux > MAX_AUX_TYPE || nd.class > MAX_NODE_CLASS {
            return Err(bad("aux type or node class"));
        }
        if nd.u_conts.iter().chain(&nd.v_conts).any(|&c| c > MAX_CONTINUITY) {
            return Err(bad("continuity code"));
        }
        check_domain(nd.domain, Some(nat), true)?;
        if !(nd.bbox[0] <= nd.bbox[3] && nd.bbox[1] <= nd.bbox[4] && nd.bbox[2] <= nd.bbox[5]) {
            return Err(bad("node box"));
        }
        if let Some(v) = &nd.polar {
            if v.len() > MAX_NODES {
                return Err(bad("too many polar vectors"));
            }
            if v.iter().any(|x| norm(*x) < 1e-9) {
                return Err(bad("zero polar vector"));
            }
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

    /// The surface block; returns its natural domain and whether it is a B-spline.
    fn surface(&mut self, s: &SurfaceIn) -> Result<([f64; 4], bool), FfiError> {
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
                Ok(([u0, v0, u1, v1], true))
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
                Ok((*domain, false))
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
            self.nums(&nd.domain)?;
            self.int(usize::from(nd.class));
            self.nums(&nd.bbox)?;
            for c in nd.u_conts.iter().chain(&nd.v_conts) {
                self.int(usize::from(*c));
            }
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

fn nonneg(x: f64) -> Result<f64, FfiError> {
    if x >= 0.0 {
        Ok(x)
    } else {
        Err(bad("negative tolerance"))
    }
}

fn encode_lsi(e: &mut Enc, l: &LsiIn, nat: [f64; 4]) -> Result<(), FfiError> {
    for f in [
        l.flags.point_test,
        l.flags.have_t_surface_cache,
        l.flags.face_was_modified,
        l.flags.process_boundary_curves,
    ] {
        e.flag(f);
    }
    e.tree(&l.nodes, nat)?;
    let node = l.nodes.get(l.node).ok_or_else(|| bad("solved node"))?;
    if node.aux != AUX_BEZIER_SURFACE {
        return Err(bad("solved node is not a patch"));
    }
    e.int(l.node);
    check_domain(l.uv_domain, None, false)?;
    e.nums(&l.uv_domain)?;
    e.nums(&l.point)?;
    e.nums(&l.vector)?;
    e.flag(l.interval.is_some());
    let ivl = l.interval.unwrap_or([0.0, 0.0]);
    if ivl[0] > ivl[1] {
        return Err(bad("line interval"));
    }
    e.nums(&ivl)?;
    e.flag(l.ray_fire);
    e.num(nonneg(l.distance_tolerance)?)?;
    e.num(nonneg(l.tolerance_3d)?)
}

fn encode_combine(e: &mut Enc, c: &CombineIn, nat: [f64; 4], bspline: bool) -> Result<(), FfiError> {
    let (k0, k1) = e.curve(&c.curve)?;
    let inside_t = |t: f64| t >= k0 && t <= k1;
    let inside_uv = |u: f64, v: f64| !bspline || (u >= nat[0] && u <= nat[2] && v >= nat[1] && v <= nat[3]);
    check_domain(c.uv_domain, None, false)?;
    e.nums(&c.uv_domain)?;
    if !(c.interval[0] < c.interval[1] && inside_t(c.interval[0]) && inside_t(c.interval[1])) {
        return Err(bad("curve interval"));
    }
    e.nums(&c.interval)?;
    e.num(nonneg(c.tolerance)?)?;
    if c.curve_nodes.len() > MAX_NODES {
        return Err(bad("too many curve nodes"));
    }
    e.int(c.curve_nodes.len());
    for n in &c.curve_nodes {
        if n.aux > MAX_AUX_TYPE {
            return Err(bad("aux type"));
        }
        e.int(usize::from(n.aux));
        e.nums(&n.interval)?;
    }
    e.tree(&c.surface_nodes, nat)?;
    if c.solutions.len() > MAX_SOLUTIONS {
        return Err(bad("too many solutions"));
    }
    e.int(c.solutions.len());
    for s in &c.solutions {
        if s.kind > MAX_SOLUTION_TYPE || s.num_variables > 3 {
            return Err(bad("solution type"));
        }
        if !(inside_t(s.start[0]) && inside_t(s.end[0])) {
            return Err(bad("solution curve parameter outside the curve"));
        }
        if !(inside_uv(s.start[1], s.start[2]) && inside_uv(s.end[1], s.end[2])) {
            return Err(bad("solution surface parameter outside the surface"));
        }
        if s.node0.is_some_and(|k| k >= c.curve_nodes.len())
            || s.node1.is_some_and(|k| k >= c.surface_nodes.len())
        {
            return Err(bad("solution node out of range"));
        }
        e.int(usize::from(s.kind));
        e.int(usize::from(s.num_variables));
        e.num(s.start_value)?;
        e.nums(&s.start)?;
        e.num(s.end_value)?;
        e.nums(&s.end)?;
        e.link(s.node0);
        e.link(s.node1);
    }
    Ok(())
}

/// Encodes and validates `prog` (see the module docs for what is accepted).
fn encode(prog: &Program) -> Result<Vec<f64>, FfiError> {
    let mut e = Enc(Vec::new());
    e.int(match prog.op {
        Op::Lsi(_) => 1,
        Op::Combine(_) => 2,
    });
    let (nat, bspline) = e.surface(&prog.surface)?;
    match &prog.op {
        Op::Lsi(l) => encode_lsi(&mut e, l, nat)?,
        Op::Combine(c) => encode_combine(&mut e, c, nat, bspline)?,
    }
    Ok(e.0)
}

/// Runs `prog` (see the module docs) and returns the record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for inputs outside what the module docs accept,
/// [`FfiError::Status`] for a shim failure.
pub fn run(prog: &Program) -> Result<Vec<f64>, FfiError> {
    let input = encode(prog)?;
    let n_in = i64::try_from(input.len()).map_err(|_| bad("input too long"))?;
    call_with_buffer(4_096, |buf, len| {
        // SAFETY: input valid for n_in reads; buf valid for buf.len() writes; len valid.
        unsafe {
            sys::sc_surface_surface_p10_run(
                input.as_ptr(),
                n_in,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
