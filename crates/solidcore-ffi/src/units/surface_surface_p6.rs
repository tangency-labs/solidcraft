//! FFI wrappers for unit `surface.surface_p6`.
//!
//! Shim: `harness/cpp/shim/surface_surface_p6.cpp` (`sc_surface_surface_p6_eval`).
//! Upstream: `source/SMLib/src/SmSurface.cpp`, `source/SMLib/inc/SmSurface.h`.
//!
//! Every input starts with a surface block: `0 oob form n m p q r s cpts(x y z w) u v` (an
//! `SmBSplineSurface(gw_SURFACE*, FALSE, NULL)`), `1 O[3] N[3]` (`SmPlane(O, N)`), `2 basis
//! implicit` (an `SmCurveBoundedSurface` without boundary curves over a B-spline or plane
//! basis block) or `3 A[3] B[3] P[3] axis[3] swap` (`SmSurfOfRevolution::CreateCanonical` of
//! the `SmLine(A, B)` generator about the axis through `P`), or `4` (a canonical radius-2
//! `SmSphere` at the origin with the standard X/Y frame). A curve block is `0 dim n p m
//! cpts(x y z w) knots` (an `SmBSplineCurve(gw_CURVE*, dim, FALSE)`) or `1 A[3] B[3]`
//! (`SmLine::CreateLineSegment`, 3D). A domain is `u0 v0 u1 v1`, an interval `a b`, a box
//! `min[3] max[3]`. A solution is `type nvars value end_value start[5] end[5]` (its objects
//! are the op's curve/surfaces). A curve record is `ivl_min ivl_max` and the points at 0, 1/4,
//! 1/2, 3/4 and 1 of the natural interval. Every output starts with the status (`0` =
//! `SM_SUCCESS`, else the `SmStatus`; `-3` = a surface or curve the shim could not build);
//! `sm_SCLocalSolve` calls the real shared p5 reference body. Remaining outputs:
//!
//! | [`Op`]     | upstream                                   | parameters after the surface | outputs |
//! |------------|--------------------------------------------|------------------------------|---------|
//! | `Cont`     | `CalculateContinuities`                    | `param`                      | `min n conts[n]` |
//! | `Trim`     | `CreateNaturalUVTrimCurves` (with gaps)    | `dom side remove`            | `n` then per side `orient gap curve3d uvcurve` |
//! | `Sample`   | `SmDerivSurfDefinition::GetSampleValues`   | `nsamples tt sdir edir slen escale sang eang has_ref [ref] curve ivl right start want2 want3` | `n` then per sample `t P D1 [D2] [D3]` |
//! | `Lci`      | `LocalCurveIntersect`                      | `dom curve ivl tol gu gv gt` | `found [u v t dev]` |
//! | `Scc`      | `SimpleCoincidenceChecker`                 | `dom curve ivl tol partial`  | `found [type obj0 obj1 sv s[3] ev e[3]]` |
//! | `Classify` | `ClassifyIntersectingRangeSolution`        | `curve solution`             | `degenerate value p[3]` (`12345` = not written) |
//! | `Ssid`     | `SmSSGlobalSolver::IdenticalSolutions`     | `surface2 tol sol1 sol2`     | `type preferred(0 none, 1, 2)` |
//! | `Scls`     | `sm_SCLocalSolve`                          | `dom uc[2] vc[2] sbox has_cbox cbox curve ivl tc[2] tol has_vec vec sop skip process` | `n` then per solution `type nvars value t u v` |
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour or runaway work:
//! well-formed clamped B-splines, planes with a non-zero normal, revolutions of a generator
//! of positive length not lying on the axis (one end on it, a cone's apex, is fine), lines
//! of positive length, parameters within one domain length of a B-spline's domain (bounded
//! elsewhere), domains inside a B-spline's natural
//! domain, intervals inside a B-spline curve's range, up to 64 samples, a reference curve
//! for `SM_TT_DIR_TO_REFERENCE_CURVE` (upstream dereferences it unchecked), solution parameter
//! counts within the objects' parameters, the solver operations `sm_SCLocalSolve` supports
//! (the signed directed one with a vector) on B-spline surfaces. Inputs are finite.

