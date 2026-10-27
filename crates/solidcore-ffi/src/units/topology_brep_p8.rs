//! FFI wrappers for unit `topology.brep_p8`.
//!
//! Shim: `harness/cpp/shim/topology_brep_p8.cpp` (`sc_topology_brep_p8_eval`).
//! Upstream: `source/SMLib/src/SmBrep.cpp`.
//!
//! Every op runs on a new `SmBrep` (its void infinite region). A surface block is `-1`
//! (null) or `sc_topology_fixture.h`'s `read_surface` block: `1 O[3] N[3]` (an `SmPlane`)
//! or `0 oob form n m p q r s cpts knots_u knots_v` (an `SmBSplineSurface`). A curve
//! block is `0` + `read_curve`'s `dim n p m cpts(x y z w)*(n+1) knots*(m+1)` (an
//! `SmBSplineCurve`; `w = NOW` non-rational, `z = NOZ` in 2D), `1 P0[3] P1[3]` (an
//! `SmLine(P0, P1, 3)`) or `2` + `read_curve`'s block (the `SmBSplineCurve` with the
//! given knots written back over its own NURB after construction, undoing the
//! constructor's knot fix-up; refused when `sm_GetKnots` would merge two distinct knots,
//! UB-266). A UV curve block is `read_curve`'s block alone.
//!
//! | [`Op`]             | input | results |
//! |--------------------|-------|---------|
//! | `InRegion`         | `surface region_null u0 v0 u1 v1` | `shell face` |
//! | `FromSurface`      | `surface u0 v0 u1 v1` | `face` |
//! | `FacesFromSurface` | `surface continuity` (the domain is the natural one; unused upstream) | `n face*` |
//! | `PlanarFace`       | `region_null tol n curve*` | `face` |
//! | `PlanarFaces`      | `region_null tol n curve*` | `n face*` |
//! | `FacesWithCurves`  | `surface region_null nloops count* has3d ncurves (curve orient)*` | `shell n face*` |
//! | `MakeFaces`        | `surface u0 v0 u1 v1 orient region_null nloops count* has3d has_uv ncurves (curve3d? curveuv? orient)* npts pt[3]*` | `region shell n face*` |
//!
//! Output: `status` (`0` success, else the `SmStatus`), after a success the results
//! (objects as indices in the walk of the B-rep after the op, `-1` null), then the B-rep after the op (also after
//! a failure): `faces edges vertices regions`, per region `void infinite nshells` and per
//! shell `type size has_head`, the graph (the ten kind counts, then per region `void
//! infinite nshells shell*`, per shell `type size region head`, per face use `face mate
//! shell next last orient nloopuses loopuse*`, per face `faceuse`, per loop use `loop mate
//! faceuse type start orient`, per loop `loopuse`, per edge use `edge owner_type owner cw
//! ccw vertexuse next last orient`, per edge `n edgeuse*`, per vertex use `vertex
//! owner_type owner`, per vertex `n vertexuse*`), then the geometry: per vertex `x y z
//! tol`, per edge `tol t0 t1 curve`, per face `tol u0 v0 u1 v1 rect surface`, per edge use
//! its UV trim curve. A geometry curve is `-1` or `type t0 t1 (ok x y z)*5` (samples at
//! `t0 + i/4 (t1 - t0)`), a surface `-1` or `type u0 v0 u1 v1 (ok x y z)*9` (samples at
//! fractions 0, 1/2, 1 of the face's domain, u outer).
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: clamped
//! B-splines of degree 1 to 5 with at most 33 control points, positive (or `NOW`)
//! weights and finite values, and planes with a non-zero normal. It rejects the inputs
//! upstream dereferences null or reads past an array for: a null surface for
//! `FacesWithCurves` (its `GetNaturalUVDomain` is read before any check) and for
//! `MakeFaces` (`FindKnotsByMaxContinuity`), and loop counts adding up to more than the
//! curves given (with a curve array) for `FacesWithCurves` and `MakeFaces`. Null curve
//! arrays, null surfaces of the other ops and degenerate lines are checked upstream and
//! pass.

use crate::{call_with_buffer, FfiError};

/// Largest control point index.
pub const MAX_INDEX: usize = 32;
/// Largest degree.
pub const MAX_DEGREE: usize = 5;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1.0e12;
/// Largest number of curves (and loops, loop points).
pub const MAX_CURVES: usize = 64;

