//! FFI wrappers for unit `curve.iso_curve`.
//!
//! Shim: `harness/cpp/shim/curve_iso_curve.cpp`.
//! Upstream: `source/SMLib/inc/SmIsoCurve.h`, `source/SMLib/src/SmIsoCurve.cpp`
//! (`SmIsoCurve` on an `SmBSplineSurface`).
//!
//! | Rust   | shim                     | upstream                           |
//! |--------|--------------------------|------------------------------------|
//! | [`op`] | `sc_curve_iso_curve_op`  | one `SmIsoCurve` method per [`Op`] |
//!
//! Input layout (`in`): curve block A (absent for [`Op::ReadDb`]), then the op's
//! parameters. A curve block is `has_srf [n m p q r s cpts u v] surf_param iso
//! from_left clamp` (see [`CurveBlock`]). Outputs start with the `SmStatus` for the
//! status-bearing ops; values follow only on success.

use crate::{call_with_buffer, FfiError};

/// NLib's "no weight" marker of a non-rational control point (`NOW`).
pub const NOW: f64 = -f64::MAX;

mod sys {
    extern "C" {
        pub fn sc_curve_iso_curve_op(
            op: i32,
            input: *const f64,
            n_in: i64,
            data: *const u8,
            data_len: i64,
            file_type: i32,
            version: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Highest derivative count [`Op::Evaluate`] accepts.
pub const MAX_DERIVS: usize = 8;

/// Highest degree of a [`Nurb`].
pub const MAX_DEGREE: usize = 9;

/// Most control points per direction of a [`Nurb`].
pub const MAX_POINTS: usize = 64;

/// One `SmIsoCurve` operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Op {
    /// Iso parameter, surface parameter type, from-left flag (members), `GetType`,
    /// `GetDim`, `GetNaturalInterval` (2).
    State = 0,
    /// `GetKnots(.., &mults, ivl)` (params `has_ivl lo hi`): status, count, knots,
    /// count, multiplicities.
    Knots = 1,
    /// `Evaluate(t, n, from_left, .., non_zero)` (params `t n from_left non_zero`):
    /// status, `n + 1` vectors. Not on a surface without NURB.
    Evaluate = 2,
    /// `EvaluatePoint(t)` (param `t`): status, point. Not on a surface without NURB.
    EvaluatePoint = 3,
    /// `CalculateContinuities(.., tol)` (param `tol`): status, minimum, count,
    /// continuities. Not on a surface without NURB.
    Continuities = 4,
    /// `A == B` and `B == A` (params `mode [block B | curve params]`; mode 1: `A == A`
    /// only).
    Eq = 5,
    /// `IsKindOf(t)` (param `t`).
    IsKindOf = 6,
    /// `GetType`, `GetTypeString` characters, `-1`, `GetClassString` characters.
    TypeStrings = 7,
    /// `CreateUVIsoLine`: status, then the line's type, dimension, interval and its
    /// `EvaluatePoint` status and point at both ends.
    UvIsoLine = 8,
    /// `SmIsoCurve::ReadFromDB` of `data`: status, then the [`Op::State`] values.
    ReadDb = 9,
}

/// A surface NURB as stored (homogeneous control points, rows in u).
#[derive(Debug, Clone)]
pub struct Nurb {
    /// Control net, `[x, y, z, w]` (`w` = [`NOW`] when non-rational), `net[i][j]` with
    /// `i` in u.
    pub net: Vec<Vec<[f64; 4]>>,
    /// Degree in u.
    pub p: usize,
    /// Degree in v.
    pub q: usize,
    /// u knots.
    pub u: Vec<f64>,
    /// v knots.
    pub v: Vec<f64>,
}

impl Nurb {
    fn push(&self, o: &mut Vec<f64>) -> Result<(), FfiError> {
        let bad = |s: &str| Err(FfiError::InvalidArgument(s.into()));
        let rows = self.net.len();
        let cols = self.net.first().map_or(0, Vec::len);
        if rows == 0 || cols == 0 || rows > MAX_POINTS || cols > MAX_POINTS {
            return bad("net size");
        }
        if self.net.iter().any(|r| r.len() != cols) {
            return bad("ragged net");
        }
        if !(1..=MAX_DEGREE).contains(&self.p) || !(1..=MAX_DEGREE).contains(&self.q) {
            return bad("degree");
        }
        if self.p >= rows || self.q >= cols {
            return bad("degree too high for the net");
        }
        if self.u.len() != rows + self.p + 1 || self.v.len() != cols + self.q + 1 {
            return bad("knot count");
        }
        let finite = |k: &[f64]| k.iter().all(|x| x.is_finite());
        if !finite(&self.u) || !finite(&self.v) {
            return bad("knots not finite");
        }
        let sorted = |k: &[f64]| k.iter().zip(k.iter().skip(1)).all(|(a, b)| a <= b);
        if !sorted(&self.u) || !sorted(&self.v) {
            return bad("knots not sorted");
        }
        let clamped = |k: &[f64], d: usize| match (k.first(), k.last()) {
            (Some(&f), Some(&l)) => {
                f < l
                    && k.iter().take(d + 1).all(|&x| x == f)
                    && k.iter().rev().take(d + 1).all(|&x| x == l)
            }
            _ => false,
        };
        if !clamped(&self.u, self.p) || !clamped(&self.v, self.q) {
            return bad("knots not clamped");
        }
        if self.net.iter().flatten().any(|c| {
            !c.iter().all(|x| x.is_finite()) || c.get(3).is_none_or(|&w| w != NOW && w <= 0.0)
        }) {
            return bad("control points");
        }
        let n = rows - 1;
        let m = cols - 1;
        o.extend([
            n as f64,
            m as f64,
            self.p as f64,
            self.q as f64,
            (self.u.len() - 1) as f64,
            (self.v.len() - 1) as f64,
        ]);
        for c in self.net.iter().flatten() {
            o.extend_from_slice(c);
        }
        o.extend_from_slice(&self.u);
        o.extend_from_slice(&self.v);
        Ok(())
    }
}

/// The curve parameters of `SmIsoCurve(surface, surf_param, iso, from_left, clamp)`.
#[derive(Debug, Clone, Copy)]
pub struct CurveParams {
    /// `SmSurfParamType` value `0..=9`.
    pub surf_param: i32,
    /// Iso parameter.
    pub iso: f64,
    /// `bFromLeft`.
    pub from_left: bool,
    /// `bClampInput`.
    pub clamp: bool,
}

impl CurveParams {
    fn push(&self, o: &mut Vec<f64>) -> Result<(), FfiError> {
        if !(0..=9).contains(&self.surf_param) {
            return Err(FfiError::InvalidArgument("surface parameter type".into()));
        }
        o.extend([
            f64::from(self.surf_param),
            self.iso,
            f64::from(u8::from(self.from_left)),
            f64::from(u8::from(self.clamp)),
        ]);
        Ok(())
    }
}

/// A curve: its surface (`None` = `SmBSplineSurface()` without NURB) and parameters.
#[derive(Debug, Clone)]
pub struct CurveBlock {
    /// The base surface's NURB.
    pub srf: Option<Nurb>,
    /// The curve parameters.
    pub params: CurveParams,
}

impl CurveBlock {
    fn push_surface(srf: Option<&Nurb>, o: &mut Vec<f64>) -> Result<(), FfiError> {
        match srf {
            None => o.push(0.0),
            Some(n) => {
                o.push(1.0);
                n.push(o)?;
            }
        }
        Ok(())
    }

    fn push(&self, o: &mut Vec<f64>) -> Result<(), FfiError> {
        Self::push_surface(self.srf.as_ref(), o)?;
        self.params.push(o)
    }
}

/// The second curve of [`Op::Eq`].
#[derive(Debug, Clone)]
pub enum Other {
    /// A curve on its own surface.
    Own(CurveBlock),
    /// `A == A`.
    Itself,
    /// A curve on A's surface (the same surface object).
    SameSurface(CurveParams),
}

/// Operation parameters.
#[derive(Debug, Clone)]
pub enum Params {
    /// No parameters ([`Op::State`], [`Op::TypeStrings`], [`Op::UvIsoLine`]).
    None,
    /// [`Op::Knots`]: optional interval `(lo, hi)`.
    Knots(Option<(f64, f64)>),
    /// [`Op::Evaluate`]: `t`, derivative count, from left, non-zero tangents.
    Evaluate(f64, usize, bool, bool),
    /// [`Op::EvaluatePoint`] parameter or [`Op::Continuities`] tolerance.
    Value(f64),
    /// [`Op::Eq`].
    Eq(Other),
    /// [`Op::IsKindOf`] type code.
    Type(i64),
    /// [`Op::ReadDb`]: bytes, binary flag, database version.
    Db(Vec<u8>, bool, i64),
}

/// Run `op` on curve `a` (ignored for [`Op::ReadDb`]) with `params`.
pub fn op(op: Op, a: &CurveBlock, params: &Params) -> Result<Vec<f64>, FfiError> {
    let bad = |s: &str| Err(FfiError::InvalidArgument(s.into()));
    let mut input = Vec::new();
    let mut data: &[u8] = &[];
    let mut file_type = 0i32;
    let mut version = 0i64;
    if op != Op::ReadDb {
        a.push(&mut input)?;
    }
    let needs_nurb = matches!(op, Op::Evaluate | Op::EvaluatePoint | Op::Continuities);
    if needs_nurb && a.srf.is_none() {
        return bad("evaluation needs a surface with NURB");
    }
    match (op, params) {
        (Op::State | Op::TypeStrings | Op::UvIsoLine, Params::None) => {}
        (Op::Knots, Params::Knots(ivl)) => {
            let (lo, hi) = ivl.unwrap_or((0.0, 0.0));
            input.extend([f64::from(u8::from(ivl.is_some())), lo, hi]);
        }
        (Op::Evaluate, Params::Evaluate(t, n, fl, nz)) => {
            if *n > MAX_DERIVS || !t.is_finite() {
                return bad("evaluate parameters");
            }
            input.extend([
                *t,
                *n as f64,
                f64::from(u8::from(*fl)),
                f64::from(u8::from(*nz)),
            ]);
        }
        (Op::EvaluatePoint | Op::Continuities, Params::Value(t)) => {
            if !t.is_finite() {
                return bad("parameter");
            }
            input.push(*t);
        }
        (Op::Eq, Params::Eq(other)) => match other {
            Other::Own(b) => {
                input.push(0.0);
                b.push(&mut input)?;
            }
            Other::Itself => input.push(1.0),
            Other::SameSurface(p) => {
                input.push(2.0);
                p.push(&mut input)?;
            }
        },
        (Op::IsKindOf, Params::Type(t)) => {
            if t.unsigned_abs() > (1 << 40) {
                return bad("type code");
            }
            input.push(*t as f64);
        }
        (Op::ReadDb, Params::Db(bytes, binary, ver)) => {
            if !(0..=100).contains(ver) {
                return bad("version");
            }
            data = bytes;
            file_type = i32::from(*binary);
            version = *ver;
        }
        _ => return bad("parameters do not match the op"),
    }
    let n_in = i64::try_from(input.len()).map_err(|_| FfiError::InvalidArgument("input".into()))?;
    let data_len =
        i64::try_from(data.len()).map_err(|_| FfiError::InvalidArgument("data too long".into()))?;
    call_with_buffer(64, |buf: &mut [f64], len| {
        // SAFETY: `input` is valid for `n_in` reads and `data` for `data_len` reads (the
        // shim bounds-checks its reads of `input` against `n_in`); `buf` is valid for
        // `buf.len()` writes; `len` is a valid i64; nothing is retained.
        unsafe {
            sys::sc_curve_iso_curve_op(
                op as i32,
                input.as_ptr(),
                n_in,
                data.as_ptr(),
                data_len,
                file_type,
                version,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
