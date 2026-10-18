//! FFI wrappers for unit `curve.hyperbola`.
//!
//! Shim: `harness/cpp/shim/curve_hyperbola.cpp` (`sc_curve_hyperbola_eval`).
//! Upstream: `source/SMLib/src/SmHyperbola.cpp`, `source/SMLib/inc/SmHyperbola.h`.
//!
//! Member ops run on a hyperbola built by the constructor from a [`HyperbolaIn`]
//! (`new (ctx) SmHyperbola(O, X, Y, SmExtent1d(dmin, dmax), a, b, dim, &ctx)`), the
//! static [`Op::ReadDb`] on the curve `SmCurve::ReadFromDB(SmHyperbola_TYPE, ..)`
//! reads from the bytes of a [`DbRecord`] ([`db_bytes`]). The flat output is the op
//! status (`0` = `SM_SUCCESS`, else the `SmStatus`), the op's outputs on success, then
//! a state (see [`eval`]):
//!
//! | [`Op`]      | upstream                    | outputs                                       |
//! |-------------|-----------------------------|-----------------------------------------------|
//! | `State`     | queries                     | `bounded analytic arc maxdom[2] stepdom[2] type`, type string chars, `-1`, class string chars, `-1`, one `IsKindOf` flag per type asked |
//! | `EvalStep`  | `EvaluateSTEP(t, nd)`       | `nd + 1` vectors                              |
//! | `StepInv`   | `STEPInversion(P, loc?)`    | `t`, and `loc` when classifying              |
//! | `Adjust`    | `AdjustSTEPInterval`        |                                               |
//! | `Reverse`   | `ReverseParameterization`   | the new interval; then `GetOrCreateGwNurbPointer` |
//! | `Transform` | `Transform`                 |                                               |
//! | `Gps`       | `GlobalPointSolveSTEP`      | `count`, `m_vStart[0]` of each solution       |
//! | `Eq`        | `operator==` (other curve)  | the flag                                      |
//! | `Copy`      | `Copy`                      | `copy == this`, the copy's state              |
//! | `ReadDb`    | `SmCurve::ReadFromDB`       | whether a curve was read, its state           |
//!
//! A state is `O[3] X[3] Y[3] dmin dmax a b dim oob borrowed knot_type form has_nurb
//! [n p m cpts(x y z w) knots]`; member ops end with the state of `this`.

use std::fmt::Write as _;

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

/// Largest magnitude of coordinates, lengths and tolerances.
pub const MAX_COORD: f64 = 1e6;
/// Largest magnitude of an analytic (hyperbolic radian) parameter.
pub const MAX_PARAM: f64 = 30.0;
/// Largest derivative count of [`Op::EvalStep`].
pub const MAX_DERIVS: usize = 8;
/// Largest number of types of [`Op::State`].
pub const MAX_TYPES: usize = 64;
/// Largest control point index of a [`DbRecord`].
pub const MAX_DB_INDEX: i64 = 64;

