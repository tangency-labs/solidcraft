//! FFI wrappers for unit `nurbs.b_spline_surface_n_lib_p1`.
//!
//! Shim: `harness/cpp/shim/nurbs_b_spline_surface_n_lib_p1.cpp`
//! (`sc_nurbs_b_spline_surface_n_lib_p1_eval`).
//! Upstream: `source/SMLib/src/SmBSplineSurfaceNLib.cpp`.
//!
//! The input starts with a surface block: `ctor`, then for `ctor != 0` `oob form`, then
//! for `ctor == 2` a surface. `ctor` is `0` (static ops), `1` `SmBSplineSurface()` (no
//! NURB) or `2` `SmBSplineSurface(gw_SURFACE*, FALSE, NULL)`. A surface is
//! `n m p q r s`, `(n+1)(m+1)` control points `x y z w` (v index fastest), `r+1` u
//! knots, `s+1` v knots. A curve is `dim n p m`, `n+1` points `x y z w`, `m+1` knots
//! (`SmBSplineCurve(gw_CURVE*, dim, FALSE)`). A domain `D` is `umin vmin umax vmax`.
//! The op's parameters follow:
//!
//! | [`Op`]        | upstream                              | parameters                                   |
//! |---------------|---------------------------------------|----------------------------------------------|
//! | `Offset`      | `ApproximateOffsetSurface`            | `dist tol param_type`                        |
//! | `Extend`      | `CreateExtendedSurface`               | `dir_overload dir dist continuity`           |
//! | `Join`        | `JoinSurface`                         | surface `dir tol`                            |
//! | `ExtEval`     | `EvaluatePointOnExtendedSurface`      | `u v`                                        |
//! | `Reparam`     | `Reparameterize`                      | `D[4]`                                       |
//! | `Reverse`     | `Reverse`                             | `dir`                                        |
//! | `Expand`      | `ExpandBoundarySpan`                  | `param dir`                                  |
//! | `EvalSimple`  | `EvaluateSimple`                      | `u v hu hv u_left v_left upper`              |
//! | `EvalPoint`   | `EvaluatePoint`                       | `u v`                                        |
//! | `Coons`       | `CreateCoonsPatch`                    | `tol bilinear` 4 curves `nd` `nd` curves     |
//! | `NSided`      | `CreateNSidedPatch`                   | `k` `k` curves, `k` x (`has` [curve]), `eps has_center C[3] N[3]` |
//! | `Ruled`       | `CreateRuledSurface`                  | curve curve `dir`                            |
//! | `Sweep`       | `CreateSweepSurface`                  | curve curve `has_scale` [curve] `trans tol`  |
//! | `Swung`       | `CreateSwungSurface`                  | curve curve `scale`                          |
//! | `NonRat`      | `sm_CreateNonRationalCurve` (copy)    | curve `has_uv` [curve] `has_base` [surface] `tol` |
//! | `MakeKnots`   | `sm_MakeKnots` (copy)                 | `n p u[n+1]`                                 |
//! | `TrimBounds`  | `sm_CheckTrimBoundaries` (copy)       | `nk K[nk] tmin min_idx+1 tmax max_idx+1 D[4] dir` |
//! | `ZeroDerivs`  | `sm_check_zero_surface_derivs` (copy) | `u v D[4] P[3] Su[3] Sv[3] Suu[3] Suv[3] Svv[3] sz` |
//!
//! The ops from `Coons` on are static (`ctor == 0`); the others need a surface. The
//! output is the op status (`0` = `SM_SUCCESS`, else the `SmStatus`), then on success:
//! for ops creating a surface `has` and its state; for `NSided` the patch count, their
//! states and the (unitized) center normal; for `Reparam` / `Reverse` the state of
//! `this` (for `Expand` also on failure); for the evaluators the point or the
//! `(hu+1)(hv+1)` derivatives (row by row, `u` order slowest; untouched entries `0`);
//! `NonRat` `has` [curve state] `has_uv` [curve state]; `MakeKnots` the `n+p+2` knots;
//! `TrimBounds` `tmin tmax`; `ZeroDerivs` `flag Su Sv`. A surface state is
//! `has_nurb oob borrowed knot_type form [n m p q r s cpts u v]`; a curve state
//! `dim oob borrowed knot_type form has_nurb [n p m cpts knots]`.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour:
//! well-formed clamped 3D surfaces and curves (`r = n+p+1`, `1 <= p <= n`, degrees at
//! most 17, finite data, consistent rationality, non-decreasing knots with end
//! multiplicity exactly `degree + 1` and interior multiplicity at most the degree), a
//! surface without NURB only for `Offset` and `Reparam`, power-basis surfaces (form
//! `10`) for `EvalSimple` only when the v-power derivative rows fit `NL_MAXDEG + 1`
//! (`p + 1 + min(hv, q)(q + 1) + q <= 17`), positive finite distances and
//! tolerances, join directions `U` / `V`, at least one n-sided curve, knot indices
//! inside the knot array, `p >= 1` for `MakeKnots`.

