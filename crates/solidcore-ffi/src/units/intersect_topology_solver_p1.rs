//! FFI wrappers for unit `intersect.topology_solver_p1`.
//!
//! Shim: `harness/cpp/shim/intersect_topology_solver_p1.cpp`
//! (`sc_intersect_topology_solver_p1_eval`). Upstream: `source/SMLib/src/SmTopologySolver.cpp`,
//! `source/SMLib/inc/SmTopologySolver.h`.
//!
//! Every record starts with the solver header `op req tol best has_at at nvec V[3]*nvec`
//! (an `SmTopologySolver(op, req, tol, best, nvec ? V : NULL, sols)`, then
//! `SetAtDistance(at)` when `has_at`). Every output starts with the status (`0` =
//! `SM_SUCCESS`, else the `SmStatus`); the rest:
//!
//! | [`Op`]  | upstream                                    | parameters after the header | outputs |
//! |---------|---------------------------------------------|-----------------------------|---------|
//! | `Ctor`  | constructor, `GetBestAnswerSoFar`           | -                           | `category projected has_best best` |
//! | `May`   | `MayContainAnswer`                          | `n (box1 box2)*n`           | `flag*n` |
//! | `Local` | `SolveIt` of a local solver, repeated       | objects solver `k (o1 o2)*k` | `done best_sq nsol sol*nsol` |
//! | `Trees` | `SolveTrees` / `SolveTwoTrees`              | objects solver tree tree `two` | `1 best_sq nsol sol*nsol` |
//!
//! A box is `min[3] max[3]`. The object table is `n (kind data)*n`: `0 P[3] tol` (a
//! free-standing `SmVertex` with that point and zone tolerance), `1 curve` (a curve block)
//! or `2 c a b` (an `SmEdge` over curve object `c` with interval `[a, b]`, the curve's owner
//! set to it). A curve block is `0 dim oob n p m cpts(x y z w) knots` (an `SmBSplineCurve`),
//! `1 dim P1 D1 P2 D2` (an `SmHermiteCurve`), `2 3 P V d0 d1 scale io` (a bounded `SmLine`),
//! `3 3 C X Y d0 d1 rx ry io` (an `SmEllipse`) or `4 3 C X Y d0 d1 r io` (an `SmCircle`), as
//! in unit `curve.curve_p5`. The solver is `kind swap P[3] pv has_ivl a b owner curve
//! cedge` with `kind` one of [`LocalKind`]: point solvers get `SetPoint(P)`,
//! `SetPointVertex(pv)` (when `pv >= 0`), the point/curve solvers `m_vInterval = [a, b]`
//! (when `has_ivl`) and `m_pCurveOwner = owner` (when `owner >= 0`), the curve solvers
//! `SetCurve(curve, [a, b])` and `SetCurveEdge(cedge)` (when `cedge >= 0`). Object
//! indices are table indices, `-1` for `NULL`. `done` counts the `SolveIt` calls made
//! (they stop at the first failing one, whose status is the record's). A tree is `0 P[3]
//! size` (a one-node tree whose object list holds `NULL`) or `1 box m (obj box)*m` (a
//! spatial tree). A solution `sol` is `type nvar nobj o0 o1 value s[nvar] [evalue
//! e[nvar]]` (the end for a range); objects are table indices, `-1` for `NULL`, `-2` for
//! another object.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour or runaway
//! work: well-formed curve blocks (as unit `curve.curve_p5`), finite bounded values,
//! non-negative tolerances, option vectors wherever upstream reads them without a `NULL`
//! check (longer than `1e-6` for the ray of `MayContainAnswer` and for every vector of
//! the curve solvers; zero or tiny elsewhere, where upstream is defined for them), objects of the class each solver
//! casts them to (a vertex, a curve, a curve with an edge; `NULL` only where upstream
//! checks it), intervals within the curves' ranges, and the operations
//! `SmCurve::GlobalPointSolve` / `GlobalCurveIntersect` / `GlobalCurveSolve` accept for
//! the curve solvers.

use crate::validate::{bad, euclid, NOW, NOZ};
use crate::{call_with_buffer, FfiError};

