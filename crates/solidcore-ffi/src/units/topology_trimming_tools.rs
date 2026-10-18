//! FFI wrappers for unit `topology.trimming_tools`.
//!
//! Shim: `harness/cpp/shim/topology_trimming_tools.cpp`
//! (`sc_topology_trimming_tools_eval`). Upstream: `source/SMLib/src/SmTrimmingTools.cpp`.
//!
//! A curve block is `kind dim ...`: `0 dim oob n p m cpts(x y z w) knots`
//! (`SmBSplineCurve`), `2 3 P[3] V[3] d0 d1 scale inside_out` (bounded `SmLine`), `3 dim
//! C[3] X[3] Y[3] d0 d1 rx ry inside_out` (`SmEllipse`, domain in degrees). A surface
//! block is that of [`super::topology_vertexuse`] (`0 oob form n m p q r s cpts u v`, or
//! `1 O[3] N[3]` for an `SmPlane`, or `2 basis implicit` for an `SmCurveBoundedSurface`
//! without boundary curves over a basis block). Outputs start with the op's status (`0` success,
//! else the `SmStatus` value) unless noted; `-2` is "upstream would dereference null or
//! read out of bounds" (not called), `-3` "not run" (a path the Rust side defers).
//!
//! | [`Op`]        | inputs | outputs |
//! |---------------|--------|---------|
//! | `Order`       | `k curve*k tol outer` | `status [n idx*n nl counts*nl orients*n pt[3] normal[3] box_min[3] box_max[3] [outer block]]` (arrays for status 0 or `SM_ERR`; the outer block, `FindOuterLoops` on the result, for status 0 and `outer`) |
//! | `Outer`       | `k curve*k nl counts*nl orients*k normal[3] tol` | `status (-2 \| n idx*n nl counts*nl orients*n normal[3] nouter outer*nouter)` |
//! | `Mirror`      | `curve side low high tol` | `status dim nurb` |
//! | `IsoAt`       | `curve side low tol` | `flag` |
//! | `Iso`         | `curve` | `flag [side param]` |
//! | `IsoSeam`     | `surface curve` | `flag` |
//! | `Continuity`  | `surface` | `status [g1]` |
//! | `UvGap`       | `surface u0 v0 u1 v1 tol given p0[3] p1[3]` | `status0 status1 [p0[3] p1[3] flag]` (the surface points evaluated unless `given`) |
//! | `Min`         | `a b` | `min` |
//! | `Loops`       | `ns curve*ns nl counts*nl nk key*nk orient*nk mode has_srf [surface] nops op*` | per op `code` and its results, then the final state |
//! | `TrimNull`    | `surface k curve*k` | `status` |
//! | `BPresent` .. | B-rep block, then the op's selectors | `0` (or `-1 status`) then the op's results |
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: well-formed
//! clamped B-splines, lines with a non-zero vector, ellipses with non-zero radii and
//! independent axes, planes with a non-zero normal, finite values, indices inside the
//! B-rep's export and the curve store, and curve loops whose counts add up to their
//! curves (every script op keeps that).

use std::path::Path;

use super::topology_vertexuse as fixture;
use crate::validate::{bad, check_knots, NOW, NOZ};
use crate::{call_with_buffer, FfiError};

/// Largest control point index.
pub const MAX_INDEX: usize = 32;
/// Largest degree.
pub const MAX_DEGREE: usize = 5;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1.0e300;
/// Largest number of curves in a geometry op.
pub const MAX_CURVES: usize = 64;
/// Largest number of script ops.
pub const MAX_OPS: usize = 64;
/// Largest loop index argument of a script op.
pub const MAX_LOOP_INDEX: usize = 1 << 40;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Order = 1,
    Outer = 2,
    Mirror = 3,
    IsoAt = 4,
    Iso = 5,
    IsoSeam = 6,
    Continuity = 7,
    UvGap = 8,
    Min = 9,
    Loops = 10,
    TrimNull = 11,
    BPresent = 20,
    BExtremes = 21,
    BCheckLoop = 22,
    BCheckFace = 23,
    BTiny = 24,
    BEnsure = 25,
    BBackwards = 26,
    BOrient = 27,
    BOrder = 28,
    BDeferred = 29,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        self as i32
    }

    /// Whether the op starts with a B-rep block.
    #[must_use]
    pub fn is_brep(self) -> bool {
        self.code() >= 20
    }
}