use crate::validate::{bad, check_knots, NOW, NOZ};
use crate::{call_with_buffer, FfiError};

/// Largest control point index.
pub const MAX_INDEX: usize = 32;
/// Largest degree.
pub const MAX_DEGREE: usize = 5;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1.0e6;
/// Largest sample count of `GetSampleValues`.
pub const MAX_SAMPLES: usize = 64;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Cont,
    Trim,
    Sample,
    Lci,
    Scc,
    Classify,
    Ssid,
    Scls,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        self as i32
    }
}

mod sys {
    extern "C" {
        pub fn sc_surface_surface_p6_eval(
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
    fn finite(&mut self) -> Result<f64, FfiError> {
        let v = self.num()?;
        if !v.is_finite() || v.abs() > MAX_COORD {
            return Err(bad("non-finite or huge value"));
        }
        Ok(v)
    }

    fn finites(&mut self, k: usize) -> Result<Vec<f64>, FfiError> {
        (0..k).map(|_| self.finite()).collect()
    }

    fn vec3(&mut self) -> Result<[f64; 3], FfiError> {
        Ok([self.finite()?, self.finite()?, self.finite()?])
    }

    fn nonzero3(&mut self) -> Result<[f64; 3], FfiError> {
        let v = self.vec3()?;
        if v.iter().map(|c| c * c).sum::<f64>() < 1e-12 {
            return Err(bad("zero vector"));
        }
        Ok(v)
    }

    fn index(&mut self, max: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
            return Err(bad(format!("bad index/count {v} (max {max})")));
        }
        Ok(v as usize)
    }

    fn flag(&mut self) -> Result<bool, FfiError> {
        Ok(self.index(1)? == 1)
    }

    fn positive(&mut self, what: &str) -> Result<f64, FfiError> {
        let v = self.finite()?;
        if v <= 0.0 {
            return Err(bad(format!("{what}: need a positive value")));
        }
        Ok(v)
    }
}

/// Checks `k` control points sharing their rational marker (`z = NOZ` allowed for
/// curves when `planar`).
fn check_points(rd: &mut Reader<'_>, k: usize, planar: bool, what: &str) -> Result<(), FfiError> {
    let mut w0 = None;
    for _ in 0..k {
        let x = rd.num()?;
        let y = rd.num()?;
        let z = rd.num()?;
        let w = rd.num()?;
        if z == NOZ && !planar {
            return Err(bad(format!("{what}: 2D data")));
        }
        let rational = w != NOW;
        if *w0.get_or_insert(rational) != rational {
            return Err(bad(format!("{what}: mixed rationality")));
        }
        if rational && !(1e-3..=1e3).contains(&w) {
            return Err(bad(format!("{what}: weight out of range")));
        }
        let z = if z == NOZ { 0.0 } else { z };
        if [x, y, z]
            .iter()
            .any(|c| !c.is_finite() || c.abs() > MAX_COORD)
        {
            return Err(bad(format!("{what}: point out of range")));
        }
    }
    Ok(())
}

/// The parameter domain `[u0, v0, u1, v1]` of a B-spline surface block, `None` otherwise.
type Domain = Option<[f64; 4]>;

fn dist_to_axis(p: [f64; 3], o: [f64; 3], a: [f64; 3]) -> f64 {
    let d = [p[0] - o[0], p[1] - o[1], p[2] - o[2]];
    let la = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    let c = [
        d[1] * a[2] - d[2] * a[1],
        d[2] * a[0] - d[0] * a[2],
        d[0] * a[1] - d[1] * a[0],
    ];
    (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt() / la
}

/// A surface block: its domain and eligibility as a B-spline-backed solver patch
/// (including the canonical Sphere).
fn surface_kind(rd: &mut Reader<'_>, nested: bool) -> Result<(Domain, bool), FfiError> {
    let kind = rd.index(4)?;
    if kind == 4 {
        return Ok((Some([0.0, 0.0, 1.0, 1.0]), true));
    }
    if kind == 3 {
        let a = rd.vec3()?;
        let b = rd.vec3()?;
        let p = rd.vec3()?;
        let ax = rd.nonzero3()?;
        rd.flag()?;
        let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2)).sqrt();
        if len < 1e-3 {
            return Err(bad("revolution generator too short"));
        }
        // one end on the axis is a true cone (its apex: a degenerate side, which upstream
        // handles); a generator along the axis sweeps no surface
        if dist_to_axis(a, p, ax) < 1e-3 && dist_to_axis(b, p, ax) < 1e-3 {
            return Err(bad("revolution generator lies on the axis"));
        }
        return Ok((None, false));
    }
    if kind == 2 {
        if nested {
            return Err(bad("nested curve-bounded surface"));
        }
        let (dom, plain) = surface_kind(rd, true)?;
        if dom.is_none() && plain {
            return Err(bad("bad basis"));
        }
        rd.flag()?;
        return Ok((dom, false));
    }
    if kind == 1 {
        rd.vec3()?;
        rd.nonzero3()?;
        return Ok((None, false));
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
    check_points(rd, (n + 1) * (m + 1), false, "surface")?;
    let u = rd.finites(r + 1)?;
    let v = rd.finites(s + 1)?;
    let (u0, u1) = check_knots(&u, p, "surface u")?;
    let (v0, v1) = check_knots(&v, q, "surface v")?;
    Ok((Some([u0, v0, u1, v1]), true))
}

/// A curve block; returns the parameter range of a B-spline (`None` for a line).
fn curve(rd: &mut Reader<'_>) -> Result<Option<(f64, f64)>, FfiError> {
    if rd.index(1)? == 1 {
        let a = rd.vec3()?;
        let b = rd.vec3()?;
        let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2)).sqrt();
        if len < 1e-6 {
            return Err(bad("line too short"));
        }
        return Ok(None);
    }
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
    check_points(rd, n + 1, dim == 2, "curve")?;
    let knots = rd.finites(m + 1)?;
    Ok(Some(check_knots(&knots, p, "curve")?))
}

