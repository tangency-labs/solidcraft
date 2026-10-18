//! FFI wrappers for unit `curve.crv_on_surf`.
//!
//! Shim: `harness/cpp/shim/curve_crv_on_surf.cpp` (`sc_curve_crv_on_surf_eval`).
//! Upstream: `source/SMLib/src/SmCrvOnSurf.cpp`, `source/SMLib/inc/SmCrvOnSurf.h`.
//!
//! Object input ([`Op::Object`]): an object block building the `SmCrvOnSurf` `c`, then
//! `nops` and `nops` ops, each its code ([`ObjOp`]) and parameters, run in order on `c`.
//! The block is the UV curve (`0 n p m cpts(x y z w) knots`: a 2D `SmBSplineCurve`;
//! or `1 x0 y0 x1 y1`: the UV-line constructor), the surface (`n m p q r s cpts(x y z
//! w, v fastest) u v`: an `SmBSplineSurface`), `has_domain umin vmin umax vmax`,
//! `owner` (the constructor's owner flag) and `has_cache [need nb B[nb]]` (loaded into
//! `m_bNeedBreaks`, `m_vBreaks`). Every op output starts with its status (`0` =
//! `SM_SUCCESS`, else the `SmStatus`); the rest:
//!
//! | [`ObjOp`]      | upstream                              | parameters                     | outputs |
//! |----------------|---------------------------------------|--------------------------------|---------|
//! | `Info`         | `GetType`, `GetDim`, `GetDegree`, `GetNaturalInterval`, `IsAnalytic`, `IsBounded`, `IsKindOf` | `nt T[nt]` | `type dim deg a b analytic bounded kind[nt]` state |
//! | `State`        | (members)                             |                                | state |
//! | `Eval`         | `Evaluate`                            | `t n left nonzero`             | `(n+1)` vectors (on success) |
//! | `EvalPoint`    | `EvaluatePoint`                       | `t`                            | point (on success) |
//! | `EvalUv`       | `EvaluateUVPoint`                     | `t`                            | `u v` (on success) |
//! | `EvalNormal`   | `EvaluateSurfaceNormal`               | `t left`                       | `uv uv' normal` (on success) |
//! | `Eval1st`      | `EvaluateSurface1stDerivDerivs`       | `t n left`                     | `Su Sv Su' Sv'` (on success) |
//! | `Knots`        | `GetKnots`                            | `has_ivl a b want`             | `nk K [nm M]` state |
//! | `Nnk`          | `GetNumberNaturalKnots`               |                                | `count` state |
//! | `Conts`        | `CalculateContinuities`               | `angle`                        | `min nc C` state |
//! | `Equal`        | `operator==` (both ways)              | `self` or `0` object block     | `flag [flag]` |
//! | `Copy`         | `Copy` (copy constructor)             |                                | state of the copy, `copy == c` (on success) |
//! | `EditParam`    | `EditParameterization`                | `a b`                          | state |
//! | `Reverse`      | `ReverseParameterization`             | `a b`                          | `[a' b']` state |
//! | `Trim`         | `Trim(ivl, notify, FALSE)`            | `a b mode` (0 `TRUE`, 1 `FALSE`, 2 `UNSURE`) | `a' b'` state |
//! | `Transform`    | `Transform`                           | `O X Y has_scale S`            | state |
//! | `Approx`       | `ApproximateCurve`                    | `nb B tol match`               | `gap made [curve]` state |
//! | `GetOrCreate`  | `GetOrCreateBSApproxPointer`          | `tol has_b nb B has_match match` | `made [curve]` state |
//! | `Bbox`         | `CalculateBoundingBox` (normal box)   | `a b`                          | `[min max]` state |
//! | `Drop`         | `DropPoint`                           | `a b P has_v V tol has_guess guess op` | `ok param dist` state |
//! | `Gap`          | `SurfaceCurveMaxDistanceBetween`      | `self` or `0` block, `a b s0 s1 ns tol has_max max` | `dist dev t1 t2 t3 t4` (on success) |
//!
//! A state is `owner umin vmin umax vmax need nb B gap matches has_approx [nurb]`, the
//! UV curve (`type dim nurb`, the B-spline part of a line) and the surface's NURB
//! (`n m cpts u v`); a NURB curve is `n p m cpts knots`.
//!
//! Static inputs: [`Op::ReadDb`] (`file_type version nb bytes[nb]`:
//! `SmCrvOnSurf::ReadFromDB`, outputs `status made [state]`) and [`Op::WriteDb`]
//! (`file_type version` object block: the bytes of `WriteToDB`, a generator helper).
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour:
//! well-formed clamped 2D B-spline UV curves and clamped surfaces, a UV curve whose
//! control polygon lies in the surface's domain (an op list on a curve outside it may
//! only evaluate, compare and copy: the curve classification is not asked to handle
//! it), parameters near the curve's range, `GetKnots` intervals inside the range
//! holding an end of the unedited curve (upstream reads the first UV knot found),
//! sorted breaks inside the range, approximation tolerances in `[1e-7, 1]`, valid placements and solver operations, and database text that is a
//! complete record when ASCII (truncated ASCII reads uninitialised variables).

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