mod sys {
    extern "C" {
        pub fn sc_curve_hyperbola_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Constructor arguments of a hyperbola.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HyperbolaIn {
    /// Center.
    pub center: [f64; 3],
    /// Semi axis direction.
    pub x_axis: [f64; 3],
    /// Semi imaginary axis direction.
    pub y_axis: [f64; 3],
    /// Analytic domain ends (`SmExtent1d(a, b)`: swapped when `a > b`).
    pub domain: [f64; 2],
    /// Semi axis length.
    pub semi_axis: f64,
    /// Semi imaginary axis length.
    pub semi_image_axis: f64,
    /// Image dimension (2 or 3).
    pub dim: u64,
}

/// The B-spline base record of a database stream (`SmBSplineCurve::WriteToDB`).
#[derive(Debug, Clone, PartialEq)]
pub struct DbRecord {
    /// Hyperbola data: `O[3] X[3] Y[3] dmin dmax a b`.
    pub head: [f64; 13],
    /// Last control point index.
    pub n: i64,
    /// Degree.
    pub p: i16,
    /// Rational flag.
    pub rat: i16,
    /// Stored dimension (2 or 3).
    pub dim: i16,
    /// Knot type code (written for versions above 32).
    pub kt: u64,
    /// Form code (written for versions above 32).
    pub form: u64,
    /// Control point values (2 to 4 each) then the `n + p + 2` knots.
    pub vals: Vec<f64>,
}

/// The upstream routine run by the shim.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// The queries; `IsKindOf` of each type.
    State {
        /// Types for `IsKindOf`.
        types: Vec<i64>,
    },
    /// `EvaluateSTEP(t, nd)`.
    EvalStep {
        /// Analytic parameter.
        t: f64,
        /// Derivative count.
        nd: usize,
    },
    /// `STEPInversion(p, classify ? &loc : NULL)`.
    StepInv {
        /// Point.
        p: [f64; 3],
        /// Whether to classify.
        classify: bool,
    },
    /// `AdjustSTEPInterval(SmExtent1d(a, b))`.
    Adjust {
        /// Interval ends.
        ivl: [f64; 2],
    },
    /// `ReverseParameterization(SmExtent1d(a, b), new)`.
    Reverse {
        /// Interval ends.
        ivl: [f64; 2],
    },
    /// `Transform(placement(o, x, y), scale)`.
    Transform {
        /// Placement origin.
        o: [f64; 3],
        /// Placement X axis.
        x: [f64; 3],
        /// Placement Y axis.
        y: [f64; 3],
        /// Optional scale.
        scale: Option<[f64; 3]>,
    },
    /// `GlobalPointSolveSTEP(SmExtent1d(a, b), op, p, tol, target, NULL, req)`.
    Gps {
        /// Interval ends.
        ivl: [f64; 2],
        /// `SmSolverOperationType` (0, 3, 4 or 5).
        op: i32,
        /// Test point.
        p: [f64; 3],
        /// Distance tolerance.
        tol: f64,
        /// Optional target distance.
        target: Option<f64>,
        /// `SmSolutionRequestedType` (0 or 1).
        req: i32,
    },
    /// `*this == other`.
    Eq {
        /// The other curve.
        other: HyperbolaIn,
    },
    /// `Copy`.
    Copy,
    /// `SmCurve::ReadFromDB(SmHyperbola_TYPE, ..)` (static).
    ReadDb {
        /// Curve dimension asked for.
        dim: u64,
        /// Binary (else ASCII) stream.
        binary: bool,
        /// Database version.
        version: u64,
        /// The record.
        record: DbRecord,
    },
}

impl Op {
    fn code(&self) -> i32 {
        match self {
            Op::State { .. } => 0,
            Op::EvalStep { .. } => 1,
            Op::StepInv { .. } => 2,
            Op::Adjust { .. } => 3,
            Op::Reverse { .. } => 4,
            Op::Transform { .. } => 5,
            Op::Gps { .. } => 6,
            Op::Eq { .. } => 7,
            Op::Copy => 8,
            Op::ReadDb { .. } => 9,
        }
    }
}

fn finite(v: f64, max: f64, what: &str) -> Result<f64, FfiError> {
    if !v.is_finite() || v.abs() > max {
        return Err(bad(format!("{what} {v} not finite or too large")));
    }
    Ok(v)
}

fn vec3(v: [f64; 3], what: &str, o: &mut Vec<f64>) -> Result<(), FfiError> {
    for x in v {
        o.push(finite(x, MAX_COORD, what)?);
    }
    Ok(())
}

fn ivl(v: [f64; 2], o: &mut Vec<f64>) -> Result<(), FfiError> {
    for x in v {
        o.push(finite(x, MAX_PARAM, "interval end")?);
    }
    Ok(())
}

fn encode_hyperbola(h: &HyperbolaIn, o: &mut Vec<f64>) -> Result<(), FfiError> {
    vec3(h.center, "center", o)?;
    vec3(h.x_axis, "x axis", o)?;
    vec3(h.y_axis, "y axis", o)?;
    ivl(h.domain, o)?;
    o.push(finite(h.semi_axis, MAX_COORD, "semi axis")?);
    o.push(finite(h.semi_image_axis, MAX_COORD, "semi imaginary axis")?);
    if !(2..=3).contains(&h.dim) {
        return Err(bad("dimension must be 2 or 3"));
    }
    o.push(h.dim as f64);
    Ok(())
}

/// Values per control point of a record.
fn per_point(r: &DbRecord) -> usize {
    2 + usize::from(r.dim != 2) + usize::from(r.rat != 0)
}

/// Checks a record: `1 <= p <= n <= MAX_DB_INDEX`, flags in range, the exact number
/// of finite values, positive weights.
fn check_record(r: &DbRecord) -> Result<(), FfiError> {
    for &x in &r.head {
        finite(x, MAX_COORD, "record head")?;
    }
    if !(1..=MAX_DB_INDEX).contains(&r.n) || r.p < 1 || i64::from(r.p) > r.n {
        return Err(bad("record sizes"));
    }
    if !(0..=1).contains(&r.rat) || !(2..=3).contains(&r.dim) || r.kt > 4 || r.form > 6 {
        return Err(bad("record flags"));
    }
    let per = per_point(r);
    let n_pts = usize::try_from(r.n + 1).map_err(|_| bad("record sizes"))?;
    let n_knots = usize::try_from(r.n + i64::from(r.p) + 2).map_err(|_| bad("record sizes"))?;
    if r.vals.len() != n_pts * per + n_knots {
        return Err(bad("record value count"));
    }
    for &x in &r.vals {
        finite(x, MAX_COORD, "record value")?;
    }
    if r.rat != 0 {
        for c in r.vals.chunks(per).take(n_pts) {
            if c.last().is_none_or(|&w| w <= 0.0) {
                return Err(bad("record weight"));
            }
        }
    }
    Ok(())
}

/// The stream bytes of `record` as `SmHyperbola::WriteToDB` writes them (ASCII: one
/// group per line with upstream's labels, values as `{:.17e}`; binary: native-endian
/// doubles, `long` / `short` header fields).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a record [`eval`] would reject.
pub fn db_bytes(binary: bool, version: u64, r: &DbRecord) -> Result<Vec<u8>, FfiError> {
    check_record(r)?;
    let per = per_point(r);
    let n_pts = usize::try_from(r.n + 1).map_err(|_| bad("record sizes"))?;
    let (pts, knots) = r.vals.split_at(n_pts * per);
    if binary {
        let mut b = Vec::new();
        for x in &r.head {
            b.extend_from_slice(&x.to_ne_bytes());
        }
        b.extend_from_slice(&r.n.to_ne_bytes());
        b.extend_from_slice(&r.p.to_ne_bytes());
        b.extend_from_slice(&r.rat.to_ne_bytes());
        b.extend_from_slice(&r.dim.to_ne_bytes());
        if version > 32 {
            b.extend_from_slice(&r.kt.to_ne_bytes());
            b.extend_from_slice(&r.form.to_ne_bytes());
        }
        for x in &r.vals {
            b.extend_from_slice(&x.to_ne_bytes());
        }
        Ok(b)
    } else {
        let f = |x: f64| format!("{x:.17e}");
        let [ox, oy, oz, xx, xy, xz, yx, yy, yz, d0, d1, a, b] = r.head;
        let mut t = String::new();
        let _ = writeln!(t, "{} {} {} SmHyperbola Axis Origin ", f(ox), f(oy), f(oz));
        let _ = writeln!(t, "{} {} {} SmHyperbola X Axis", f(xx), f(xy), f(xz));
        let _ = writeln!(t, "{} {} {} SmHyperbola Y Axis", f(yx), f(yy), f(yz));
        let _ = writeln!(
            t,
            "{} {} SmHyperbola anular arc domain in degrees ",
            f(d0),
            f(d1)
        );
        let _ = writeln!(t, "{} SmHyperbola Length of semi axis           ", f(a));
        let _ = writeln!(t, "{} SmHyperbola Length of semi imaginary axis ", f(b));
        let _ = write!(t, "{}\n{}\n{}\n{}\n", r.n, r.p, r.rat, r.dim);
        if version > 32 {
            let _ = write!(t, "{}\n{}\n", r.kt, r.form);
        }
        for c in pts.chunks(per) {
            let s: Vec<String> = c.iter().map(|&x| f(x)).collect();
            t.push_str(&s.join(" "));
            t.push('\n');
        }
        for &x in knots {
            t.push_str(&f(x));
            t.push('\n');
        }
        Ok(t.into_bytes())
    }
}

fn encode(this: Option<&HyperbolaIn>, op: &Op) -> Result<Vec<f64>, FfiError> {
    let mut o = Vec::new();
    match (op, this) {
        (Op::ReadDb { .. }, _) => {}
        (_, Some(h)) => encode_hyperbola(h, &mut o)?,
        (_, None) => return Err(bad("member op without a curve")),
    }
    match op {
        Op::State { types } => {
            if types.len() > MAX_TYPES {
                return Err(bad("too many types"));
            }
            o.push(types.len() as f64);
            o.extend(types.iter().map(|&t| t as f64));
        }
        Op::EvalStep { t, nd } => {
            o.push(finite(*t, MAX_PARAM * 2.0, "parameter")?);
            if *nd > MAX_DERIVS {
                return Err(bad("too many derivatives"));
            }
            o.push(*nd as f64);
        }
        Op::StepInv { p, classify } => {
            vec3(*p, "point", &mut o)?;
            o.push(f64::from(u8::from(*classify)));
        }
        Op::Adjust { ivl: v } | Op::Reverse { ivl: v } => ivl(*v, &mut o)?,
        Op::Transform { o: po, x, y, scale } => {
            vec3(*po, "placement origin", &mut o)?;
            vec3(*x, "placement x", &mut o)?;
            vec3(*y, "placement y", &mut o)?;
            o.push(f64::from(u8::from(scale.is_some())));
            vec3(scale.unwrap_or([1.0; 3]), "scale", &mut o)?;
        }
        Op::Gps {
            ivl: v,
            op,
            p,
            tol,
            target,
            req,
        } => {
            ivl(*v, &mut o)?;
            if ![0, 3, 4, 5].contains(op) || !(0..=1).contains(req) {
                return Err(bad("solver operation or request"));
            }
            o.push(f64::from(*op));
            vec3(*p, "point", &mut o)?;
            let tol = finite(*tol, MAX_COORD, "tolerance")?;
            if tol <= 0.0 {
                return Err(bad("tolerance must be positive"));
            }
            o.push(tol);
            o.push(f64::from(u8::from(target.is_some())));
            o.push(finite(target.unwrap_or(0.0), MAX_COORD, "target")?);
            o.push(f64::from(*req));
        }
        Op::Eq { other } => encode_hyperbola(other, &mut o)?,
        Op::Copy => {}
        Op::ReadDb {
            dim,
            binary,
            version,
            record,
        } => {
            if !(2..=3).contains(dim) || !(30..=64).contains(version) {
                return Err(bad("dimension or version"));
            }
            let bytes = db_bytes(*binary, *version, record)?;
            o.extend([
                *dim as f64,
                f64::from(u8::from(*binary)),
                *version as f64,
                bytes.len() as f64,
            ]);
            o.extend(bytes.iter().map(|&b| f64::from(b)));
        }
    }
    Ok(o)
}

/// Runs `op` on the hyperbola `this` (ignored for [`Op::ReadDb`]); returns the flat
/// output described in the module docs.
///
/// Accepts finite inputs only: coordinates, lengths and tolerances up to
/// [`MAX_COORD`], analytic parameters up to [`MAX_PARAM`] (evaluation parameters up to
/// twice that), dimensions 2 or 3, at most [`MAX_DERIVS`] derivatives and
/// [`MAX_TYPES`] types, the solver operations `SM_SO_INTERSECT`, `_MINIMIZE`,
/// `_MAXIMIZE`, `_NORMALIZE` with a positive tolerance and `SM_SR_SINGLE` / `_ALL`,
/// database versions 30 to 64 and records that pass `check_record`.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for rejected inputs; [`FfiError::Status`] for a shim
/// failure.
pub fn eval(this: Option<&HyperbolaIn>, op: &Op) -> Result<Vec<f64>, FfiError> {
    let input = encode(this, op)?;
    let code = op.code();
    call_with_buffer(256, |buf, len| {
        // SAFETY: `input` is valid for `input.len()` reads and holds every value the shim
        // reads for `code` (checked by `encode`); `buf` is valid for `buf.len()` writes;
        // `len` is a valid out pointer.
        unsafe {
            sys::sc_curve_hyperbola_eval(
                code,
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