use crate::{call_with_buffer, FfiError};

/// Largest control point index accepted by [`eval`].
pub const MAX_INDEX: usize = 2048;
/// Largest NURB degree accepted by [`eval`].
pub const MAX_DEGREE: usize = 17;
/// `NL_NOZ`.
const NOZ: f64 = f64::MAX;
/// `NL_NOW`.
const NOW: f64 = -f64::MAX;
/// `NL_MAXDER`.
const MAX_DERIVATIVE: usize = 4;
/// `SM_SF_POLYNOMIAL`.
const FORM_POLYNOMIAL: usize = 10;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Offset,
    Extend,
    Join,
    ExtEval,
    Reparam,
    Reverse,
    Expand,
    EvalSimple,
    EvalPoint,
    Coons,
    NSided,
    Ruled,
    Sweep,
    Swung,
    NonRat,
    MakeKnots,
    TrimBounds,
    ZeroDerivs,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::Offset => 0,
            Op::Extend => 1,
            Op::Join => 2,
            Op::ExtEval => 3,
            Op::Reparam => 4,
            Op::Reverse => 5,
            Op::Expand => 6,
            Op::EvalSimple => 7,
            Op::EvalPoint => 8,
            Op::Coons => 20,
            Op::NSided => 21,
            Op::Ruled => 22,
            Op::Sweep => 23,
            Op::Swung => 24,
            Op::NonRat => 25,
            Op::MakeKnots => 26,
            Op::TrimBounds => 27,
            Op::ZeroDerivs => 28,
        }
    }

    /// Whether the op is static (no `this`).
    #[must_use]
    pub fn is_static(self) -> bool {
        self.code() >= 20
    }

    /// Whether the op handles a surface without NURB.
    #[must_use]
    pub fn handles_empty(self) -> bool {
        matches!(self, Op::Offset | Op::Reparam)
    }
}

mod sys {
    extern "C" {
        pub fn sc_nurbs_b_spline_surface_n_lib_p1_eval(
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

crate::reader::unit_reader!();

impl Reader<'_> {
    fn finite(&mut self) -> Result<f64, FfiError> {
        let v = self.num()?;
        if !v.is_finite() {
            return Err(bad("non-finite value"));
        }
        Ok(v)
    }

    fn finites(&mut self, k: usize) -> Result<Vec<f64>, FfiError> {
        (0..k).map(|_| self.finite()).collect()
    }

    fn positive(&mut self, what: &str) -> Result<f64, FfiError> {
        let v = self.finite()?;
        if v <= 0.0 {
            return Err(bad(format!("{what}: need a positive value")));
        }
        Ok(v)
    }
}

/// Checks a clamped knot vector of degree `p` with `m + 1` knots.
fn check_knots(knots: &[f64], p: usize, what: &str) -> Result<(), FfiError> {
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
    Ok(())
}

/// Checks that all points are 3D and share their rational marker.
fn check_points(pts: &[[f64; 4]], what: &str) -> Result<(), FfiError> {
    let [_, _, _, w0] = *pts
        .first()
        .ok_or_else(|| bad(format!("{what}: no points")))?;
    for &[_, _, z, w] in pts {
        if z == NOZ {
            return Err(bad(format!("{what}: 2D data")));
        }
        if (w == NOW) != (w0 == NOW) {
            return Err(bad(format!("{what}: mixed rationality")));
        }
        if w != NOW && w <= 0.0 {
            return Err(bad(format!("{what}: non-positive weight")));
        }
    }
    Ok(())
}

fn points(rd: &mut Reader<'_>, k: usize) -> Result<Vec<[f64; 4]>, FfiError> {
    (0..k)
        .map(|_| Ok([rd.finite()?, rd.finite()?, rd.finite()?, rd.finite()?]))
        .collect()
}

/// Reads and validates a surface `n m p q r s cpts u v`; returns `(p, q)`.
fn surface(rd: &mut Reader<'_>) -> Result<(usize, usize), FfiError> {
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
    let pts = points(rd, (n + 1) * (m + 1))?;
    let u = rd.finites(r + 1)?;
    let v = rd.finites(s + 1)?;
    check_points(&pts, "surface")?;
    check_knots(&u, p, "surface u")?;
    check_knots(&v, q, "surface v")?;
    Ok((p, q))
}

/// Reads and validates a curve `dim n p m cpts knots`.
fn curve(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    if rd.index(3)? != 3 {
        return Err(bad("curve: dimension must be 3"));
    }
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad("curve: need 1 <= p <= n and m = n + p + 1"));
    }
    let pts = points(rd, n + 1)?;
    let knots = rd.finites(m + 1)?;
    check_points(&pts, "curve")?;
    check_knots(&knots, p, "curve")
}