/// `NOW`: the weight of a non-rational control point.
const NOW: f64 = -f64::MAX;
/// `NOZ`: the z of a 2D control point.
const NOZ: f64 = f64::MAX;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    InRegion = 1,
    FromSurface = 2,
    FacesFromSurface = 3,
    PlanarFace = 4,
    PlanarFaces = 5,
    FacesWithCurves = 6,
    MakeFaces = 7,
}

mod sys {
    extern "C" {
        pub fn sc_topology_brep_p8_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

fn bad(msg: impl Into<String>) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

crate::reader::unit_reader!(MAX_COORD);

impl Reader<'_> {
    fn orient(&mut self) -> Result<(), FfiError> {
        // SM_OT_UNKNOWN (0), SM_OT_SAME (1) or SM_OT_OPPOSITE (2): upstream tests for
        // SAME or OPPOSITE and treats anything else as the other branch
        self.index(2).map(|_| ())
    }
}

/// Checks a clamped, non-decreasing knot vector of degree `p` with a non-empty domain.
fn check_knots(knots: &[f64], p: usize) -> Result<(), FfiError> {
    if knots.windows(2).any(|w| w.first() > w.last()) {
        return Err(bad("knots not non-decreasing"));
    }
    let (Some(&a), Some(&b)) = (knots.first(), knots.last()) else {
        return Err(bad("no knots"));
    };
    if b.partial_cmp(&a) != Some(core::cmp::Ordering::Greater) {
        return Err(bad("empty knot domain"));
    }
    let lo = knots.iter().take(p + 1).all(|&k| k == a);
    let hi = knots.iter().rev().take(p + 1).all(|&k| k == b);
    if !lo || !hi {
        return Err(bad("knots not clamped"));
    }
    Ok(())
}

fn points(rd: &mut Reader<'_>, count: usize, dim2: bool) -> Result<(), FfiError> {
    for _ in 0..count {
        rd.finite()?;
        rd.finite()?;
        if dim2 {
            // a 2D control point has z = NOZ
            if rd.num()? != NOZ {
                return Err(bad("2D control point z must be NOZ"));
            }
        } else {
            rd.finite()?;
        }
        let w = rd.num()?;
        if w != NOW && !(w.is_finite() && w > 0.0) {
            return Err(bad("weight must be NOW or positive"));
        }
    }
    Ok(())
}

/// `read_curve`'s block.
fn bspline_curve(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    bspline_knots(rd).map(drop)
}

/// `read_curve`'s block; its knots.
fn bspline_knots(rd: &mut Reader<'_>) -> Result<Vec<f64>, FfiError> {
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("curve dimension 2 or 3"));
    }
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad("curve: need 1 <= p <= n, m = n+p+1"));
    }
    points(rd, n + 1, dim == 2)?;
    let knots = (0..=m)
        .map(|_| rd.finite())
        .collect::<Result<Vec<_>, _>>()?;
    check_knots(&knots, p)?;
    Ok(knots)
}

/// A 3D curve block (`0` B-spline, `1` line, `2` B-spline with its given knots written
/// back after construction).
fn curve(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let kind = rd.index(2)?;
    if kind == 2 {
        return raw_bspline_curve(rd);
    }
    if kind == 1 {
        // (a line with ends closer than SM_EFF_ZERO is the degenerate line SmLine makes)
        for _ in 0..6 {
            rd.finite()?;
        }
        return Ok(());
    }
    bspline_curve(rd)
}

/// A curve block of kind `2`: `read_curve`'s block, whose knots the shim writes back over
/// the curve's own NURB. Refused when `sm_GetKnots` would count a knot of another value
/// as a duplicate of the last unique one (`SM_IS_ZERO`: `|d| <= SM_EFF_ZERO`): the
/// continuities then have fewer entries than the Bezier spans and
/// `SmCurveCache::Tessellate` reads past them (UB-266).
fn raw_bspline_curve(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let knots = bspline_knots(rd)?;
    let mut unique: Option<f64> = None;
    for &u in &knots {
        match unique {
            Some(last) if (last - u).abs() <= 1.0e-12 => {
                if u != last {
                    return Err(bad("distinct knots within SM_EFF_ZERO (UB-266)"));
                }
            }
            _ => unique = Some(u),
        }
    }
    Ok(())
}