/// Largest control point index `n`.
pub const MAX_INDEX: usize = 64;
/// Largest degree.
pub const MAX_DEGREE: usize = 9;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1e8;
/// Parameters may lie this many interval lengths outside the curve's range.
pub const MAX_REL_DIST: f64 = 2.0;
/// Most objects in a table.
pub const MAX_OBJECTS: usize = 16;
/// Most `SolveIt` calls of a `Local` record.
pub const MAX_CALLS: usize = 16;
/// Most box pairs of a `May` record.
pub const MAX_BOXES: usize = 64;
/// Most objects in a spatial tree.
pub const MAX_TREE_OBJECTS: usize = 16;
/// `SM_BIG_DOUBLE`.
pub const BIG_DOUBLE: f64 = 1.0e20;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// The constructor's category and projection flag, `GetBestAnswerSoFar`.
    Ctor,
    /// `SmTopologySolver::MayContainAnswer` on box pairs.
    May,
    /// `SolveIt` of a local solver.
    Local,
    /// `SmTopologySolver::SolveTrees` / `SolveTwoTrees`.
    Trees,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::Ctor => 0,
            Op::May => 1,
            Op::Local => 2,
            Op::Trees => 3,
        }
    }
}

/// The local solver classes the shim builds (the solver record's `kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalKind {
    /// `SmTopoLocalSolver`.
    Base = 0,
    /// `SmTLSPoint`.
    Point = 1,
    /// `SmTLSPointVertex`.
    PointVertex = 2,
    /// `SmTLSVertexVertex`.
    VertexVertex = 3,
    /// `SmTLSPointCurve`.
    PointCurve = 4,
    /// `SmTLSPointEdge`.
    PointEdge = 5,
    /// `SmTLSVertexEdge`.
    VertexEdge = 6,
    /// `SmTLSCurve`.
    Curve = 7,
    /// `SmTLSCurveEdge`.
    CurveEdge = 8,
    /// `SmTLSEdgeEdge`.
    EdgeEdge = 9,
    /// `SmTLSCurveVertex` (owned by `topology_solver_p2`).
    CurveVertex = 10,
}

impl LocalKind {
    /// Every kind, in code order.
    pub const ALL: [Self; 11] = [
        Self::Base,
        Self::Point,
        Self::PointVertex,
        Self::VertexVertex,
        Self::PointCurve,
        Self::PointEdge,
        Self::VertexEdge,
        Self::Curve,
        Self::CurveEdge,
        Self::EdgeEdge,
        Self::CurveVertex,
    ];

    /// The kind with code `code`.
    #[must_use]
    pub fn from_code(code: usize) -> Option<Self> {
        Self::ALL.get(code).copied()
    }
}

