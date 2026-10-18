//! FFI wrappers for unit `heal.gap`.
//!
//! Shim: `harness/cpp/shim/heal_gap.cpp` (`sc_heal_gap_eval`). Upstream:
//! `source/SMLib/src/SmGap.cpp`, `source/SMLib/inc/SmGap.h`.
//!
//! Geometry blocks:
//!
//! | block    | layout |
//! |----------|--------|
//! | curve    | `0 dim oob n p m cpts(x y z w) knots` (`SmBSplineCurve`); `1 dim P1 D1 P2 D2` (`SmHermiteCurve`); `2 dim P V d0 d1 scale io` (bounded `SmLine`, always 3D); `3 dim C X Y d0 d1 rx ry io` (`SmEllipse`); `4 dim C X Y d0 d1 r io` (`SmCircle`); `5 dim n p m cpts knots surface-bspline` (`SmCrvOnSurf` over a 2D B-spline and a B-spline surface) |
//! | surface  | `0 bspline` (`SmBSplineSurface`), `1 O X Y sx sy u0 v0 u1 v1` (`SmPlane`), `2 bspline` (`SmCurveBoundedSurface` over it) with `bspline = oob form n m p q r s cpts u v` |
//! | function | `kind tol this ivl other cnt_u cnt_v`: `kind` 0 crv/pt, 1 crv/crv, 2 crv/srf, 3 srf/pt, 4 srf/crv, 5 srf/srf; `this` a curve (`ivl = t0 t1`) or surface (`ivl = u0 v0 u1 v1`); `other` a point `x y z`, a curve or a surface; `tol` is passed to `SmXSectTol3d(double)` |
//!
//! | [`Op`]      | input | output |
//! |-------------|-------|--------|
//! | `Gf`        | `function set_cnt su sv set_tol tol stop mode` | `status cnt_u cnt_v state max min all_normal oivl_u(2) oivl_v(2) n sample*n` (`mode` 0 `GetMaxGapSample`, 1 `GetSampleSet`) |
//! | `Eval`      | `function u v has_gu gu has_gv gv` | `status sample` |
//! | `AddUnique` | `n a* value` | `index n' a'*` |
//! | `Sample`    | `this(3) tu tv other(3) ou ov drop tol` | `sample length this_param(0) this_param(1) other_param(0) other_param(1)` |
//! | `GapArray`  | `na len* nb len*` | per array `size max_index max_length`, then `b`'s lengths with `>` / `<` of neighbours, then the size after `ReSet` |
//! | `Topo`      | B-rep block (as [`super::topology_vertexuse`]) `kind sel0 sel1 t0 t1 u v has_uv` | `0 gap is_init edge_t0 edge_t1 uvt face_uv(2) vertex vertexuse edge0 edge1 edgeuse0 edgeuse1 face type end_status p1(3) p2(3) cached_gap`, `-2` (upstream dereferences null) or `-1 status` (no B-rep) |
//!
//! A sample is `drop this(3) other(3) tol_point(3) this_param(2) other_param(2) xtol
//! in_tol`. Topology indices are the export's traversal order (`-1` null).
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: well-formed
//! clamped B-splines, non-degenerate analytic curves and planes, sample counts in
//! `1..=MAX_COUNT` (upstream indexes before its arrays for `0`), finite values,
//! topology selectors inside the B-rep's export.

use crate::validate::{bad, check_knots, NOW, NOZ};
use crate::{call_with_buffer, FfiError};

use super::topology_vertexuse as vu;

/// Largest control point index.
pub const MAX_INDEX: usize = 24;
/// Largest degree.
pub const MAX_DEGREE: usize = 5;
/// Largest sample count per direction.
pub const MAX_COUNT: usize = 200;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1.0e12;
/// Largest array length of `AddUnique` and `GapArray`.
pub const MAX_ARRAY: usize = 1000;

/// `SM_UNDEF_DOUBLE`.
pub const UNDEF: f64 = 1.23456e-61;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Gf = 0,
    Eval = 1,
    AddUnique = 2,
    Sample = 3,
    GapArray = 4,
    Topo = 5,
}

