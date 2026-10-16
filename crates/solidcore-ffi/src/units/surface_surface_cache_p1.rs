//! FFI wrappers for unit `surface.surface_cache_p1`.
//!
//! Shim: `harness/cpp/shim/surface_surface_cache_p1.cpp` (`sc_surface_surface_cache_p1_eval`).
//! Upstream: `source/SMLib/src/SmSurfaceCache.cpp`, `source/SMLib/inc/SmSurfaceCache.h`.
//!
//! Every op but `Plane3` and `Cull` starts with a B-spline surface block `oob form n m p q r
//! s cpts(x y z w) u v` (`SmBSplineSurface(gw_SURFACE*, FALSE, NULL)`, control points with the
//! v index fastest). `V` is a vector `x y z`, `D` a domain `umin vmin umax vmax`, `C` a curve
//! `dim n p m cpts(x y z w) knots`, `K` alternate cache user data `hv V hb b hl l hd d hp p`
//! (`h*` = given), `T` a subdivision tree `n` then `n` x `parent c1 c2 aux split marked class
//! face D` (node 0 the top; `-1` = none). The op's parameters follow:
//!
//! | [`Op`]        | upstream                                     | parameters                    |
//! |---------------|----------------------------------------------|-------------------------------|
//! | `Plane3`      | `sm_ComputePlaneFrom3Points`                 | `V V V tol`                   |
//! | `Halfspace`   | `sm_CheckBoundingPlaneSeparatingHalfspace`   | `V V`                         |
//! | `BPlanes`     | `SmPatchBoundaryBoundingPlanes`: `CreateBoundaryCurveForIndex`, `GetBoundaryUVEndPoints`, `ComputeBoundingPlanes` | `D index` |
//! | `BezApprox`   | `sm_ComputeBezierApproximation`              | `D`                           |
//! | `SizeCorners` | `ComputeSizeTolerances` (corners)            | `V V V V`                     |
//! | `SizeSurf`    | `ComputeSizeTolerances` (surface)            | `D`                           |
//! | `Corners`     | `GetCorners`                                 | `D has [n m p q r s cpts u v]` |
//! | `Alt`         | `SmAltSrfCache`, `MatchCache`, `GetCopyOfCurves`, `HasCachedCurves` | `na` x (`type approx angle K n3 C.. nuv C..`) `nq` x (`type approx angle K w3 wuv`); `n = -1` no array |
//! | `TreeTop`     | `BuildTreeTop`, `GetPatchCount`, `GetPatchAt` | `nu nv` then `nu*nv` x `V V D` |
//! | `Subdiv`      | `FindUnmarkedLeafNode`, `GetConnectedRegion`, `MarkFaceContainment` | `T k` then `k` program ops |
//! | `Cont`        | `GetSurfaceContinuity`                       | `nu c.. nv c.. D`             |
//! | `Iso`         | `GetIsoBoundaryCurves`                       | `D k` then `k` x `D tol`      |
//! | `Polar`       | `SmBezierAux2d::GetPolarBox`                 | `T k` then `k` node indices   |
//! | `Misc`        | type queries, `PointTest`, `PointClassify`, `ImplantEdgeuse`, `ImplantPolyEdge`, ... | `D test uv` |
//! | `Face`        | `GetFace`                                    | -                             |
//! | `Cull`        | `sm_CullArray`                               | `n v..`                       |
//!
//! `Subdiv` program ops: `0 class k nodes..` (find), `1 node` (region), `2` (mark containment),
//! `3 node marked class face` (set), `4` (dump marks), `5 node` (`SmTreeNode::GetFirstBezierPatch`:
//! `0` none, `1` the surface's NURB, `2` another).
//!
//! The output is the op status (`0` = `SM_SUCCESS`, else the `SmStatus`) and the op's outputs
//! (see the shim). [`eval`] accepts only what upstream handles without undefined behaviour:
//! well-formed clamped surfaces and curves, domains inside the surface domain, valid
//! enumerators, bounded counts, finite values, well-formed subdivision trees (one root, child
//! pairs pointing back, every node a subdivision node), faces only on inside nodes (upstream
//! reads the tolerance of an unclassified node's face), continuity arrays no longer than the
//! knot lists.