mod sys {
    extern "C" {
        pub fn sc_intersect_topology_solver_p1_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

crate::reader::unit_reader!(MAX_COORD);

impl Reader<'_> {
    /// An object index below `n`, or `-1` (`None`).
    fn object(&mut self, n: usize) -> Result<Option<usize>, FfiError> {
        let v = self.num()?;
        if v == -1.0 {
            return Ok(None);
        }
        if v.fract() != 0.0 || v < 0.0 || v >= n as f64 {
            return Err(bad(format!("bad object index {v}")));
        }
        Ok(Some(v as usize))
    }

    /// A box `min max` with `min <= max`.
    fn bbox(&mut self) -> Result<(), FfiError> {
        let a = self.vec3()?;
        let b = self.vec3()?;
        if a.iter().zip(&b).any(|(x, y)| x > y) {
            return Err(bad("box with min > max"));
        }
        Ok(())
    }
}

/// A B-spline `n p m cpts knots`.
fn bspline(rd: &mut Reader<'_>) -> Result<(f64, f64), FfiError> {
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad("curve: need 1 <= p <= n and m = n + p + 1"));
    }
    let cps = rd.take(4 * (n + 1))?;
    let knots = rd.take(m + 1)?;
    let (cpts, _) = cps.as_chunks::<4>();
    let [_, _, z0, w0] = *cpts.first().ok_or_else(|| bad("empty curve"))?;
    for &[x, y, z, w] in cpts {
        if (z == NOZ) != (z0 == NOZ) || (w == NOW) != (w0 == NOW) {
            return Err(bad("curve: mixed dimension or rationality"));
        }
        if !x.is_finite()
            || !y.is_finite()
            || (z != NOZ && !z.is_finite())
            || (w != NOW && !w.is_finite())
        {
            return Err(bad("curve: non-finite control point"));
        }
        if w != NOW && w <= 1e-3 {
            return Err(bad("curve: non-positive weight"));
        }
        if euclid([x, y, z, w])
            .iter()
            .any(|c| !c.is_finite() || c.abs() > MAX_COORD)
        {
            return Err(bad("curve: Euclidean point out of range"));
        }
    }
    if knots.iter().any(|k| !k.is_finite() || k.abs() > MAX_COORD)
        || knots.windows(2).any(|w| w.first() > w.last())
    {
        return Err(bad("curve: knots not non-decreasing"));
    }
    let (Some(&a), Some(&b)) = (knots.first(), knots.last()) else {
        return Err(bad("empty knots"));
    };
    let short = || bad("curve: knots too short");
    let head = knots.get(..=p).ok_or_else(short)?;
    let tail = knots.get(m - p..).ok_or_else(short)?;
    let inner = knots.get(p + 1..m - p).ok_or_else(short)?;
    if a >= b || head.iter().any(|&k| k != a) || tail.iter().any(|&k| k != b) {
        return Err(bad("curve: knots not clamped"));
    }
    if b - a < 1e-6 * a.abs().max(b.abs()).max(1.0) {
        return Err(bad("curve: parameter range too small"));
    }
    if inner.iter().any(|&k| k == a || k == b) {
        return Err(bad("curve: end multiplicity above p + 1"));
    }
    let mut run = 1;
    for w in inner.windows(2) {
        run = if w.first() == w.last() { run + 1 } else { 1 };
        if run > p {
            return Err(bad("curve: interior multiplicity above the degree"));
        }
    }
    Ok((a, b))
}

/// A line `P V d0 d1 scale io`.
fn line(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    rd.vec3()?;
    rd.nonzero3()?;
    let d0 = rd.finite()?;
    let d1 = rd.finite()?;
    if (d1 - d0).abs() < 1e-6 {
        return Err(bad("line: domain too small"));
    }
    let scale = rd.finite()?;
    if scale <= 0.0 {
        return Err(bad("line: scale not positive"));
    }
    rd.flag()?;
    Ok(())
}

/// An ellipse or circle placement and domain `C X Y d0 d1`.
fn conic_frame(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    rd.vec3()?;
    let x = rd.nonzero3()?;
    let y = rd.nonzero3()?;
    let cross = [
        x[1] * y[2] - x[2] * y[1],
        x[2] * y[0] - x[0] * y[2],
        x[0] * y[1] - x[1] * y[0],
    ];
    if cross.iter().map(|c| c * c).sum::<f64>() < 1e-12 {
        return Err(bad("ellipse: parallel axes"));
    }
    let d0 = rd.finite()?;
    let d1 = rd.finite()?;
    if d0.abs() > 720.0 || d1.abs() > 720.0 {
        return Err(bad("ellipse: domain end beyond 720 degrees"));
    }
    if !(1e-4..=360.0).contains(&(d1 - d0)) {
        return Err(bad("ellipse: domain length outside [1e-4, 360]"));
    }
    Ok(())
}

fn radius(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let r = rd.finite()?;
    if !(1e-6..=1e6).contains(&r) {
        return Err(bad("ellipse: radius outside [1e-6, 1e6]"));
    }
    Ok(())
}

/// The parameter range of a validated curve block.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CurveInfo {
    a: f64,
    b: f64,
}