mod sys {
    extern "C" {
        pub fn sc_heal_gap_eval(
            op: i32,
            path: *const std::ffi::c_char,
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

    /// A finite value or `SM_UNDEF_DOUBLE` (any finite value qualifies).
    fn param(&mut self) -> Result<f64, FfiError> {
        self.finite()
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
}

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

/// `n p m cpts knots` of a B-spline curve of dimension `dim`.
fn nurb_curve(rd: &mut Reader<'_>, dim: usize) -> Result<(), FfiError> {
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad("curve: need 1 <= p <= n and m = n + p + 1"));
    }
    check_points(rd, n + 1, dim == 2, "curve")?;
    let knots = (0..=m)
        .map(|_| rd.finite())
        .collect::<Result<Vec<_>, _>>()?;
    check_knots(&knots, p, "curve")?;
    Ok(())
}

/// `oob form n m p q r s cpts u v`.
fn bspline_surface(rd: &mut Reader<'_>) -> Result<(), FfiError> {
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
    let u = (0..=r)
        .map(|_| rd.finite())
        .collect::<Result<Vec<_>, _>>()?;
    let v = (0..=s)
        .map(|_| rd.finite())
        .collect::<Result<Vec<_>, _>>()?;
    check_knots(&u, p, "surface u")?;
    check_knots(&v, q, "surface v")?;
    Ok(())
}

fn surface(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match rd.index(2)? {
        1 => {
            rd.vec3()?;
            let x = rd.nonzero3()?;
            let y = rd.nonzero3()?;
            let c = [
                x[1] * y[2] - x[2] * y[1],
                x[2] * y[0] - x[0] * y[2],
                x[0] * y[1] - x[1] * y[0],
            ];
            let nx = x.iter().map(|a| a * a).sum::<f64>().sqrt();
            let ny = y.iter().map(|a| a * a).sum::<f64>().sqrt();
            if c.iter().map(|a| a * a).sum::<f64>().sqrt() < 1e-6 * nx * ny {
                return Err(bad("plane: parallel axes"));
            }
            for _ in 0..2 {
                if rd.finite()?.abs() < 1e-6 {
                    return Err(bad("plane: zero scale"));
                }
            }
            let d = [rd.finite()?, rd.finite()?, rd.finite()?, rd.finite()?];
            if d[0] > d[2] || d[1] > d[3] {
                return Err(bad("plane: domain not ordered"));
            }
            Ok(())
        }
        _ => bspline_surface(rd),
    }
}

fn curve(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let kind = rd.index(5)?;
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("curve: dimension must be 2 or 3"));
    }
    match kind {
        0 => {
            rd.flag()?;
            nurb_curve(rd, dim)
        }
        1 => {
            for _ in 0..4 {
                rd.vec3()?;
            }
            Ok(())
        }
        2 => {
            if dim != 3 {
                return Err(bad("line: 3D only"));
            }
            rd.vec3()?;
            rd.nonzero3()?;
            let d0 = rd.finite()?;
            let d1 = rd.finite()?;
            if d0 >= d1 {
                return Err(bad("line: domain not increasing"));
            }
            if rd.finite()?.abs() < 1e-6 {
                return Err(bad("line: zero scale"));
            }
            rd.flag()?;
            Ok(())
        }
        3 | 4 => {
            rd.vec3()?;
            let x = rd.nonzero3()?;
            let y = rd.nonzero3()?;
            let dot: f64 = x.iter().zip(&y).map(|(a, b)| a * b).sum();
            let nx = x.iter().map(|a| a * a).sum::<f64>().sqrt();
            let ny = y.iter().map(|a| a * a).sum::<f64>().sqrt();
            if dot.abs() > 1e-6 * nx * ny {
                return Err(bad("ellipse: axes not perpendicular"));
            }
            let d0 = rd.finite()?;
            let d1 = rd.finite()?;
            if d0 >= d1 || d1 - d0 > 4.0 * std::f64::consts::PI {
                return Err(bad("ellipse: bad domain"));
            }
            let radii = if kind == 3 { 2 } else { 1 };
            for _ in 0..radii {
                if rd.finite()? <= 1e-9 {
                    return Err(bad("ellipse: radius not positive"));
                }
            }
            rd.flag()?;
            Ok(())
        }
        _ => {
            nurb_curve(rd, 2)?;
            bspline_surface(rd)
        }
    }
}