use crate::{call_with_buffer, FfiError};

/// Largest control point index accepted by [`eval`].
pub const MAX_INDEX: usize = 32;
/// Largest NURB degree accepted by [`eval`].
pub const MAX_DEGREE: usize = 7;
/// Largest coordinate magnitude accepted by [`eval`].
pub const MAX_COORD: f64 = 1.0e6;
/// Largest number of subdivision tree nodes.
pub const MAX_NODES: usize = 256;
/// Number of fake object pointers (faces, user pointers).
pub const MAX_OBJECTS: i64 = 4096;
/// `NL_NOW`.
const NOW: f64 = -f64::MAX;
/// `NL_NOZ`.
const NOZ: f64 = f64::MAX;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `sm_ComputePlaneFrom3Points`.
    Plane3,
    /// `sm_CheckBoundingPlaneSeparatingHalfspace`.
    Halfspace,
    /// `SmPatchBoundaryBoundingPlanes`.
    BPlanes,
    /// `sm_ComputeBezierApproximation`.
    BezApprox,
    /// `ComputeSizeTolerances` (corner points).
    SizeCorners,
    /// `ComputeSizeTolerances` (surface).
    SizeSurf,
    /// `GetCorners`.
    Corners,
    /// The alternate caches.
    Alt,
    /// `BuildTreeTop`.
    TreeTop,
    /// Subdivision tree classification.
    Subdiv,
    /// `GetSurfaceContinuity`.
    Cont,
    /// `GetIsoBoundaryCurves`.
    Iso,
    /// `SmBezierAux2d::GetPolarBox`.
    Polar,
    /// Simple queries.
    Misc,
    /// `GetFace`.
    Face,
    /// `sm_CullArray`.
    Cull,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::Plane3 => 0,
            Op::Halfspace => 1,
            Op::BPlanes => 2,
            Op::BezApprox => 3,
            Op::SizeCorners => 4,
            Op::SizeSurf => 5,
            Op::Corners => 6,
            Op::Alt => 7,
            Op::TreeTop => 8,
            Op::Subdiv => 9,
            Op::Cont => 10,
            Op::Iso => 11,
            Op::Polar => 12,
            Op::Misc => 13,
            Op::Face => 14,
            Op::Cull => 15,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_surface_surface_cache_p1_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

pub(crate) fn bad(msg: impl Into<String>) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

/// Sequential reader over the input.
pub(crate) struct Reader<'a> {
    pub(crate) input: &'a [f64],
    pub(crate) at: usize,
}

impl Reader<'_> {
    pub(crate) fn num(&mut self) -> Result<f64, FfiError> {
        let v = self
            .input
            .get(self.at)
            .copied()
            .ok_or_else(|| bad("input too short"))?;
        self.at += 1;
        Ok(v)
    }

    pub(crate) fn finite(&mut self) -> Result<f64, FfiError> {
        let v = self.num()?;
        if !v.is_finite() || v.abs() > MAX_COORD {
            return Err(bad("non-finite or huge value"));
        }
        Ok(v)
    }

    pub(crate) fn finites(&mut self, k: usize) -> Result<Vec<f64>, FfiError> {
        (0..k).map(|_| self.finite()).collect()
    }

    pub(crate) fn vec3(&mut self) -> Result<[f64; 3], FfiError> {
        Ok([self.finite()?, self.finite()?, self.finite()?])
    }

    pub(crate) fn index(&mut self, max: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
            return Err(bad(format!("bad index/count {v} (max {max})")));
        }
        Ok(v as usize)
    }

    /// An index in `-1..max` (`-1` = none).
    pub(crate) fn opt_index(&mut self, max: i64) -> Result<i64, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < -1.0 || v >= max as f64 {
            return Err(bad(format!("bad index {v} (below {max})")));
        }
        Ok(v as i64)
    }

    pub(crate) fn flag(&mut self) -> Result<bool, FfiError> {
        Ok(self.index(1)? == 1)
    }

    pub(crate) fn domain(&mut self) -> Result<[f64; 4], FfiError> {
        let d = [
            self.finite()?,
            self.finite()?,
            self.finite()?,
            self.finite()?,
        ];
        if d[0] > d[2] || d[1] > d[3] {
            return Err(bad("domain not ordered"));
        }
        Ok(d)
    }

    /// A domain inside `outer`; with `strict` of positive size in both directions.
    pub(crate) fn domain_in(&mut self, outer: &Srf, strict: bool) -> Result<[f64; 4], FfiError> {
        let d = self.domain()?;
        let o = outer.dom;
        if d[0] < o[0] || d[1] < o[1] || d[2] > o[2] || d[3] > o[3] {
            return Err(bad("domain outside the surface domain"));
        }
        if strict && (d[2] - d[0] < 1e-6 * (o[2] - o[0]) || d[3] - d[1] < 1e-6 * (o[3] - o[1])) {
            return Err(bad("domain too small"));
        }
        Ok(d)
    }

    pub(crate) fn done(&self) -> Result<(), FfiError> {
        if self.at != self.input.len() {
            return Err(bad("trailing input"));
        }
        Ok(())
    }
}

