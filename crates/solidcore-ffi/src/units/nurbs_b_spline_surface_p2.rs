//! FFI wrappers for unit `nurbs.b_spline_surface_p2`.
//!
//! Shim: `harness/cpp/shim/nurbs_b_spline_surface_p2.cpp`
//! (`sc_nurbs_b_spline_surface_p2_eval`).
//! Upstream: `source/SMLib/src/SmBSplineSurface.cpp`.
//!
//! The input starts with a surface block: `ctor`, then for `ctor != 0` `oob form`, then
//! for `ctor == 2` a surface. `ctor` is `0` (static ops), `1` `SmBSplineSurface()` (no
//! NURB) or `2` `SmBSplineSurface(gw_SURFACE*, FALSE, NULL)`. A surface is
//! `n m p q r s`, `(n+1)(m+1)` control points `x y z w` (v index fastest), `r+1` u
//! knots, `s+1` v knots. A curve is `n p m`, `n+1` points `x y z w`, `m+1` knots
//! (`SmBSplineCurve(gw_CURVE*, 3, FALSE)`). A domain `D` is `umin vmin umax vmax`.
//! The op's parameters follow:
//!
//! | [`Op`]       | upstream                      | parameters                                   |
//! |--------------|-------------------------------|----------------------------------------------|
//! | `Analytics`  | `CopyAndAddAnalytics`         |                                              |
//! | `Iso`        | `CreateIsoParametricCurve`    | `param iso has_dom D[4] want_uv`             |
//! | `DegenPt`    | `IsDegeneratePoint`           | `tol has_dom D[4]`                           |
//! | `Bilinear`   | `IsBilinear`                  |                                              |
//! | `GetCp`      | `GetControlPoint`             | `form u v`                                   |
//! | `Expert`     | `GetControlPointsExpert`      | `form su eu sv ev u_stride v_stride`         |
//! | `MaxDomain`  | `GetMaxAnalyticDomain`        |                                              |
//! | `SetCanon`   | `SetCanonical`                | `ud vd ncp P[3 ncp] form nu UK[nu] UM[nu] nv VK[nv] VM[nv] kt has_w [nw W[nw]]` |
//! | `SetCp`      | `SetControlPoint`             | `form u v x y z w`                           |
//! | `SubPatch`   | `CopySubPatch`                | `D[4]`                                       |
//! | `Split`      | `SplitAt`                     | `t param`                                    |
//! | `Swap`       | `SwapUV`                      |                                              |
//! | `Adjust`     | `AdjustSTEPUVDomain`          | `D[4]`                                       |
//! | `Sphere`     | `CreateSphereFromArcs`        | `k` `k` curves `tol`                         |
//! | `Revolution` | `CreateSurfOfRevolution`      | curve `O[3] A[3] angle swap_uv`              |
//! | `TrimPlane`  | `CreateTrimmedPlaneFromRuled` | curve curve                                  |
//!
//! The last three are static (`ctor == 0`). The output is the op status (`0` =
//! `SM_SUCCESS`, else the `SmStatus`), then: `Analytics` on success the new surface's
//! type; `Iso` on success `max_gap has_uv has_curve [curve state]`; `DegenPt`,
//! `Bilinear` the flag; `GetCp` on success `x y z w`; `Expert` on success `len` and
//! `len` values (`len` = `(eu-su) u_stride + (ev-sv) v_stride + point size`, `0` for
//! an inverted range; unwritten slots `0`); `MaxDomain` the domain; `SetCanon`,
//! `SetCp`, `Swap`, `Adjust` always the state of `this`; `SubPatch`, `Revolution` on
//! success `has [state]`; `Split` on success `has [left state, right state]`;
//! `Sphere` on success `k`, the input index of each CCW arc, the orientations,
//! `has [state]`; `TrimPlane` on success the trimming curve count and `has [state]`.
//! A surface state is `has_nurb oob borrowed knot_type form [n m p q r s cpts u v]`, a
//! curve state `dim oob borrowed knot_type form has_nurb [n p m cpts knots]`.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour:
//! well-formed clamped 3D surfaces and curves (`r = n+p+1`, `1 <= p <= n`, degrees at
//! most 17, finite data, consistent rationality, non-decreasing knots with end
//! multiplicity exactly `degree + 1` and interior multiplicity at most the degree);
//! a NURB for every surface op but `Iso`, `Bilinear`, `SetCanon` and `Swap`; ordered
//! domains; `GetCp` indices inside the net, `Expert` single-point indices inside the
//! net and strides of at most [`MAX_STRIDE`]; control point forms `0..=2`; canonical
//! data whose expanded knots give at least one control point per direction once
//! degrees, weight count and point count pass upstream's checks; one to six arcs for
//! `Sphere`.