mod sys {
    extern "C" {
        pub fn sc_topology_trimming_tools_eval(
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

    fn below(&mut self, count: usize) -> Result<usize, FfiError> {
        let i = self.index(usize::MAX >> 12)?;
        if i >= count {
            return Err(bad(format!("index {i} out of range ({count})")));
        }
        Ok(i)
    }

    fn flag(&mut self) -> Result<bool, FfiError> {
        Ok(self.index(1)? == 1)
    }

    fn side(&mut self) -> Result<(), FfiError> {
        self.index(1).map(|_| ())
    }

    fn done(&self) -> Result<(), FfiError> {
        if self.at != self.input.len() {
            return Err(bad("trailing input"));
        }
        Ok(())
    }
}

/// Checks `k` control points sharing their rational marker (`z = NOZ` for 2D data when
/// `planar`).
fn check_points(rd: &mut Reader<'_>, k: usize, planar: bool, what: &str) -> Result<(), FfiError> {
    let mut w0 = None;
    for _ in 0..k {
        let x = rd.num()?;
        let y = rd.num()?;
        let z = rd.num()?;
        let w = rd.num()?;
        if (z == NOZ) != planar {
            return Err(bad(format!("{what}: 2D/3D data mismatch")));
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

/// A surface block (the [`super::topology_vertexuse`] layout, or `2 basis implicit` for
/// an `SmCurveBoundedSurface` without boundary curves over a basis block).
fn surface(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    surface_of(rd, true)
}

fn surface_of(rd: &mut Reader<'_>, bounded_ok: bool) -> Result<(), FfiError> {
    let kind = rd.index(if bounded_ok { 2 } else { 1 })?;
    if kind == 2 {
        surface_of(rd, false)?;
        rd.flag()?;
        return Ok(());
    }
    if kind == 1 {
        rd.vec3()?;
        rd.nonzero3()?;
        return Ok(());
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
    check_knots(&u, p, "surface u")?;
    check_knots(&v, q, "surface v")?;
    Ok(())
}

/// A curve block. The UV curve ops take every kind the shim builds: `SmLine` and
/// `SmEllipse` are `SmBSplineCurve`s, and the `SmBSplineCurve` methods they reach
/// (`GetCanonical`, `GetEnds`, `ConvertTo2D`) make a missing NURB first.
fn curve(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let kind = rd.index(3)?;
    let dim = rd.index(3)?;
    match kind {
        0 => {
            if dim < 2 {
                return Err(bad("curve: dimension must be 2 or 3"));
            }
            rd.flag()?;
            let n = rd.index(MAX_INDEX)?;
            let p = rd.index(MAX_DEGREE)?;
            let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
            if p == 0 || p > n || m != n + p + 1 {
                return Err(bad("curve: need 1 <= p <= n and m = n + p + 1"));
            }
            check_points(rd, n + 1, dim == 2, "curve")?;
            let knots = rd.finites(m + 1)?;
            check_knots(&knots, p, "curve")?;
        }
        2 => {
            if dim != 3 {
                return Err(bad("line: dimension 3"));
            }
            rd.vec3()?;
            rd.nonzero3()?;
            let d0 = rd.finite()?;
            let d1 = rd.finite()?;
            let scale = rd.finite()?;
            rd.flag()?;
            if d0 >= d1 || !(1e-6..=1e6).contains(&scale) {
                return Err(bad("line: bad domain or scale"));
            }
        }
        3 => {
            if dim != 3 {
                return Err(bad("ellipse: dimension 3"));
            }
            rd.vec3()?;
            let x = rd.nonzero3()?;
            let y = rd.nonzero3()?;
            let d0 = rd.finite()?;
            let d1 = rd.finite()?;
            let rx = rd.finite()?;
            let ry = rd.finite()?;
            rd.flag()?;
            let cross = [
                x[1] * y[2] - x[2] * y[1],
                x[2] * y[0] - x[0] * y[2],
                x[0] * y[1] - x[1] * y[0],
            ];
            let nx: f64 = x.iter().map(|c| c * c).sum();
            let ny: f64 = y.iter().map(|c| c * c).sum();
            let nc: f64 = cross.iter().map(|c| c * c).sum();
            if nc < 1e-6 * nx * ny || d0 >= d1 || d1 - d0 > 720.0 || rx <= 1e-9 || ry <= 1e-9 {
                return Err(bad("ellipse: bad axes, domain or radii"));
            }
        }
        _ => return Err(bad("curve kind")),
    }
    Ok(())
}

fn orient(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    rd.index(2).map(|_| ())
}

/// The curve-loops script (see the runner for the op layouts).
fn loops(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let ns = rd.index(MAX_CURVES)?;
    if ns == 0 {
        return Err(bad("loops: empty curve store"));
    }
    for _ in 0..ns {
        curve(rd)?;
    }
    let nl = rd.index(MAX_CURVES)?;
    let mut sum = 0;
    for _ in 0..nl {
        sum += rd.index(MAX_CURVES)?;
    }
    let nk = rd.index(MAX_CURVES)?;
    for _ in 0..nk {
        rd.below(ns)?;
    }
    for _ in 0..nk {
        orient(rd)?;
    }
    let mode = rd.index(2)?;
    if sum != nk || (mode == 2 && (nk != 0 || nl != 0)) {
        return Err(bad("loops: counts must add up to the curves"));
    }
    let has_srf = rd.flag()?;
    if has_srf {
        surface(rd)?;
    }
    let nops = rd.index(MAX_OPS)?;
    for _ in 0..nops {
        match rd.index(12)? {
            1 | 2 | 7 => {
                rd.index(MAX_LOOP_INDEX)?;
            }
            3 | 5 => {
                rd.below(ns)?;
                rd.finite()?;
            }
            4 => {
                rd.index(MAX_LOOP_INDEX)?;
                rd.index(MAX_LOOP_INDEX)?;
            }
            6 => {
                rd.below(ns)?;
                rd.finite()?;
                rd.side()?;
                rd.finites(2)?;
                let d = rd.finites(4)?;
                if d.first() > d.get(2) || d.get(1) > d.get(3) {
                    return Err(bad("loops: domain not ordered"));
                }
            }
            8 => {
                rd.below(ns)?;
                rd.below(ns)?;
            }
            9 => {
                rd.below(ns)?;
                rd.finites(2)?;
                for _ in 0..4 {
                    rd.index(MAX_LOOP_INDEX)?;
                }
            }
            10 => {
                let nint = rd.index(8)?;
                for _ in 0..nint {
                    let iso = rd.finite()?;
                    if iso != -1.0 && (iso.fract() != 0.0 || iso < 0.0 || iso >= ns as f64) {
                        return Err(bad("loops: bad iso curve key"));
                    }
                    rd.side()?;
                    rd.finite()?;
                    let nsol = rd.index(8)?;
                    for _ in 0..nsol {
                        let obj = rd.finite()?;
                        let ok = obj == -1.0
                            || (obj == -2.0 && has_srf)
                            || (obj.fract() == 0.0 && obj >= 0.0 && obj < ns as f64);
                        if !ok {
                            return Err(bad("loops: bad solution object"));
                        }
                        rd.finite()?;
                    }
                }
                rd.below(ns)?;
                rd.below(ns)?;
                rd.finite()?;
            }
            11 => {}
            12 => {
                for _ in 0..4 {
                    rd.index(MAX_LOOP_INDEX)?;
                }
            }
            _ => return Err(bad("loops: bad op")),
        }
    }
    Ok(())
}

fn geometry_args(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::Order => {
            let k = rd.index(MAX_CURVES)?;
            for _ in 0..k {
                curve(rd)?;
            }
            rd.finite()?;
            rd.flag()?;
        }
        Op::Outer => {
            let k = rd.index(MAX_CURVES)?;
            for _ in 0..k {
                curve(rd)?;
            }
            let nl = rd.index(MAX_CURVES)?;
            let mut sum = 0;
            for _ in 0..nl {
                sum += rd.index(MAX_CURVES)?;
            }
            if sum != k {
                return Err(bad("outer: loop counts must add up to the curves"));
            }
            for _ in 0..k {
                orient(rd)?;
            }
            rd.vec3()?;
            rd.finite()?;
        }
        Op::Mirror => {
            curve(rd)?;
            rd.side()?;
            rd.finites(3)?;
        }
        Op::IsoAt => {
            curve(rd)?;
            rd.side()?;
            rd.finites(2)?;
        }
        Op::Iso => curve(rd)?,
        Op::IsoSeam => {
            surface(rd)?;
            curve(rd)?;
        }
        Op::Continuity => surface(rd)?,
        Op::UvGap => {
            surface(rd)?;
            rd.finites(5)?;
            rd.flag()?;
            rd.finites(6)?;
        }
        Op::Min => {
            rd.num()?;
            rd.num()?;
        }
        Op::Loops => loops(rd)?,
        Op::TrimNull => {
            surface(rd)?;
            let k = rd.index(MAX_CURVES)?;
            for _ in 0..k {
                curve(rd)?;
            }
        }
        _ => return Err(bad("not a geometry op")),
    }
    Ok(())
}

/// The mirror pre-step `k side` (`k = -1`: none; else any edge use position, taken
/// modulo the loop's count).
fn mirror(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let k = rd.num()?;
    if k != -1.0 && (k.fract() != 0.0 || !(0.0..=1.0e9).contains(&k)) {
        return Err(bad("bad mirror selector"));
    }
    rd.side()
}

/// Checks the arguments after the B-rep block against the B-rep's counts (regions,
/// shells, face uses, faces, loop uses, loops, edge uses, edges, vertex uses, vertices).
fn brep_args(op: Op, rd: &mut Reader<'_>, c: [usize; 10]) -> Result<(), FfiError> {
    let [_, _, _, n_f, _, n_l, n_eu, _, _, _] = c;
    match op {
        Op::BPresent => {}
        Op::BExtremes => {
            rd.below(n_eu)?;
        }
        Op::BCheckLoop => {
            rd.below(n_l)?;
            rd.finite()?;
            rd.flag()?;
            mirror(rd)?;
        }
        Op::BCheckFace => {
            rd.below(n_f)?;
            rd.finite()?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::BTiny => {
            let of_face = rd.flag()?;
            rd.below(if of_face { n_f } else { n_l })?;
            rd.finite()?;
            mirror(rd)?;
        }
        Op::BEnsure => {
            rd.below(n_l)?;
            mirror(rd)?;
        }
        Op::BBackwards => {
            rd.below(n_f)?;
            mirror(rd)?;
        }
        Op::BOrient | Op::BOrder => {
            rd.below(n_f)?;
        }
        Op::BDeferred => {
            rd.index(2)?;
            rd.below(n_f)?;
        }
        _ => return Err(bad("not a B-rep op")),
    }
    Ok(())
}

fn raw(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let cpath = std::ffi::CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: cpath is a valid NUL-terminated string; input is valid for
        // input.len() reads and holds what the shim reads for `op` (validated by
        // `eval`); buf is valid for buf.len() writes and the shim honours the capacity;
        // len is valid.
        unsafe {
            sys::sc_topology_trimming_tools_eval(
                op.code(),
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

/// The length of the B-rep block at the start of `input` (the
/// [`super::topology_vertexuse`] layout).
fn brep_block_len(input: &[f64], path: &str) -> Result<usize, FfiError> {
    let mut rd = Reader { input, at: 0 };
    match rd.index(3)? {
        0 => {
            let p = Path::new(path);
            if p.extension().and_then(|e| e.to_str()) != Some("smb") || !p.is_file() {
                return Err(bad("B-rep file: need an existing .smb file"));
            }
            rd.flag()?;
        }
        1 => {
            // a wire edge: `dim n p m cpts knots t0 t1`
            rd.index(3)?;
            let n = rd.index(MAX_INDEX)?;
            rd.index(MAX_DEGREE)?;
            let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
            for _ in 0..(4 * (n + 1) + m + 1 + 2) {
                rd.num()?;
            }
        }
        2 => {
            surface(&mut rd)?;
            rd.finites(4)?;
        }
        _ => {}
    }
    rd.flag()?;
    rd.flag()?;
    Ok(rd.at)
}

/// Runs `op` on `input` (see the module docs); `path` is the `.smb` file of a file
/// B-rep block (ignored otherwise).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed input; [`FfiError::Status`] for a shim
/// failure.
pub fn eval(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    if !op.is_brep() {
        let mut rd = Reader { input, at: 0 };
        geometry_args(op, &mut rd)?;
        rd.done()?;
        return raw(op, path, input);
    }
    let n = brep_block_len(input, path)?;
    let block = input.get(..n).ok_or_else(|| bad("input too short"))?;
    // the topology.vertexuse wrapper validates the block itself and exports the graph
    let export = fixture::eval(fixture::Op::Export, path, block)?;
    let Some(c) = fixture::counts(&export) else {
        // the B-rep could not be built: the op reports the same failure (`-1 status`)
        return Ok(export);
    };
    let mut rd = Reader { input, at: n };
    brep_args(op, &mut rd, c)?;
    rd.done()?;
    raw(op, path, input)
}