/// Checks a clamped knot vector of degree `p`; returns the number of distinct knots.
fn check_knots(knots: &[f64], p: usize, what: &str) -> Result<(f64, f64, usize), FfiError> {
    let m = knots
        .len()
        .checked_sub(1)
        .ok_or_else(|| bad(format!("{what}: no knots")))?;
    if knots.windows(2).any(|w| w.first() > w.last()) {
        return Err(bad(format!("{what}: knots not non-decreasing")));
    }
    let (Some(&a), Some(&b)) = (knots.first(), knots.last()) else {
        return Err(bad(format!("{what}: empty knots")));
    };
    let short = || bad(format!("{what}: knots too short"));
    if m < 2 * p + 1 {
        return Err(short());
    }
    let head = knots.get(..=p).ok_or_else(short)?;
    let tail = knots.get(m - p..).ok_or_else(short)?;
    let inner = knots.get(p + 1..m - p).ok_or_else(short)?;
    if a >= b || head.iter().any(|&k| k != a) || tail.iter().any(|&k| k != b) {
        return Err(bad(format!("{what}: knots not clamped")));
    }
    if b - a < 1e-6 * a.abs().max(b.abs()).max(1.0) {
        return Err(bad(format!("{what}: parameter range too small")));
    }
    if inner.iter().any(|&k| k == a || k == b) {
        return Err(bad(format!("{what}: end multiplicity above degree + 1")));
    }
    let mut run = 1;
    for w in inner.windows(2) {
        run = if w.first() == w.last() { run + 1 } else { 1 };
        if run > p {
            return Err(bad(format!(
                "{what}: interior multiplicity above the degree"
            )));
        }
    }
    let distinct = 1 + knots.windows(2).filter(|w| w.first() != w.last()).count();
    Ok((a, b, distinct))
}

/// Checks 3D control points sharing their rational marker.
fn check_points(rd: &mut Reader<'_>, k: usize, what: &str) -> Result<(), FfiError> {
    let mut w0 = None;
    for _ in 0..k {
        let x = rd.num()?;
        let y = rd.num()?;
        let z = rd.num()?;
        let w = rd.num()?;
        if z == NOZ {
            return Err(bad(format!("{what}: 2D data")));
        }
        let rational = w != NOW;
        if *w0.get_or_insert(rational) != rational {
            return Err(bad(format!("{what}: mixed rationality")));
        }
        if rational && !(1e-3..=1e3).contains(&w) {
            return Err(bad(format!("{what}: weight out of range")));
        }
        if [x, y, z]
            .iter()
            .any(|c| !c.is_finite() || c.abs() > MAX_COORD)
        {
            return Err(bad(format!("{what}: point out of range")));
        }
    }
    Ok(())
}

/// A validated surface: its domain and distinct knot counts.
pub(crate) struct Srf {
    pub(crate) dom: [f64; 4],
    pub(crate) u_distinct: usize,
    pub(crate) v_distinct: usize,
}