/// A curve block and an interval inside a B-spline's range (bounded for a line).
fn curve_and_interval(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let range = curve(rd)?;
    let x = rd.finite()?;
    let y = rd.finite()?;
    if x >= y {
        return Err(bad("interval not ordered"));
    }
    match range {
        Some((a, b)) if x < a || y > b => Err(bad("interval outside the curve's range")),
        None if x.abs() > 1e4 || y.abs() > 1e4 => Err(bad("line interval too large")),
        _ => Ok(()),
    }
}

/// A `uv` point within one domain length of `dom` (bounded elsewhere).
fn uv(rd: &mut Reader<'_>, dom: Domain) -> Result<(), FfiError> {
    let u = rd.finite()?;
    let v = rd.finite()?;
    if let Some([u0, v0, u1, v1]) = dom {
        let (du, dv) = (u1 - u0, v1 - v0);
        if u < u0 - du || u > u1 + du || v < v0 - dv || v > v1 + dv {
            return Err(bad("uv too far outside the domain"));
        }
    } else if u.abs() > 1e4 || v.abs() > 1e4 {
        return Err(bad("uv too large"));
    }
    Ok(())
}

/// A non-degenerate domain, inside `dom` for a B-spline.
fn domain_in(rd: &mut Reader<'_>, dom: Domain) -> Result<(), FfiError> {
    let d = rd.finites(4)?;
    let (Some(&a), Some(&b), Some(&c), Some(&e)) = (d.first(), d.get(1), d.get(2), d.get(3)) else {
        return Err(bad("domain too short"));
    };
    if a >= c || b >= e {
        return Err(bad("domain not ordered"));
    }
    if let Some([u0, v0, u1, v1]) = dom {
        if a < u0 || c > u1 || b < v0 || e > v1 {
            return Err(bad("domain outside the surface's"));
        }
    } else if a.abs() > 1e4 || b.abs() > 1e4 || c.abs() > 1e4 || e.abs() > 1e4 {
        return Err(bad("domain too large"));
    }
    Ok(())
}