/// Largest control point index of a curve.
pub const MAX_INDEX: usize = 64;
/// Most control points per direction of a surface.
pub const MAX_POINTS: usize = 32;
/// Largest degree.
pub const MAX_DEGREE: usize = 7;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1e5;
/// Parameters may lie this many interval lengths outside the curve's range.
pub const MAX_REL_DIST: f64 = 2.0;
/// Most breaks, knots, type codes or ops.
pub const MAX_ARRAY: usize = 256;
/// Most bytes of a database stream.
pub const MAX_DB_BYTES: usize = 1 << 16;
/// Highest derivative count of `Evaluate`.
pub const MAX_DERIVS: usize = 6;

/// NLib's "no z" marker of a 2D control point.
pub const NOZ: f64 = f64::MAX;
/// NLib's "no weight" marker of a non-rational control point.
pub const NOW: f64 = -f64::MAX;

/// The shim entry kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// An object block and an op list.
    Object,
    /// `SmCrvOnSurf::ReadFromDB`.
    ReadDb,
    /// `WriteToDB` of an object block.
    WriteDb,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::Object => 0,
            Op::ReadDb => 40,
            Op::WriteDb => 41,
        }
    }
}

/// The object ops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum ObjOp {
    Info,
    Eval,
    EvalPoint,
    EvalUv,
    EvalNormal,
    Eval1st,
    Knots,
    Nnk,
    Conts,
    Equal,
    Copy,
    EditParam,
    Reverse,
    Trim,
    Transform,
    Approx,
    GetOrCreate,
    Bbox,
    Drop,
    Gap,
    State,
}

impl ObjOp {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            ObjOp::Info => 0,
            ObjOp::Eval => 1,
            ObjOp::EvalPoint => 2,
            ObjOp::EvalUv => 3,
            ObjOp::EvalNormal => 4,
            ObjOp::Eval1st => 5,
            ObjOp::Knots => 6,
            ObjOp::Nnk => 7,
            ObjOp::Conts => 8,
            ObjOp::Equal => 9,
            ObjOp::Copy => 10,
            ObjOp::EditParam => 11,
            ObjOp::Reverse => 12,
            ObjOp::Trim => 13,
            ObjOp::Transform => 14,
            ObjOp::Approx => 15,
            ObjOp::GetOrCreate => 16,
            ObjOp::Bbox => 17,
            ObjOp::Drop => 18,
            ObjOp::Gap => 19,
            ObjOp::State => 20,
        }
    }

    fn from_code(c: i64) -> Option<Self> {
        [
            ObjOp::Info,
            ObjOp::Eval,
            ObjOp::EvalPoint,
            ObjOp::EvalUv,
            ObjOp::EvalNormal,
            ObjOp::Eval1st,
            ObjOp::Knots,
            ObjOp::Nnk,
            ObjOp::Conts,
            ObjOp::Equal,
            ObjOp::Copy,
            ObjOp::EditParam,
            ObjOp::Reverse,
            ObjOp::Trim,
            ObjOp::Transform,
            ObjOp::Approx,
            ObjOp::GetOrCreate,
            ObjOp::Bbox,
            ObjOp::Drop,
            ObjOp::Gap,
            ObjOp::State,
        ]
        .into_iter()
        .find(|o| i64::from(o.code()) == c)
    }

    /// Whether the op is safe on a UV curve outside the surface's domain.
    fn safe_outside(self) -> bool {
        matches!(
            self,
            ObjOp::Info
                | ObjOp::State
                | ObjOp::Eval
                | ObjOp::EvalPoint
                | ObjOp::EvalUv
                | ObjOp::Equal
                | ObjOp::Copy
        )
    }
}