/// A surface `n m p q r s cpts u v`.
pub(crate) fn surface(rd: &mut Reader<'_>) -> Result<Srf, FfiError> {
    let n = rd.index(MAX_INDEX)?;
    let m = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let q = rd.index(MAX_DEGREE)?;
    let r = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    let s = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || q == 0 || q > m || r != n + p + 1 || s != m + q + 1 {
        return Err(bad(
            "surface: need 1 <= p <= n, 1 <= q <= m, r = n+p+1, s = m+q+1",
        ));
    }
    check_points(rd, (n + 1) * (m + 1), "surface")?;
    let u = rd.finites(r + 1)?;
    let v = rd.finites(s + 1)?;
    let (u0, u1, ud) = check_knots(&u, p, "surface u")?;
    let (v0, v1, vd) = check_knots(&v, q, "surface v")?;
    Ok(Srf {
        dom: [u0, v0, u1, v1],
        u_distinct: ud,
        v_distinct: vd,
    })
}

/// A B-spline surface block `oob form surface`.
pub(crate) fn bspline_block(rd: &mut Reader<'_>) -> Result<Srf, FfiError> {
    rd.flag()?;
    rd.index(11)?;
    surface(rd)
}

/// A 3D or 2D curve `dim n p m cpts knots`.
pub(crate) fn curve(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("curve: dimension must be 2 or 3"));
    }
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad("curve: need 1 <= p <= n and m = n + p + 1"));
    }
    check_points(rd, n + 1, "curve")?;
    let knots = rd.finites(m + 1)?;
    check_knots(&knots, p, "curve").map(|_| ())
}

/// Alternate cache user data `hv V hb b hl l hd d hp p`.
pub(crate) fn key(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    rd.flag()?;
    rd.vec3()?;
    rd.flag()?;
    rd.flag()?;
    rd.flag()?;
    let l = rd.num()?;
    if l.fract() != 0.0 || l.abs() > 2.0e9 {
        return Err(bad("user long out of range"));
    }
    rd.flag()?;
    rd.finite()?;
    rd.flag()?;
    rd.opt_index(MAX_OBJECTS)?;
    Ok(())
}

/// `k` then `k` curves; `-1` = no array.
pub(crate) fn curves(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let k = rd.opt_index(9)?;
    for _ in 0..k.max(0) {
        curve(rd)?;
    }
    Ok(())
}

/// A node class `0..=3`; returns it.
fn class(rd: &mut Reader<'_>) -> Result<usize, FfiError> {
    rd.index(3)
}

/// A face for a node of class `class`: only inside nodes may have one.
fn face_for(rd: &mut Reader<'_>, class: usize) -> Result<(), FfiError> {
    let f = rd.opt_index(MAX_OBJECTS)?;
    if f >= 0 && class != 1 {
        return Err(bad("only inside nodes may have a face"));
    }
    Ok(())
}

/// A subdivision tree; returns its node count.
fn tree(rd: &mut Reader<'_>, srf: &Srf) -> Result<usize, FfiError> {
    let n = rd.index(MAX_NODES)?;
    if n == 0 {
        return Err(bad("tree: no nodes"));
    }
    let mut links = Vec::with_capacity(n);
    for _ in 0..n {
        let parent = rd.opt_index(n as i64)?;
        let c1 = rd.opt_index(n as i64)?;
        let c2 = rd.opt_index(n as i64)?;
        let aux = rd.index(6)?;
        if !matches!(aux, 3 | 4 | 6) {
            return Err(bad("tree: aux type must be a surface subdivision type"));
        }
        rd.index(2)?;
        rd.flag()?;
        let c = class(rd)?;
        face_for(rd, c)?;
        rd.domain_in(srf, false)?;
        links.push((parent, c1, c2));
    }
    // one root (node 0), child pairs pointing back, every node reached once from the root
    let mut seen = vec![false; n];
    let mut stack = vec![0usize];
    if links.first().map(|l| l.0) != Some(-1) {
        return Err(bad("tree: node 0 must be the root"));
    }
    while let Some(k) = stack.pop() {
        let Some(s) = seen.get_mut(k) else {
            return Err(bad("tree: bad node"));
        };
        if *s {
            return Err(bad("tree: node reached twice"));
        }
        *s = true;
        let (_, c1, c2) = links.get(k).copied().ok_or_else(|| bad("tree: bad node"))?;
        match (c1, c2) {
            (-1, -1) => {}
            (a, b) if a >= 0 && b >= 0 && a != b => {
                for c in [a as usize, b as usize] {
                    if links.get(c).map(|l| l.0) != Some(k as i64) {
                        return Err(bad("tree: child does not point back"));
                    }
                    stack.push(c);
                }
            }
            _ => return Err(bad("tree: need two children or none")),
        }
    }
    if seen.iter().any(|s| !s) {
        return Err(bad("tree: unreachable node"));
    }
    Ok(n)
}