/// A curve block (see the module docs).
fn curve_block(rd: &mut Reader<'_>) -> Result<CurveInfo, FfiError> {
    let kind = rd.index(4)?;
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("curve block: dimension must be 2 or 3"));
    }
    match kind {
        0 => {
            rd.flag()?;
            let (a, b) = bspline(rd)?;
            Ok(CurveInfo { a, b })
        }
        1 => {
            for _ in 0..4 {
                rd.vec3()?;
            }
            Ok(CurveInfo { a: 0.0, b: 1.0 })
        }
        2 => {
            if dim != 3 {
                return Err(bad("line block: dimension must be 3"));
            }
            // a line evaluates anywhere: only bound its parameters
            line(rd)?;
            Ok(CurveInfo {
                a: -MAX_COORD / 8.0,
                b: MAX_COORD / 8.0,
            })
        }
        _ => {
            if dim != 3 {
                return Err(bad("ellipse block: dimension must be 3"));
            }
            conic_frame(rd)?;
            radius(rd)?;
            if kind == 3 {
                radius(rd)?;
            }
            rd.flag()?;
            Ok(CurveInfo { a: -1e3, b: 1e3 })
        }
    }
}

/// An increasing interval `a b` of parameters within [`MAX_REL_DIST`] ranges of `c`'s.
fn interval(rd: &mut Reader<'_>, c: &CurveInfo) -> Result<(), FfiError> {
    let far = MAX_REL_DIST * (c.b - c.a);
    let a = rd.finite()?;
    let b = rd.finite()?;
    if a < c.a - far || b > c.b + far {
        return Err(bad("interval too far outside the curve"));
    }
    if a >= b {
        return Err(bad("interval not increasing"));
    }
    Ok(())
}

/// What an object table entry is.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Entry {
    Vertex,
    /// A curve, its range and whether an edge owns it.
    Curve(CurveInfo, bool),
    Edge,
}

/// The validated solver header.
#[derive(Debug, Clone, Copy)]
struct Header {
    /// `eSolverOperation` code.
    op: usize,
    /// Number of option vectors.
    nvec: usize,
    /// Whether the first option vector is shorter than `1e-6` (or there is none).
    short_first: bool,
    /// Whether any option vector is shorter than `1e-6`.
    any_short: bool,
}

impl Header {
    /// `MayContainAnswer`'s `SM_SO_RAYFIRE` branch intersects the box with the ray
    /// `cpOptVectors[0]` through `SmExtent3d::IntersectLine`, which fails for a ray
    /// shorter than `SM_EFF_ZERO` and leaves the results it is read for
    /// uninitialised. Every other read of the vectors in this unit is defined for a
    /// zero or tiny vector (`smgu_LineClosestPoint` gives `0`,
    /// `smgu_PointProjectToPlane` NaN), so only this one is refused (with a margin).
    fn short_ray(&self) -> bool {
        self.op == RAYFIRE && self.short_first
    }
}

/// `SM_SO_RAYFIRE`.
const RAYFIRE: usize = 8;

/// Whether `v` is shorter than `1e-6`.
fn is_short(v: [f64; 3]) -> bool {
    v.iter().map(|c| c * c).sum::<f64>() < 1e-12
}

/// The solver header.
fn header(rd: &mut Reader<'_>) -> Result<Header, FfiError> {
    let op = rd.index(24)?;
    rd.index(3)?;
    let tol = rd.finite()?;
    if tol < 0.0 {
        return Err(bad("negative tolerance"));
    }
    let best = rd.num()?;
    if !best.is_finite() || best.abs() > BIG_DOUBLE {
        return Err(bad("best answer not finite or beyond SM_BIG_DOUBLE"));
    }
    rd.flag()?;
    rd.finite()?;
    let nvec = rd.index(3)?;
    let mut short_first = true;
    let mut any_short = false;
    for i in 0..nvec {
        let short = is_short(rd.vec3()?);
        if i == 0 {
            short_first = short;
        }
        any_short |= short;
    }
    Ok(Header {
        op,
        nvec,
        short_first,
        any_short,
    })
}

/// Option vectors `MayContainAnswer` and the local solvers read for `op` (the
/// projected, directed, ray and 3D signed operations).
fn vectors_needed(op: usize) -> usize {
    match op {
        // signed directed, directed min / max, signed angle minimize
        11..=13 | 21 => 2,
        // ray fire, 3D signed directed minimize, projected min / max, angle minimize
        8 | 9 | 14 | 15 | 20 => 1,
        _ => 0,
    }
}