fn validate_static(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::Coons => {
            rd.positive("coons tol")?;
            rd.flag()?;
            for _ in 0..4 {
                curve(rd)?;
            }
            let nd = rd.index(4)?;
            if nd != 0 && nd != 4 {
                return Err(bad("coons: need zero or four derivative curves"));
            }
            for _ in 0..nd {
                curve(rd)?;
            }
        }
        Op::NSided => {
            let k = rd.index(12)?;
            if k == 0 {
                return Err(bad("n-sided: no curves"));
            }
            for _ in 0..k {
                curve(rd)?;
            }
            for _ in 0..k {
                if rd.flag()? {
                    curve(rd)?;
                }
            }
            rd.positive("n-sided eps")?;
            rd.flag()?;
            rd.finites(6)?;
        }
        Op::Ruled => {
            curve(rd)?;
            curve(rd)?;
            rd.index(9)?;
        }
        Op::Sweep => {
            curve(rd)?;
            curve(rd)?;
            if rd.flag()? {
                curve(rd)?;
            }
            rd.flag()?;
            rd.positive("sweep tol")?;
        }
        Op::Swung => {
            curve(rd)?;
            curve(rd)?;
            rd.finite()?;
        }
        Op::NonRat => {
            curve(rd)?;
            if rd.flag()? {
                curve(rd)?;
            }
            if rd.flag()? {
                surface(rd)?;
            }
            rd.positive("non-rational tol")?;
        }
        Op::MakeKnots => {
            let n = rd.index(MAX_INDEX)?;
            let p = rd.index(MAX_DEGREE)?;
            if p == 0 {
                return Err(bad("make knots: degree 0"));
            }
            rd.finites(n + 1)?;
        }
        Op::TrimBounds => {
            let nk = rd.index(MAX_INDEX)?;
            if nk == 0 {
                return Err(bad("trim bounds: no knots"));
            }
            rd.finites(nk)?;
            rd.finite()?;
            let min1 = rd.index(MAX_INDEX + 1)?;
            rd.finite()?;
            rd.index(MAX_INDEX + 1)?;
            if min1 > nk {
                return Err(bad("trim bounds: min index past the knots"));
            }
            rd.finites(4)?;
            rd.index(9)?;
        }
        Op::ZeroDerivs => {
            rd.finites(2 + 4 + 3 * 6)?;
            rd.finite()?;
        }
        _ => return Err(bad("not a static op")),
    }
    Ok(())
}

fn validate_surface_op(
    op: Op,
    rd: &mut Reader<'_>,
    degrees: Option<(usize, usize)>,
    form: usize,
) -> Result<(), FfiError> {
    if degrees.is_none() && !op.handles_empty() {
        return Err(bad("op needs a NURB"));
    }
    match op {
        Op::Offset => {
            rd.finite()?;
            rd.positive("offset tol")?;
            rd.index(3)?;
        }
        Op::Extend => {
            rd.flag()?;
            rd.index(9)?;
            rd.positive("extend dist")?;
            rd.index(13)?;
        }
        Op::Join => {
            surface(rd)?;
            if rd.index(1)? > 1 {
                return Err(bad("join: direction must be U or V"));
            }
            rd.positive("join tol")?;
        }
        Op::ExtEval | Op::EvalPoint => {
            rd.finites(2)?;
        }
        Op::Reparam => {
            rd.finites(4)?;
        }
        Op::Reverse => {
            rd.index(9)?;
        }
        Op::Expand => {
            rd.finite()?;
            rd.index(9)?;
        }
        Op::EvalSimple => {
            rd.finites(2)?;
            let hu = rd.index(20)?;
            let hv = rd.index(20)?;
            rd.flag()?;
            rd.flag()?;
            rd.flag()?;
            // N_SrfPowerBasisEvalDerivs keeps the u powers and min(hv, q) rows of
            // v-power derivatives in one NL_MAXDEG + 1 array
            if form == FORM_POLYNOMIAL && hu <= MAX_DERIVATIVE && hv <= MAX_DERIVATIVE {
                let (p, q) = degrees.unwrap_or((0, 0));
                if p + 1 + hv.min(q) * (q + 1) + q > MAX_DEGREE {
                    return Err(bad("power basis: derivative rows overrun NL_MAXDEG + 1"));
                }
            }
        }
        _ => return Err(bad("not a surface op")),
    }
    Ok(())
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    let ctor = rd.index(2)?;
    if (ctor == 0) != op.is_static() {
        return Err(bad("static ops need ctor 0, surface ops a constructor"));
    }
    if ctor == 0 {
        validate_static(op, &mut rd)?;
    } else {
        rd.flag()?;
        let form = rd.index(11)?;
        let degrees = if ctor == 2 {
            Some(surface(&mut rd)?)
        } else {
            None
        };
        validate_surface_op(op, &mut rd, degrees, form)?;
    }
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
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above: well-formed clamped 3D surfaces and curves,
        // every index, count and flag the shim passes to upstream, NURB-less surfaces
        // only for ops that handle them); buf is valid for buf.len() writes and the shim
        // honours the capacity; len is valid.
        unsafe {
            sys::sc_nurbs_b_spline_surface_n_lib_p1_eval(
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