mod sys {
    extern "C" {
        pub fn sc_curve_crv_on_surf_eval(
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

/// Clamped, sorted, finite knots of degree `p` with interior multiplicities at most
/// `p`; returns the range.
fn check_knots(knots: &[f64], p: usize, what: &str) -> Result<(f64, f64), FfiError> {
    if knots.iter().any(|k| !k.is_finite() || k.abs() > MAX_COORD)
        || knots.windows(2).any(|w| w.first() > w.last())
    {
        return Err(bad(format!("{what}: knots not non-decreasing")));
    }
    let (Some(&a), Some(&b)) = (knots.first(), knots.last()) else {
        return Err(bad("empty knots"));
    };
    let m = knots.len() - 1;
    let short = || bad(format!("{what}: knots too short"));
    let head = knots.get(..=p).ok_or_else(short)?;
    let tail = knots
        .get(m.checked_sub(p).ok_or_else(short)?..)
        .ok_or_else(short)?;
    let inner = knots.get(p + 1..m - p).ok_or_else(short)?;
    if a >= b || head.iter().any(|&k| k != a) || tail.iter().any(|&k| k != b) {
        return Err(bad(format!("{what}: knots not clamped")));
    }
    if b - a < 1e-3 * a.abs().max(b.abs()).max(1.0) {
        return Err(bad(format!("{what}: parameter range too small")));
    }
    if inner.iter().any(|&k| k == a || k == b) {
        return Err(bad(format!("{what}: end multiplicity above p + 1")));
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
    Ok((a, b))
}

/// A 2D B-spline `n p m cpts knots`; returns its range and the Euclidean control
/// points.
#[allow(clippy::type_complexity)]
fn uv_bspline(rd: &mut Reader<'_>) -> Result<((f64, f64), Vec<[f64; 2]>), FfiError> {
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad("uv curve: need 1 <= p <= n and m = n + p + 1"));
    }
    let cps = rd.take(4 * (n + 1))?;
    let knots = rd.take(m + 1)?;
    let (cpts, _) = cps.as_chunks::<4>();
    let [_, _, _, w0] = *cpts.first().ok_or_else(|| bad("empty curve"))?;
    let mut euclid = Vec::with_capacity(cpts.len());
    for &[x, y, z, w] in cpts {
        if z != NOZ {
            return Err(bad("uv curve: not 2D"));
        }
        if (w == NOW) != (w0 == NOW) {
            return Err(bad("uv curve: mixed rationality"));
        }
        if !x.is_finite() || !y.is_finite() || (w != NOW && !(1e-3..=1e3).contains(&w)) {
            return Err(bad("uv curve: bad control point"));
        }
        let (ex, ey) = if w == NOW { (x, y) } else { (x / w, y / w) };
        if ex.abs() > MAX_COORD || ey.abs() > MAX_COORD {
            return Err(bad("uv curve: point out of range"));
        }
        euclid.push([ex, ey]);
    }
    Ok((check_knots(knots, p, "uv curve")?, euclid))
}

/// A surface `n m p q r s cpts u v`; returns its UV domain.
fn surface(rd: &mut Reader<'_>) -> Result<[f64; 4], FfiError> {
    let n = rd.index(MAX_POINTS - 1)?;
    let m = rd.index(MAX_POINTS - 1)?;
    let p = rd.index(MAX_DEGREE)?;
    let q = rd.index(MAX_DEGREE)?;
    let r = rd.index(MAX_POINTS + MAX_DEGREE)?;
    let s = rd.index(MAX_POINTS + MAX_DEGREE)?;
    if p == 0 || q == 0 || p > n || q > m || r != n + p + 1 || s != m + q + 1 {
        return Err(bad("surface: bad sizes"));
    }
    let cps = rd.take(4 * (n + 1) * (m + 1))?;
    let (cpts, _) = cps.as_chunks::<4>();
    let [_, _, _, w0] = *cpts.first().ok_or_else(|| bad("empty surface"))?;
    for &[x, y, z, w] in cpts {
        if (w == NOW) != (w0 == NOW) {
            return Err(bad("surface: mixed rationality"));
        }
        if !x.is_finite()
            || !y.is_finite()
            || !z.is_finite()
            || (w != NOW && !(1e-3..=1e3).contains(&w))
        {
            return Err(bad("surface: bad control point"));
        }
        let d = if w == NOW { 1.0 } else { w };
        if [x / d, y / d, z / d].iter().any(|c| c.abs() > MAX_COORD) {
            return Err(bad("surface: point out of range"));
        }
    }
    let u = rd.take(r + 1)?;
    let v = rd.take(s + 1)?;
    let (u0, u1) = check_knots(u, p, "surface u")?;
    let (v0, v1) = check_knots(v, q, "surface v")?;
    Ok([u0, v0, u1, v1])
}

/// A validated object block.
#[derive(Debug, Clone, Copy)]
struct Block {
    a: f64,
    b: f64,
    owner_flag: u64,
    /// The UV curve's control polygon lies in the surface domain.
    inside: bool,
    /// An op changed the UV curve's parameterisation (its knots are no longer known).
    edited: bool,
}

fn in_domain(p: [f64; 2], d: [f64; 4]) -> bool {
    let [u0, v0, u1, v1] = d;
    let tu = 1e-9 * (u1 - u0);
    let tv = 1e-9 * (v1 - v0);
    p[0] >= u0 - tu && p[0] <= u1 + tu && p[1] >= v0 - tv && p[1] <= v1 + tv
}

/// A parameter within [`MAX_REL_DIST`] ranges of the curve's.
fn near(t: f64, a: f64, b: f64) -> Result<f64, FfiError> {
    let far = MAX_REL_DIST * (b - a);
    if !t.is_finite() || t < a - far || t > b + far {
        return Err(bad("parameter too far outside the curve"));
    }
    Ok(t)
}

/// An object block (see the module docs).
fn object_block(rd: &mut Reader<'_>) -> Result<Block, FfiError> {
    let kind = rd.index(1)?;
    let (range, pts) = if kind == 0 {
        uv_bspline(rd)?
    } else {
        let x0 = rd.finite()?;
        let y0 = rd.finite()?;
        let x1 = rd.finite()?;
        let y1 = rd.finite()?;
        let d = ((x1 - x0) * (x1 - x0) + (y1 - y0) * (y1 - y0)).sqrt();
        if d < 1e-6 {
            return Err(bad("uv line: coincident points"));
        }
        ((0.0, 1.0), vec![[x0, y0], [x1, y1]])
    };
    let dom = surface(rd)?;
    rd.flag()?;
    for _ in 0..4 {
        rd.finite()?;
    }
    let owner_flag = rd.index(7)? as u64;
    let (a, b) = range;
    if rd.flag()? {
        rd.flag()?;
        let nb = rd.index(MAX_ARRAY)?;
        let mut prev = f64::NEG_INFINITY;
        for _ in 0..nb {
            let t = near(rd.finite()?, a, b)?;
            if t <= prev {
                return Err(bad("cache: breaks not increasing"));
            }
            prev = t;
        }
    }
    let inside = pts.iter().all(|&p| in_domain(p, dom));
    Ok(Block {
        a,
        b,
        owner_flag,
        inside,
        edited: false,
    })
}

fn param(rd: &mut Reader<'_>, c: &Block) -> Result<f64, FfiError> {
    let t = rd.finite()?;
    near(t, c.a, c.b)
}

/// An interval `x y` inside the curve's range with `x < y`.
fn inner_interval(rd: &mut Reader<'_>, c: &Block) -> Result<(f64, f64), FfiError> {
    let x = rd.finite()?;
    let y = rd.finite()?;
    let eps = 1e-12 * (c.b - c.a);
    if !(x >= c.a - eps && y <= c.b + eps && y - x > 1e-4 * (c.b - c.a)) {
        return Err(bad("interval not inside the curve range"));
    }
    Ok((x, y))
}

/// Breaks `nb B[nb]`: increasing, inside the range (any count).
fn breaks(rd: &mut Reader<'_>, c: &Block) -> Result<(), FfiError> {
    let nb = rd.index(MAX_ARRAY)?;
    let mut prev = f64::NEG_INFINITY;
    for _ in 0..nb {
        let t = rd.finite()?;
        let eps = 1e-12 * (c.b - c.a);
        if t < c.a - eps || t > c.b + eps || t <= prev {
            return Err(bad("breaks not increasing inside the range"));
        }
        prev = t;
    }
    Ok(())
}

fn tolerance(rd: &mut Reader<'_>) -> Result<f64, FfiError> {
    let t = rd.finite()?;
    if !(1e-7..=1.0).contains(&t) {
        return Err(bad("tolerance out of range"));
    }
    Ok(t)
}

/// A unit-ish, non-parallel placement `O X Y`.
fn placement(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    rd.vec3()?;
    let x = rd.vec3()?;
    let y = rd.vec3()?;
    let len = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let dot = x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
    if !(0.5..=2.0).contains(&len(x)) || !(0.5..=2.0).contains(&len(y)) {
        return Err(bad("placement: axis length"));
    }
    if dot.abs() > 0.9 * len(x) * len(y) {
        return Err(bad("placement: axes nearly parallel"));
    }
    Ok(())
}

fn validate_op(op: ObjOp, rd: &mut Reader<'_>, c: &mut Block) -> Result<(), FfiError> {
    if !c.inside && !op.safe_outside() {
        return Err(bad("op on a UV curve outside the surface domain"));
    }
    match op {
        ObjOp::Info => {
            let nt = rd.index(MAX_ARRAY)?;
            for _ in 0..nt {
                let v = rd.num()?;
                if v.fract() != 0.0 || v.abs() > f64::from(1 << 30) {
                    return Err(bad("bad type"));
                }
            }
        }
        ObjOp::State | ObjOp::Nnk | ObjOp::Copy => {}
        ObjOp::Eval => {
            param(rd, c)?;
            rd.index(MAX_DERIVS)?;
            rd.flag()?;
            rd.flag()?;
        }
        ObjOp::EvalPoint | ObjOp::EvalUv => {
            param(rd, c)?;
        }
        ObjOp::EvalNormal => {
            param(rd, c)?;
            rd.flag()?;
        }
        ObjOp::Eval1st => {
            param(rd, c)?;
            rd.index(4)?;
            rd.flag()?;
        }
        ObjOp::Knots => {
            let has = rd.flag()?;
            if has {
                // the UV curve's knots in the interval must not be empty (an end of
                // the unedited curve is a knot)
                let (x, y) = inner_interval(rd, c)?;
                if c.edited || !(x <= c.a || y >= c.b) {
                    return Err(bad("knots: the interval may hold no UV knot"));
                }
            } else {
                rd.finite()?;
                rd.finite()?;
            }
            rd.flag()?;
        }
        ObjOp::Conts => {
            let a = rd.finite()?;
            if !(0.0..=90.0).contains(&a) {
                return Err(bad("angle out of range"));
            }
        }
        ObjOp::Equal => {
            if !rd.flag()? {
                object_block(rd)?;
            }
        }
        ObjOp::EditParam => {
            c.edited = true;
            let x = rd.finite()?;
            let y = rd.finite()?;
            if (y - x).abs() < 1e-3 || x.abs() > 1e3 || y.abs() > 1e3 {
                return Err(bad("degenerate new parameterisation"));
            }
            c.a = x.min(y);
            c.b = x.max(y);
        }
        ObjOp::Reverse => {
            c.edited = true;
            inner_interval(rd, c)?;
        }
        ObjOp::Trim => {
            c.edited = true;
            let (x, y) = inner_interval(rd, c)?;
            let mode = rd.index(2)?;
            if c.owner_flag != 0 && mode != 2 {
                c.a = x;
                c.b = y;
            }
        }
        ObjOp::Transform => {
            placement(rd)?;
            rd.flag()?;
            let s = rd.vec3()?;
            if s.iter().any(|&v| !(0.1..=10.0).contains(&v)) {
                return Err(bad("transform: scale out of range"));
            }
        }
        ObjOp::Approx => {
            breaks(rd, c)?;
            tolerance(rd)?;
            rd.flag()?;
        }
        ObjOp::GetOrCreate => {
            tolerance(rd)?;
            rd.flag()?;
            breaks(rd, c)?;
            rd.flag()?;
            rd.flag()?;
        }
        ObjOp::Bbox => {
            inner_interval(rd, c)?;
        }
        ObjOp::Drop => {
            inner_interval(rd, c)?;
            rd.vec3()?;
            rd.flag()?;
            rd.vec3()?;
            let tol = rd.finite()?;
            if !(0.0..=1.0).contains(&tol) {
                return Err(bad("drop: tolerance out of range"));
            }
            rd.flag()?;
            param(rd, c)?;
            let o = rd.index(24)?;
            if ![0, 3, 4, 5, 6].contains(&o) {
                return Err(bad("drop: operation"));
            }
        }
        ObjOp::Gap => {
            if !rd.flag()? {
                let o = object_block(rd)?;
                if !o.inside {
                    return Err(bad("gap: other outside its domain"));
                }
            }
            inner_interval(rd, c)?;
            rd.finite()?;
            rd.finite()?;
            if rd.index(8)? == 1 {
                return Err(bad("gap: one sample divides by zero"));
            }
            let tol = rd.finite()?;
            if !(1e-9..=1.0).contains(&tol) {
                return Err(bad("gap: tolerance out of range"));
            }
            rd.flag()?;
            rd.finite()?;
        }
    }
    Ok(())
}

/// Whether ASCII database `bytes` hold a complete record (the writer's last line).
fn complete_ascii(bytes: &[u8]) -> bool {
    bytes.ends_with(b"\n")
}

/// Checks `input` for `op` (layouts in the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input [`eval`] rejects.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    match op {
        Op::Object => {
            let mut c = object_block(&mut rd)?;
            let nops = rd.index(16)?;
            for _ in 0..nops {
                let code = rd.num()?;
                if code.fract() != 0.0 {
                    return Err(bad("op code"));
                }
                let o = ObjOp::from_code(code as i64).ok_or_else(|| bad("unknown op"))?;
                validate_op(o, &mut rd, &mut c)?;
            }
        }
        Op::ReadDb => {
            let ft = rd.index(1)?;
            let _version = rd.index(1 << 20)?;
            let nb = rd.index(MAX_DB_BYTES)?;
            let mut bytes = Vec::with_capacity(nb);
            for _ in 0..nb {
                bytes.push(rd.index(255)? as u8);
            }
            if ft == 0 && !complete_ascii(&bytes) {
                return Err(bad("read: incomplete ASCII record"));
            }
        }
        Op::WriteDb => {
            rd.index(1)?;
            rd.index(1 << 20)?;
            object_block(&mut rd)?;
        }
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
        // reads for `op` (validated above: well-formed curve and surface blocks, every
        // index, count, flag and operation code the shim passes to upstream, UV curves
        // inside the surface domain for the ops that classify them, complete ASCII
        // records); buf is valid for buf.len() writes and the shim honours the capacity;
        // len is valid.
        unsafe {
            sys::sc_curve_crv_on_surf_eval(
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