/// Operations the curve point solvers accept (`SmCurve::GlobalPointSolve` without a
/// `NULL` vector check, among the operations of the topology solver constructor).
fn point_curve_op(op: usize) -> bool {
    matches!(op, 0 | 3..=6 | 8 | 9 | 11..=15)
}

/// Operations the curve/curve solvers accept: intersections and the plain distance
/// extrema of `SmCurve::GlobalCurveSolve`.
fn curve_curve_op(op: usize) -> bool {
    matches!(op, 0 | 3 | 4)
}

fn objects(rd: &mut Reader<'_>) -> Result<Vec<Entry>, FfiError> {
    let n = rd.index(MAX_OBJECTS)?;
    let mut out: Vec<Entry> = Vec::new();
    for _ in 0..n {
        match rd.index(2)? {
            0 => {
                rd.vec3()?;
                if rd.finite()? < 0.0 {
                    return Err(bad("negative vertex tolerance"));
                }
                out.push(Entry::Vertex);
            }
            1 => {
                let c = curve_block(rd)?;
                out.push(Entry::Curve(c, false));
            }
            _ => {
                let c = rd
                    .object(out.len())?
                    .ok_or_else(|| bad("edge without a curve"))?;
                let Some(Entry::Curve(info, owned)) = out.get_mut(c) else {
                    return Err(bad("edge curve is not a curve"));
                };
                if *owned {
                    return Err(bad("curve already owned by an edge"));
                }
                *owned = true;
                let info = *info;
                interval(rd, &info)?;
                out.push(Entry::Edge);
            }
        }
    }
    Ok(out)
}

fn entry(objs: &[Entry], i: Option<usize>) -> Option<Entry> {
    i.and_then(|i| objs.get(i).copied())
}

fn is_vertex(e: Option<Entry>) -> bool {
    e == Some(Entry::Vertex)
}

fn is_curve(e: Option<Entry>) -> bool {
    matches!(e, Some(Entry::Curve(..)))
}

fn is_edge_curve(e: Option<Entry>) -> bool {
    matches!(e, Some(Entry::Curve(_, true)))
}

/// The validated solver record.
#[derive(Debug, Clone, Copy)]
struct Spec {
    kind: LocalKind,
    swap: bool,
    /// `m_vInterval` when set.
    interval: Option<(f64, f64)>,
}

fn solver(rd: &mut Reader<'_>, objs: &[Entry], hd: Header) -> Result<Spec, FfiError> {
    let (op, nvec) = (hd.op, hd.nvec);
    let kind = LocalKind::from_code(rd.index(10)?).ok_or_else(|| bad("solver kind"))?;
    let swap = rd.flag()?;
    rd.vec3()?;
    let pv = rd.object(objs.len())?;
    if pv.is_some() && !is_vertex(entry(objs, pv)) {
        return Err(bad("point vertex is not a vertex"));
    }
    let has_ivl = rd.flag()?;
    let a = rd.finite()?;
    let b = rd.finite()?;
    let owner = rd.object(objs.len())?;
    let curve = rd.object(objs.len())?;
    let cedge = rd.object(objs.len())?;
    if cedge.is_some() && entry(objs, cedge) != Some(Entry::Edge) {
        return Err(bad("curve edge is not an edge"));
    }
    if nvec < vectors_needed(op) {
        return Err(bad("operation needs more option vectors"));
    }
    let point_curve = matches!(
        kind,
        LocalKind::PointCurve
            | LocalKind::PointEdge
            | LocalKind::VertexEdge
            | LocalKind::CurveVertex
    );
    if point_curve && !point_curve_op(op) {
        return Err(bad("operation not accepted by GlobalPointSolve"));
    }
    // the curve solvers hand the vectors to SmCurve::GlobalPointSolve /
    // GlobalCurveSolve, whose behaviour for a zero or tiny vector is not established
    if (point_curve || matches!(kind, LocalKind::CurveEdge | LocalKind::EdgeEdge)) && hd.any_short {
        return Err(bad("option vector too short for the curve solvers"));
    }
    let curve_curve = matches!(kind, LocalKind::CurveEdge | LocalKind::EdgeEdge);
    if curve_curve && !curve_curve_op(op) {
        return Err(bad("operation not accepted by the curve/curve solvers"));
    }
    if kind == LocalKind::PointCurve && !has_ivl {
        return Err(bad("point/curve solver needs an interval"));
    }
    if has_ivl && a >= b {
        return Err(bad("interval not increasing"));
    }
    if owner.is_some() && kind != LocalKind::PointCurve {
        return Err(bad("curve owner only for the point/curve solver"));
    }
    if matches!(kind, LocalKind::CurveEdge | LocalKind::CurveVertex) {
        let Some(Entry::Curve(info, _)) = entry(objs, curve) else {
            return Err(bad("curve/edge solver needs a curve"));
        };
        if !has_ivl {
            return Err(bad("curve/edge solver needs an interval"));
        }
        let far = MAX_REL_DIST * (info.b - info.a);
        if a < info.a - far || b > info.b + far {
            return Err(bad("interval too far outside the curve"));
        }
    } else if curve.is_some() || cedge.is_some() {
        return Err(bad("curve and curve edge only for the curve/edge solver"));
    }
    Ok(Spec {
        kind,
        swap,
        interval: has_ivl.then_some((a, b)),
    })
}