/// A solution `type nvars value end_value start[5] end[5]`, `nvars <= max_vars`.
fn solution(rd: &mut Reader<'_>, max_vars: usize) -> Result<(), FfiError> {
    rd.index(4)?;
    rd.index(max_vars)?;
    rd.finites(12)?;
    Ok(())
}

/// `SM_TT_DIR_TO_REFERENCE_CURVE`.
const TT_DIR_TO_REFERENCE_CURVE: usize = 3;

/// `SM_SO_MINIMIZE`, `SM_SO_MAXIMIZE`, `SM_SO_NORMALIZE`,
/// `SM_SO_3D_SIGNED_DIRECTED_MINIMIZE`.
const SO_MINIMIZE: usize = 3;
const SO_MAXIMIZE: usize = 4;
const SO_NORMALIZE: usize = 5;
const SO_SIGNED_MIN: usize = 9;

/// Checks `input` for `op` (see the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for anything the shim must not see.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    let (dom, is_bspline) = surface_kind(&mut rd, false)?;
    match op {
        Op::Cont => {
            rd.index(1)?;
        }
        Op::Trim => {
            domain_in(&mut rd, dom)?;
            rd.index(4)?;
            rd.flag()?;
        }
        Op::Sample => {
            rd.index(MAX_SAMPLES)?;
            let tt = rd.index(8)?;
            for _ in 0..6 {
                let a = rd.finite()?;
                if a.abs() > 1e4 {
                    return Err(bad("angle or length too large"));
                }
            }
            if rd.flag()? {
                curve(&mut rd)?;
            } else if tt == TT_DIR_TO_REFERENCE_CURVE {
                // AssertValid accepts it, then GetSampleValues dereferences the NULL
                // m_pTangentReference (plan/upstream-issues.md UB-100)
                return Err(bad("SM_TT_DIR_TO_REFERENCE_CURVE needs a reference curve"));
            }
            curve_and_interval(&mut rd)?;
            for _ in 0..4 {
                rd.flag()?;
            }
        }
        Op::Lci => {
            domain_in(&mut rd, dom)?;
            curve_and_interval(&mut rd)?;
            rd.positive("tolerance")?;
            uv(&mut rd, dom)?;
            let t = rd.finite()?;
            if t.abs() > 1e4 {
                return Err(bad("curve guess too large"));
            }
        }
        Op::Scc => {
            domain_in(&mut rd, dom)?;
            curve_and_interval(&mut rd)?;
            rd.positive("tolerance")?;
            rd.flag()?;
        }
        Op::Classify => {
            curve(&mut rd)?;
            // curve (1 parameter) and surface (2)
            solution(&mut rd, 3)?;
        }
        Op::Ssid => {
            surface_kind(&mut rd, false)?;
            let tol = rd.finite()?;
            if tol < 0.0 {
                return Err(bad("negative tolerance"));
            }
            // two surfaces (2 parameters each)
            solution(&mut rd, 4)?;
            solution(&mut rd, 4)?;
        }
        Op::Scls => {
            let d = dom
                .filter(|_| is_bspline)
                .ok_or_else(|| bad("this op needs a B-spline surface"))?;
            domain_in(&mut rd, Some(d))?;
            for _ in 0..4 {
                rd.index(13)?;
            }
            rd.vec3()?;
            rd.vec3()?;
            rd.flag()?;
            rd.vec3()?;
            rd.vec3()?;
            curve_and_interval(&mut rd)?;
            rd.index(13)?;
            rd.index(13)?;
            rd.positive("tolerance")?;
            let has_vec = rd.flag()?;
            rd.vec3()?;
            let s = rd.index(15)?;
            if ![SO_MINIMIZE, SO_MAXIMIZE, SO_NORMALIZE, SO_SIGNED_MIN].contains(&s) {
                return Err(bad(format!("solver operation {s} not supported here")));
            }
            if s == SO_SIGNED_MIN && !has_vec {
                return Err(bad("signed directed solve needs a vector"));
            }
            rd.flag()?;
            rd.flag()?;
        }
    }
    rd.done()
}