/// A gap function block; returns its kind.
fn function(rd: &mut Reader<'_>) -> Result<usize, FfiError> {
    let kind = rd.index(5)?;
    rd.finite()?;
    if kind < 3 {
        curve(rd)?;
        rd.finite()?;
        rd.finite()?;
    } else {
        surface(rd)?;
        for _ in 0..4 {
            rd.finite()?;
        }
    }
    match kind {
        0 | 3 => {
            rd.vec3()?;
        }
        1 | 4 => curve(rd)?,
        _ => surface(rd)?,
    }
    let cu = rd.index(MAX_COUNT)?;
    let cv = rd.index(MAX_COUNT)?;
    if cu == 0 || cv == 0 {
        return Err(bad("zero sample count"));
    }
    Ok(kind)
}

/// A point that may hold `SM_UNDEF_DOUBLE` or `NL_NOZ`.
fn raw_point(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    for _ in 0..3 {
        let v = rd.num()?;
        if v != NOZ && (!v.is_finite() || v.abs() > MAX_COORD) {
            return Err(bad("bad point coordinate"));
        }
    }
    Ok(())
}

fn check(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    match op {
        Op::Gf => {
            function(&mut rd)?;
            let set_cnt = rd.flag()?;
            let su = rd.index(MAX_COUNT)?;
            let sv = rd.index(MAX_COUNT)?;
            if set_cnt && (su == 0 || sv == 0) {
                return Err(bad("zero sample count"));
            }
            rd.flag()?;
            rd.finite()?;
            rd.flag()?;
            rd.index(1)?;
        }
        Op::Eval => {
            function(&mut rd)?;
            rd.finite()?;
            rd.finite()?;
            rd.flag()?;
            rd.finite()?;
            rd.flag()?;
            rd.finite()?;
        }
        Op::AddUnique => {
            let n = rd.index(MAX_ARRAY)?;
            for _ in 0..n {
                rd.finite()?;
            }
            let v = rd.num()?;
            if v.is_infinite() || v.abs() > MAX_COORD {
                return Err(bad("bad value"));
            }
        }
        Op::Sample => {
            raw_point(&mut rd)?;
            rd.finite()?;
            rd.finite()?;
            raw_point(&mut rd)?;
            rd.finite()?;
            rd.finite()?;
            rd.index(3)?;
            rd.finite()?;
        }
        Op::GapArray => {
            for _ in 0..2 {
                let n = rd.index(MAX_ARRAY)?;
                for _ in 0..n {
                    rd.finite()?;
                }
            }
        }
        Op::Topo => return Err(bad("use topo()")),
    }
    rd.done()
}

fn raw(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let cpath = std::ffi::CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: cpath is a valid NUL-terminated string; input is valid for
        // input.len() reads and holds what the shim reads for `op` (validated by the
        // callers); buf is valid for buf.len() writes and the shim honours the
        // capacity; len is valid.
        unsafe {
            sys::sc_heal_gap_eval(
                op as i32,
                cpath.as_ptr(),
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Runs a geometry `op` (every op but [`Op::Topo`]) on `input` (see the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed input; [`FfiError::Status`] for a shim
/// failure.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    check(op, input)?;
    raw(op, "", input)
}

/// The export of `block` (a B-rep block as [`super::topology_vertexuse`] reads it):
/// the graph the topology op runs on.
///
/// # Errors
///
/// As [`super::topology_vertexuse::eval`].
pub fn export(path: &str, block: &[f64]) -> Result<Vec<f64>, FfiError> {
    vu::eval(vu::Op::Export, path, block)
}

/// Runs [`Op::Topo`] on the B-rep of `block` with `args = kind sel0 sel1 t0 t1 u v
/// has_uv` (selectors index the vertex uses for kinds `0..=2`, else the edge uses; `-1`
/// is null).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed input; [`FfiError::Status`] for a shim
/// failure.
pub fn topo(path: &str, block: &[f64], args: &[f64]) -> Result<Vec<f64>, FfiError> {
    let ex = export(path, block)?;
    let mut rd = Reader::new(args);
    let kind = rd.index(5)?;
    let counts = vu::counts(&ex);
    let n = match counts {
        Some(c) => {
            if kind <= 2 {
                c[8]
            } else {
                c[6]
            }
        }
        None => 0,
    };
    for _ in 0..2 {
        let s = rd.num()?;
        if s.fract() != 0.0 || s < -1.0 || (s >= 0.0 && s as usize >= n.max(1)) {
            return Err(bad("bad selector"));
        }
    }
    rd.param()?;
    rd.param()?;
    rd.param()?;
    rd.param()?;
    rd.flag()?;
    rd.done()?;
    let mut input = block.to_vec();
    input.extend_from_slice(args);
    raw(Op::Topo, path, &input)
}