/// A surface block; `false` for a null surface.
fn surface(rd: &mut Reader<'_>) -> Result<bool, FfiError> {
    let kind = rd.num()?;
    if kind == -1.0 {
        return Ok(false);
    }
    rd.at -= 1;
    if rd.index(1)? == 1 {
        for _ in 0..3 {
            rd.finite()?;
        }
        let n = [rd.finite()?, rd.finite()?, rd.finite()?];
        // (SmPlane's precondition: LengthSquared() > SM_EFF_ZERO_SQ, SmPlane.cpp)
        if n.iter().map(|c| c * c).sum::<f64>() <= 1.0e-24 {
            return Err(bad("zero plane normal"));
        }
        return Ok(true);
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
        return Err(bad(
            "surface: need 1 <= p <= n, 1 <= q <= m, r = n+p+1, s = m+q+1",
        ));
    }
    points(rd, (n + 1) * (m + 1), false)?;
    let u = (0..=r)
        .map(|_| rd.finite())
        .collect::<Result<Vec<_>, _>>()?;
    let v = (0..=s)
        .map(|_| rd.finite())
        .collect::<Result<Vec<_>, _>>()?;
    check_knots(&u, p)?;
    check_knots(&v, q)?;
    Ok(true)
}

/// `nloops count*`: the total of the counts.
fn loop_counts(rd: &mut Reader<'_>) -> Result<usize, FfiError> {
    let nl = rd.index(MAX_CURVES)?;
    let mut total = 0usize;
    for _ in 0..nl {
        total += rd.index(MAX_CURVES)?;
    }
    Ok(total)
}

/// Runs `op` on `input` (see the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed input or one upstream does not handle
/// safely; [`FfiError::Status`] for a shim failure.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let mut rd = Reader::new(input);
    match op {
        Op::InRegion | Op::FromSurface => {
            surface(&mut rd)?;
            if op == Op::InRegion {
                rd.flag()?;
            }
            for _ in 0..4 {
                rd.finite()?;
            }
        }
        Op::FacesFromSurface => {
            surface(&mut rd)?;
            // SmContinuityType: SM_CT_UNDEFINED (0) .. SM_CT_CINFINITY (13)
            rd.index(13)?;
        }
        Op::PlanarFace | Op::PlanarFaces => {
            rd.flag()?;
            rd.finite()?;
            let n = rd.index(MAX_CURVES)?;
            for _ in 0..n {
                curve(&mut rd)?;
            }
        }
        Op::FacesWithCurves => {
            if !surface(&mut rd)? {
                return Err(bad("null surface (upstream reads its domain first)"));
            }
            rd.flag()?;
            let total = loop_counts(&mut rd)?;
            let has3d = rd.flag()?;
            let nc = rd.index(MAX_CURVES)?;
            if has3d && total > nc {
                return Err(bad("loop counts exceed the curves"));
            }
            for _ in 0..nc {
                curve(&mut rd)?;
                rd.orient()?;
            }
        }
        Op::MakeFaces => {
            if !surface(&mut rd)? {
                return Err(bad("null surface (upstream dereferences it)"));
            }
            for _ in 0..4 {
                rd.finite()?;
            }
            rd.orient()?;
            rd.flag()?;
            let total = loop_counts(&mut rd)?;
            let has3d = rd.flag()?;
            let hasuv = rd.flag()?;
            let nc = rd.index(MAX_CURVES)?;
            // (without curve arrays upstream fails before reading the counts)
            if (has3d || hasuv) && total > nc {
                return Err(bad("loop counts exceed the curves"));
            }
            for _ in 0..nc {
                if has3d {
                    curve(&mut rd)?;
                }
                if hasuv {
                    bspline_curve(&mut rd)?;
                }
                rd.orient()?;
            }
            let np = rd.index(MAX_CURVES)?;
            for _ in 0..(3 * np) {
                rd.finite()?;
            }
        }
    }
    rd.done()?;
    raw(op, input)
}

fn raw(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds what the shim reads for
        // `op` (validated by `eval`); buf is valid for buf.len() writes and the shim
        // honours the capacity; len is valid.
        unsafe {
            sys::sc_topology_brep_p8_eval(
                op as i32,
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
