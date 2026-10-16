//! FFI wrappers for unit `nurbs.b_spline_surface_n_lib_p2`.
//!
//! Shim: `harness/cpp/shim/nurbs_b_spline_surface_n_lib_p2.cpp`
//! (`sc_nurbs_b_spline_surface_n_lib_p2_eval`).
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
//! | [`Op`]   | upstream               | parameters                                                     |
//! |----------|------------------------|----------------------------------------------------------------|
//! | `Eval`   | `Evaluate`             | `u v hu hv u_left v_left upper non_zero_tangents`              |
//! | `Trim`   | `TrimWithDomain`       | `D[4]`                                                         |
//! | `Skin`   | `CreateSkinnedSurface` | `k` `k` curves, `sync dir tol has_r1 [curve] has_r2 [curve] spine has_params [np params[np]] has_derivs [has0 [surface] has1 [surface]] degree` |
//! | `Corner` | `CreateCornerBlend`    | `k` `k` curves, `k` orients, `k` x (`has` [curve]), `k` x (`has` [surface]), `tol ang_tol bilinear` |
//!
//! `Skin` and `Corner` are static (`ctor == 0`). The output is the op status (`0` =
//! `SM_SUCCESS`, else the `SmStatus`), then: `Eval` on success the `(hu+1)(hv+1)`
//! derivatives (row by row, `u` order slowest; untouched entries `0`); `Trim` always
//! the (possibly snapped) domain and the state of `this`; `Skin` on success `has` and
//! the surface state; `Corner` on success the surface count and their states. A
//! surface state is `has_nurb oob borrowed knot_type form [n m p q r s cpts u v]`.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour:
//! well-formed clamped 3D surfaces and curves (`r = n+p+1`, `1 <= p <= n`, degrees at
//! most 17, finite data, consistent rationality, non-decreasing knots with end
//! multiplicity exactly `degree + 1` and interior multiplicity at most the degree);
//! `Eval` needs a NURB, and for power-basis surfaces (form `10`) the v-power
//! derivative rows of every evaluation (including the `2, 2` recursion) must fit
//! `NL_MAXDEG + 1`; `Trim` domains are ordered; `Skin` needs 1 to 12 cross sections,
//! a degree of 1 to 5, at least `k` (`2k` with a second rail) intersection parameters
//! when given, compatible cross sections when `sync` is set, and no `sync` with a
//! derivative surface; `Corner` takes at most 8 curves, orientations `0..=2`, a UV
//! curve exactly where a surface is given, non-rational UV curves, no surface for a
//! rational curve (upstream drops it with `SmSurface::DropCurve`, which crashes for a
//! standalone surface), and with more than four curves all UV curves and surfaces
//! unless the blend is not bilinear.

use crate::{call_with_buffer, FfiError};

/// Largest control point index accepted by [`eval`].
pub const MAX_INDEX: usize = 512;
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
pub enum Op {
    /// `Evaluate`.
    Eval,
    /// `TrimWithDomain`.
    Trim,
    /// `CreateSkinnedSurface`.
    Skin,
    /// `CreateCornerBlend`.
    Corner,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::Eval => 0,
            Op::Trim => 1,
            Op::Skin => 20,
            Op::Corner => 21,
        }
    }

    /// Whether the op is static (no `this`).
    #[must_use]
    pub fn is_static(self) -> bool {
        self.code() >= 20
    }
}

mod sys {
    extern "C" {
        pub fn sc_nurbs_b_spline_surface_n_lib_p2_eval(
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

/// Sequential reader over the input.
struct Reader<'a> {
    input: &'a [f64],
    at: usize,
}

impl Reader<'_> {
    fn num(&mut self) -> Result<f64, FfiError> {
        let v = self
            .input
            .get(self.at)
            .copied()
            .ok_or_else(|| bad("input too short"))?;
        self.at += 1;
        Ok(v)
    }

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

    fn index(&mut self, max: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
            return Err(bad(format!("bad index/count {v}")));
        }
        Ok(v as usize)
    }

    fn flag(&mut self) -> Result<bool, FfiError> {
        Ok(self.index(1)? == 1)
    }