/// Checks the objects of one `SolveIt(o1, o2)` call of `spec` (and, for the point/curve
/// solver, its interval against the curve).
fn call(spec: Spec, objs: &[Entry], o1: Option<usize>, o2: Option<usize>) -> Result<(), FfiError> {
    let (first, second) = if spec.swap {
        (entry(objs, o2), entry(objs, o1))
    } else {
        (entry(objs, o1), entry(objs, o2))
    };
    let ok = match spec.kind {
        // the abstract solvers read nothing
        LocalKind::Base | LocalKind::Point | LocalKind::Curve => true,
        // NER(pV) checks the vertex for NULL
        LocalKind::PointVertex => second.is_none() || is_vertex(second),
        // The release `SM_ASSERT(pObjN->IsKindOf(..))` of `SmTLSVertexVertex::SolveIt` and
        // `SmTLSEdgeEdge::SolveIt` still evaluates its argument, so both objects are
        // dereferenced before any `NULL` check (UB-238).
        LocalKind::VertexVertex => is_vertex(first) && is_vertex(second),
        LocalKind::PointCurve => match (second, spec.interval) {
            // the interval must lie near the curve's range
            (Some(Entry::Curve(info, _)), Some((a, b))) => {
                let far = MAX_REL_DIST * (info.b - info.a);
                a >= info.a - far && b <= info.b + far
            }
            _ => false,
        },
        LocalKind::PointEdge => is_edge_curve(second),
        LocalKind::VertexEdge => is_vertex(first) && is_edge_curve(second),
        // CurveVertex always reads pObj2 as the vertex, independently of swap.
        LocalKind::CurveVertex => is_vertex(entry(objs, o2)),
        // NER(pCurve) checks the second curve for NULL
        LocalKind::CurveEdge => second.is_none() || is_edge_curve(second),
        LocalKind::EdgeEdge => is_edge_curve(first) && is_edge_curve(second),
    };
    if !ok {
        return Err(bad("call objects do not fit the solver"));
    }
    Ok(())
}

/// A tree whose objects are passed to `spec` as the first (`first`) or second object.
fn tree(rd: &mut Reader<'_>, objs: &[Entry], spec: Spec, first: bool) -> Result<(), FfiError> {
    if rd.index(1)? == 0 {
        rd.vec3()?;
        if rd.finite()? < 0.0 {
            return Err(bad("negative point size"));
        }
        // the point tree's object is NULL
        let ok = first
            && matches!(
                spec.kind,
                LocalKind::PointVertex | LocalKind::PointCurve | LocalKind::PointEdge
            );
        if !ok {
            return Err(bad("point tree objects do not fit the solver"));
        }
        return Ok(());
    }
    rd.bbox()?;
    let m = rd.index(MAX_TREE_OBJECTS)?;
    for _ in 0..m {
        let o = rd.object(objs.len())?;
        let e = entry(objs, o);
        let ok = match (spec.kind, first) {
            (LocalKind::PointVertex, false) => o.is_none() || is_vertex(e),
            (LocalKind::PointCurve, false) => is_curve(e),
            (LocalKind::PointEdge, false) | (LocalKind::EdgeEdge, _) => is_edge_curve(e),
            _ => false,
        };
        if !ok {
            return Err(bad("tree objects do not fit the solver"));
        }
        rd.bbox()?;
    }
    Ok(())
}