fn validate_op(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    if op == Op::Plane3 {
        for _ in 0..3 {
            rd.vec3()?;
        }
        let tol = rd.finite()?;
        if tol < 0.0 {
            return Err(bad("negative tolerance"));
        }
        return Ok(());
    }
    if op == Op::Cull {
        let n = rd.index(64)?;
        rd.finites(n)?;
        return Ok(());
    }
    let srf = bspline_block(rd)?;
    match op {
        Op::Plane3 | Op::Cull | Op::Face => {}
        Op::Halfspace => {
            rd.vec3()?;
            rd.vec3()?;
        }
        Op::BPlanes => {
            rd.domain_in(&srf, false)?;
            rd.index(3)?;
        }
        Op::BezApprox | Op::SizeSurf => {
            rd.domain_in(&srf, true)?;
        }
        Op::SizeCorners => {
            for _ in 0..4 {
                rd.vec3()?;
            }
        }
        Op::Corners => {
            rd.domain_in(&srf, false)?;
            if rd.flag()? {
                surface(rd)?;
            }
        }
        Op::Alt => {
            let na = rd.index(5)?;
            for _ in 0..na {
                rd.index(1)?;
                rd.finite()?;
                rd.finite()?;
                key(rd)?;
                curves(rd)?;
                curves(rd)?;
            }
            let nq = rd.index(16)?;
            for _ in 0..nq {
                rd.index(1)?;
                rd.finite()?;
                rd.finite()?;
                key(rd)?;
                rd.flag()?;
                rd.flag()?;
            }
        }
        Op::TreeTop => {
            let nu = rd.index(16)?;
            let nv = rd.index(16)?;
            if nu == 0 || nv == 0 {
                return Err(bad("tree top: empty grid"));
            }
            for _ in 0..nu * nv {
                rd.vec3()?;
                rd.vec3()?;
                rd.domain()?;
            }
        }
        Op::Subdiv => {
            let n = tree(rd, &srf)?;
            let k = rd.index(64)?;
            for _ in 0..k {
                match rd.index(5)? {
                    0 => {
                        class(rd)?;
                        let m = rd.index(2 * n)?;
                        for _ in 0..m {
                            rd.index(n - 1)?;
                        }
                    }
                    1 | 5 => {
                        rd.index(n - 1)?;
                    }
                    3 => {
                        rd.index(n - 1)?;
                        rd.flag()?;
                        let c = class(rd)?;
                        face_for(rd, c)?;
                    }
                    _ => {}
                }
            }
        }
        Op::Cont => {
            let nu = rd.index(srf.u_distinct)?;
            for _ in 0..nu {
                rd.index(13)?;
            }
            let nv = rd.index(srf.v_distinct)?;
            for _ in 0..nv {
                rd.index(13)?;
            }
            rd.domain()?;
        }
        Op::Iso => {
            rd.domain_in(&srf, true)?;
            let k = rd.index(6)?;
            for _ in 0..k {
                rd.domain_in(&srf, true)?;
                let tol = rd.finite()?;
                if tol < 0.0 {
                    return Err(bad("negative tolerance"));
                }
            }
        }
        Op::Polar => {
            let n = tree(rd, &srf)?;
            let k = rd.index(16)?;
            for _ in 0..k {
                rd.index(n - 1)?;
            }
        }
        Op::Misc => {
            rd.domain()?;
            rd.flag()?;
            rd.finite()?;
            rd.finite()?;
        }
    }
    Ok(())
}

/// Checks `input` for `op` (layouts in the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
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
    call_with_buffer(65536, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above: well-formed clamped surfaces and curves, domains
        // inside the surface domain, well-formed subdivision trees, every index, count and
        // enumerator the shim passes to upstream); buf is valid for buf.len() writes and the
        // shim honours the capacity; len is valid.
        unsafe {
            sys::sc_surface_surface_cache_p1_eval(
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