use crate::{call_with_buffer, FfiError};

/// Largest control point index accepted by [`eval`].
pub const MAX_INDEX: usize = 512;
/// Largest NURB degree accepted by [`eval`].
pub const MAX_DEGREE: usize = 17;
/// Largest `Expert` stride accepted by [`eval`].
pub const MAX_STRIDE: usize = 256;
/// `NL_NOZ`.
const NOZ: f64 = f64::MAX;
/// `NL_NOW`.
const NOW: f64 = -f64::MAX;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `CopyAndAddAnalytics`.
    Analytics,
    /// `CreateIsoParametricCurve`.
    Iso,
    /// `IsDegeneratePoint`.
    DegenPt,
    /// `IsBilinear`.
    Bilinear,
    /// `GetControlPoint`.
    GetCp,
    /// `GetControlPointsExpert`.
    Expert,
    /// `GetMaxAnalyticDomain`.
    MaxDomain,
    /// `SetCanonical`.
    SetCanon,
    /// `SetControlPoint`.
    SetCp,
    /// `CopySubPatch`.
    SubPatch,
    /// `SplitAt`.
    Split,
    /// `SwapUV`.
    Swap,
    /// `AdjustSTEPUVDomain`.
    Adjust,
    /// `CreateSphereFromArcs`.
    Sphere,
    /// `CreateSurfOfRevolution`.
    Revolution,
    /// `CreateTrimmedPlaneFromRuled`.
    TrimPlane,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::Analytics => 0,
            Op::Iso => 1,
            Op::DegenPt => 2,
            Op::Bilinear => 3,
            Op::GetCp => 4,
            Op::Expert => 5,
            Op::MaxDomain => 6,
            Op::SetCanon => 7,
            Op::SetCp => 8,
            Op::SubPatch => 9,
            Op::Split => 10,
            Op::Swap => 11,
            Op::Adjust => 12,
            Op::Sphere => 20,
            Op::Revolution => 21,
            Op::TrimPlane => 22,
        }
    }

    /// Whether the op is static (no `this`).
    #[must_use]
    pub fn is_static(self) -> bool {
        self.code() >= 20
    }

    /// Whether the op handles a surface without NURB.
    #[must_use]
    pub fn allows_empty(self) -> bool {
        matches!(self, Op::Iso | Op::Bilinear | Op::SetCanon | Op::Swap)
    }
}