    fn done(&self) -> Result<(), FfiError> {
        if self.at != self.input.len() {
            return Err(bad("trailing input"));
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

/// Checks that all points are 3D and share their rational marker; returns whether
/// they are rational.
fn check_points(pts: &[[f64; 4]], what: &str) -> Result<bool, FfiError> {
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
    Ok(w0 != NOW)
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

/// A validated curve: degree, control point count, knots, rationality.
struct CurveInfo {
    p: usize,
    n: usize,
    knots: Vec<f64>,
    rational: bool,
}

/// Reads and validates a curve `dim n p m cpts knots`.
fn curve(rd: &mut Reader<'_>) -> Result<CurveInfo, FfiError> {
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
    let rational = check_points(&pts, "curve")?;
    check_knots(&knots, p, "curve")?;
    Ok(CurveInfo {
        p,
        n,
        knots,
        rational,
    })
}

fn validate_skin(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let k = rd.index(12)?;
    if k == 0 {
        return Err(bad("skin: no cross sections"));
    }
    let secs = (0..k).map(|_| curve(rd)).collect::<Result<Vec<_>, _>>()?;
    let sync = rd.flag()?;
    rd.index(9)?;
    let tol = rd.finite()?;
    if tol < 0.0 {
        return Err(bad("skin: negative tolerance"));
    }
    if rd.flag()? {
        curve(rd)?;
    }
    let rail2 = rd.flag()?;
    if rail2 {
        curve(rd)?;
    }
    rd.flag()?;
    if rd.flag()? {
        let np = rd.index(64)?;
        let need = if rail2 { 2 * k } else { k };
        if np < need {
            return Err(bad("skin: too few intersection parameters"));
        }
        rd.finites(np)?;
    }
    let mut has_derivs = false;
    if rd.flag()? {
        for _ in 0..2 {
            if rd.flag()? {
                surface(rd)?;
                has_derivs = true;
            }
        }
    }
    let degree = rd.index(5)?;
    if degree == 0 {
        return Err(bad("skin: degree 0"));
    }
    if sync {
        if has_derivs {
            return Err(bad(
                "skin: synchronized cross sections with derivative surfaces",
            ));
        }
        let first = secs.first().ok_or_else(|| bad("skin: no cross sections"))?;
        if secs
            .iter()
            .any(|c| c.p != first.p || c.n != first.n || c.knots != first.knots)
        {
            return Err(bad("skin: synchronized cross sections are not compatible"));
        }
    }
    Ok(())
}

fn validate_corner(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let k = rd.index(8)?;
    let crvs = (0..k).map(|_| curve(rd)).collect::<Result<Vec<_>, _>>()?;
    for _ in 0..k {
        rd.index(2)?;
    }
    let mut has_uv = Vec::with_capacity(k);
    for _ in 0..k {
        let has = rd.flag()?;
        if has && curve(rd)?.rational {
            return Err(bad("corner: rational UV curve"));
        }
        has_uv.push(has);
    }
    let mut has_srf = Vec::with_capacity(k);
    for _ in 0..k {
        let has = rd.flag()?;
        if has {
            surface(rd)?;
        }
        has_srf.push(has);
    }
    rd.positive("corner tol")?;
    rd.positive("corner angle tol")?;
    let bilinear = rd.flag()?;
    for ((c, &uv), &srf) in crvs.iter().zip(&has_uv).zip(&has_srf) {
        if uv != srf {
            return Err(bad("corner: a UV curve needs a surface and vice versa"));
        }
        if c.rational && srf {
            return Err(bad("corner: rational curve with a surface (DropCurve)"));
        }
        if k > 4 && bilinear && !srf {
            return Err(bad(
                "corner: n-sided bilinear blend without UV curve/surface",
            ));
        }
    }
    Ok(())
}

fn validate_surface_op(
    op: Op,
    rd: &mut Reader<'_>,
    degrees: Option<(usize, usize)>,
    form: usize,
) -> Result<(), FfiError> {
    match op {
        Op::Eval => {
            let Some((p, q)) = degrees else {
                return Err(bad("evaluate needs a NURB"));
            };
            rd.finites(2)?;
            let hu = rd.index(20)?;
            let hv = rd.index(20)?;
            rd.flag()?;
            rd.flag()?;
            rd.flag()?;
            let nzt = rd.flag()?;
            // N_SrfPowerBasisEvalDerivs keeps the u powers and min(hv, q) rows of
            // v-power derivatives in one NL_MAXDEG + 1 array (also in the 2, 2
            // recursion of the zero-tangent fix)
            if form == FORM_POLYNOMIAL && hu <= MAX_DERIVATIVE && hv <= MAX_DERIVATIVE {
                let hv_max = if nzt { hv.max(2) } else { hv };
                if p + 1 + hv_max.min(q) * (q + 1) + q > MAX_DEGREE {
                    return Err(bad("power basis: derivative rows overrun NL_MAXDEG + 1"));
                }
            }
        }
        Op::Trim => {
            let d = rd.finites(4)?;
            let at = |i: usize| d.get(i).copied().unwrap_or(f64::NAN);
            if !(at(0) <= at(2) && at(1) <= at(3)) {
                return Err(bad("trim: domain not ordered"));
            }
        }
        _ => return Err(bad("not a surface op")),
    }
    Ok(())
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    let ctor = rd.index(2)?;
    if (ctor == 0) != op.is_static() {
        return Err(bad("static ops need ctor 0, surface ops a constructor"));
    }
    if ctor == 0 {
        match op {
            Op::Skin => validate_skin(&mut rd)?,
            Op::Corner => validate_corner(&mut rd)?,
            _ => return Err(bad("not a static op")),
        }
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
        // every index, count and flag the shim passes to upstream, the combinations
        // upstream handles without reading outside its arrays); buf is valid for
        // buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_nurbs_b_spline_surface_n_lib_p2_eval(
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