/// Runs `op` on `input` (see the module docs) and returns the output record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed input; [`FfiError::Status`] for a shim
/// failure.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(256, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above); buf is valid for buf.len() writes and the
        // shim honours the capacity; len is valid.
        unsafe {
            sys::sc_surface_surface_p6_eval(
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

    #[test]
    fn sphere_kind_uses_existing_generic_validation() {
        let mut rd = Reader::new(&[4.0, 123.0]);
        assert_eq!(
            surface_kind(&mut rd, false).unwrap(),
            (Some([0.0, 0.0, 1.0, 1.0]), true)
        );
        assert_eq!(rd.num().unwrap(), 123.0);
        rd.done().unwrap();
        let mut nested = Reader::new(&[2.0, 4.0, 1.0]);
        assert_eq!(
            surface_kind(&mut nested, false).unwrap(),
            (Some([0.0, 0.0, 1.0, 1.0]), false)
        );
        nested.done().unwrap();
        assert!(surface_kind(&mut Reader::new(&[2.0, 2.0, 4.0, 1.0, 1.0]), false).is_err());
        for side in 0..=4 {
            for remove in [0.0, 1.0] {
                validate(
                    Op::Trim,
                    &[4.0, 0.0, 0.0, 1.0, 1.0, f64::from(side), remove],
                )
                .unwrap();
            }
        }
        for input in [
            vec![4.0, 0.5, 0.0, 0.5, 1.0, 0.0, 0.0],
            vec![4.0, -0.1, 0.0, 1.0, 1.0, 0.0, 0.0],
            vec![4.0, 0.0, 0.0, f64::NAN, 1.0, 0.0, 0.0],
            vec![4.0, 0.0],
            vec![4.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 2.0],
            vec![5.0],
        ] {
            assert!(validate(Op::Trim, &input).is_err());
        }
        validate(Op::Trim, &[2.0, 4.0, 1.0, 0.0, 0.0, 1.0, 1.0, 3.0, 1.0]).unwrap();
        validate(Op::Cont, &[4.0, 1.0]).unwrap();
        let line = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        let mut classify = vec![4.0];
        classify.extend(line);
        classify.extend([0.0; 14]);
        validate(Op::Classify, &classify).unwrap();
        let mut sample = vec![4.0, 4.0, 0.0];
        sample.extend([0.0; 7]); // six values and has_ref
        sample.extend(line);
        sample.extend([0.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
        validate(Op::Sample, &sample).unwrap();
        let mut intersect = vec![4.0, 0.0, 0.0, 1.0, 1.0];
        intersect.extend(line);
        intersect.extend([0.0, 1.0, 1e-6]);
        let mut scc = intersect.clone();
        scc.push(0.0);
        validate(Op::Scc, &scc).unwrap();
        intersect.extend([0.5, 0.5, 0.5]);
        validate(Op::Lci, &intersect).unwrap();
        let mut ssid = vec![4.0, 4.0, 1e-6];
        ssid.extend([0.0; 28]);
        validate(Op::Ssid, &ssid).unwrap();
        let mut scls = vec![4.0, 0.0, 0.0, 1.0, 1.0];
        scls.extend([0.0; 17]); // continuities, boxes, has_cbox
        scls.extend([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0]); // line
        scls.extend([0.0, 1.0, 0.0, 0.0, 1e-6, 0.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0]);
        validate(Op::Scls, &scls).unwrap();
    }
}