mod sys {
    extern "C" {
        pub fn sc_nurbs_b_spline_surface_p2_eval(
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

    fn domain(&mut self) -> Result<(), FfiError> {
        let d = self.finites(4)?;
        let at = |i: usize| d.get(i).copied().unwrap_or(f64::NAN);
        if !(at(0) <= at(2) && at(1) <= at(3)) {
            return Err(bad("domain not ordered"));
        }
        Ok(())
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

/// Reads and validates a surface `n m p q r s cpts u v`; returns `(n, m)`.
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
    Ok((n, m))
}

/// Reads and validates a curve `n p m cpts knots`.
fn curve(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad("curve: need 1 <= p <= n and m = n + p + 1"));
    }
    let pts = points(rd, n + 1)?;
    let knots = rd.finites(m + 1)?;
    check_points(&pts, "curve")?;
    check_knots(&knots, p, "curve")?;
    Ok(())
}

fn validate_canonical(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let ud = rd.index(40)?;
    let vd = rd.index(40)?;
    let ncp = rd.index(MAX_INDEX * 8)?;
    rd.finites(3 * ncp)?;
    rd.index(11)?;
    let nu = rd.index(MAX_INDEX)?;
    let uk = rd.finites(nu)?;
    let mut tu = 0usize;
    for _ in 0..nu {
        tu += rd.index(64)?;
    }
    let nv = rd.index(MAX_INDEX)?;
    let vk = rd.finites(nv)?;
    let mut tv = 0usize;
    for _ in 0..nv {
        tv += rd.index(64)?;
    }
    rd.index(4)?;
    let weights_ok = if rd.flag()? {
        let nw = rd.index(MAX_INDEX * 8)?;
        let w = rd.finites(nw)?;
        if w.iter().any(|&x| x <= 0.0) {
            return Err(bad("canonical: non-positive weight"));
        }
        nw == 0 || nw == ncp
    } else {
        true
    };
    let degrees_ok = (1..=32).contains(&ud) && (1..=32).contains(&vd);
    let nuc = tu.wrapping_sub(ud + 1);
    let nvc = tv.wrapping_sub(vd + 1);
    if degrees_ok && weights_ok && nuc.wrapping_mul(nvc) == ncp {
        if tu <= ud || tv <= vd || nuc == 0 || nvc == 0 {
            return Err(bad("canonical: need a control point per direction"));
        }
        if uk.windows(2).any(|w| w.first() >= w.last())
            || vk.windows(2).any(|w| w.first() >= w.last())
        {
            return Err(bad("canonical: knots not increasing"));
        }
    }
    Ok(())
}

fn validate_static(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::Sphere => {
            let k = rd.index(6)?;
            if k == 0 {
                return Err(bad("sphere: no arcs (upstream reads crArcs[0])"));
            }
            for _ in 0..k {
                curve(rd)?;
            }
            rd.finite()?;
        }
        Op::Revolution => {
            curve(rd)?;
            rd.finites(6)?;
            let ang = rd.finite()?;
            if ang.abs() > 720.0 {
                return Err(bad("revolution: angle beyond 720 degrees"));
            }
            rd.flag()?;
        }
        Op::TrimPlane => {
            curve(rd)?;
            curve(rd)?;
        }
        _ => return Err(bad("not a static op")),
    }
    Ok(())
}

fn validate_surface_op(
    op: Op,
    rd: &mut Reader<'_>,
    dims: Option<(usize, usize)>,
) -> Result<(), FfiError> {
    if dims.is_none() && !op.allows_empty() {
        return Err(bad("op needs a NURB"));
    }
    let (n, m) = dims.unwrap_or((0, 0));
    match op {
        Op::Analytics | Op::Bilinear | Op::MaxDomain | Op::Swap => {}
        Op::Iso => {
            rd.index(9)?;
            rd.finite()?;
            rd.flag()?;
            rd.domain()?;
            rd.flag()?;
        }
        Op::DegenPt => {
            rd.finite()?;
            rd.flag()?;
            rd.domain()?;
        }
        Op::GetCp => {
            rd.index(2)?;
            let u = rd.index(MAX_INDEX)?;
            let v = rd.index(MAX_INDEX)?;
            if u > n || v > m {
                return Err(bad("get control point: index outside the net"));
            }
        }
        Op::Expert => {
            rd.index(2)?;
            let su = rd.index(MAX_INDEX)?;
            let eu = rd.index(MAX_INDEX)?;
            let sv = rd.index(MAX_INDEX)?;
            let ev = rd.index(MAX_INDEX)?;
            rd.index(MAX_STRIDE)?;
            rd.index(MAX_STRIDE)?;
            let need_stride = eu > su || ev > sv;
            if !need_stride && su == eu && sv == ev && (eu > n || ev > m) {
                return Err(bad("expert: single point outside the net"));
            }
        }
        Op::SetCanon => validate_canonical(rd)?,
        Op::SetCp => {
            rd.index(2)?;
            rd.index(MAX_INDEX * 2)?;
            rd.index(MAX_INDEX * 2)?;
            rd.finites(4)?;
        }
        Op::SubPatch | Op::Adjust => rd.domain()?,
        Op::Split => {
            rd.finite()?;
            rd.index(9)?;
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
        rd.index(11)?;
        let dims = if ctor == 2 {
            Some(surface(&mut rd)?)
        } else {
            None
        };
        validate_surface_op(op, &mut rd, dims)?;
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
        // every index, count and flag the shim passes to upstream, NURBs where upstream
        // dereferences them, single control point indices inside the net, bounded
        // strides); buf is valid for buf.len() writes and the shim honours the
        // capacity; len is valid.
        unsafe {
            sys::sc_nurbs_b_spline_surface_p2_eval(
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