fn validate_op(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let hd = header(rd)?;
    let (sop, nvec) = (hd.op, hd.nvec);
    match op {
        Op::Ctor => {}
        Op::May => {
            if nvec < vectors_needed(sop) {
                return Err(bad("operation needs more option vectors"));
            }
            if hd.short_ray() {
                return Err(bad("ray shorter than 1e-6 in MayContainAnswer"));
            }
            let n = rd.index(MAX_BOXES)?;
            for _ in 0..n {
                rd.bbox()?;
                rd.bbox()?;
            }
        }
        Op::Local => {
            let objs = objects(rd)?;
            let spec = solver(rd, &objs, hd)?;
            let k = rd.index(MAX_CALLS)?;
            for _ in 0..k {
                let o1 = rd.object(objs.len())?;
                let o2 = rd.object(objs.len())?;
                call(spec, &objs, o1, o2)?;
            }
        }
        Op::Trees => {
            let objs = objects(rd)?;
            let spec = solver(rd, &objs, hd)?;
            if spec.swap
                || !matches!(
                    spec.kind,
                    LocalKind::PointVertex
                        | LocalKind::PointCurve
                        | LocalKind::PointEdge
                        | LocalKind::EdgeEdge
                )
            {
                return Err(bad(
                    "tree solves only for unswapped point, curve and edge solvers",
                ));
            }
            // SolveTrees reaches MayContainAnswer
            if hd.short_ray() {
                return Err(bad("ray shorter than 1e-6 in MayContainAnswer"));
            }
            tree(rd, &objs, spec, true)?;
            tree(rd, &objs, spec, false)?;
            rd.flag()?;
        }
    }
    Ok(())
}

/// Checks that `input` is a well-formed record for `op` (see the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] describing the first problem.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    validate_op(op, &mut rd)?;
    rd.done()
}

/// Runs `op` on `input` (layouts in the module docs); returns the shim's output record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(1024, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above: the header, well-formed curve blocks, object
        // indices of the right classes, every count and flag the shim passes to
        // upstream); buf is valid for buf.len() writes and the shim honours the capacity;
        // len is valid.
        unsafe {
            sys::sc_intersect_topology_solver_p1_eval(
                op.code(),
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(kind: LocalKind, swap: bool) -> Spec {
        Spec {
            kind,
            swap,
            interval: None,
        }
    }

    #[test]
    fn vertex_and_edge_pair_solvers_refuse_a_null_object() {
        let vertices = [Entry::Vertex, Entry::Vertex];
        let edge = Entry::Curve(CurveInfo { a: 0.0, b: 1.0 }, true);
        let edges = [edge, edge];
        for swap in [false, true] {
            let vv = spec(LocalKind::VertexVertex, swap);
            assert!(call(vv, &vertices, Some(0), Some(1)).is_ok());
            assert!(call(vv, &vertices, Some(0), None).is_err());
            assert!(call(vv, &vertices, None, Some(1)).is_err());
            let ee = spec(LocalKind::EdgeEdge, swap);
            assert!(call(ee, &edges, Some(0), Some(1)).is_ok());
            assert!(call(ee, &edges, Some(0), None).is_err());
            assert!(call(ee, &edges, None, Some(1)).is_err());
            // NER(pV) / NER(pCurve) still accept a NULL second object
            let pv = spec(LocalKind::PointVertex, swap);
            let (o1, o2) = if swap { (None, Some(0)) } else { (Some(0), None) };
            assert!(call(pv, &vertices, o1, o2).is_ok());
        }
    }
}
