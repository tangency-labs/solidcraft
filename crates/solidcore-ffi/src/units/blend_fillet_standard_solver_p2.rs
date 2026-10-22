//! FFI wrappers for unit `blend.fillet_standard_solver_p2`.
//!
//! Shim: `harness/cpp/shim/blend_fillet_standard_solver_p2.cpp`
//! (`sc_blend_fillet_standard_solver_p2_eval`).
//! Upstream: `source/SMLib/src/SmFilletStandardSolver.cpp`.
//!
//! Encodings. A surface is `kind`: `0 n m p q r s`, `(n+1)(m+1)` control points `x y z w`
//! (v index fastest), `r+1` u knots, `s+1` v knots (`SmBSplineSurface(gw_SURFACE*, FALSE,
//! NULL)`), or `1 o[3] x[3] y[3] sx sy u0 v0 u1 v1` (`SmPlane(o, x, y, (sx, sy), domain,
//! ctx)`). A solver block is `cls same surfaceA [surfaceB] r0 r1 o0 o1 ext`: `cls` `0`
//! `SmConstantRadiusFS(ctx, 1e-6, 30deg, 2deg, r0, r1, A, B or A, o0, o1)`, `1`
//! `SmSurfaceSurfaceFS` (the same, UV guesses `(0.5, 0.5)`) or `2` an
//! `SmConstantRadiusAssistedFS` built on an `SmApiCreateBox` edge use whose rail `i`
//! offsets its base by `r_i` times `+-1` (`o_i`), with radii `r_i` and no edge uses; `ext`
//! is the extension factor. A surface selector is `1` `GetSurface(0)`, `2`
//! `GetSurface(1)`, `3` another offset surface of A, `4` an offset surface of a copy of A.
//! `kinds` is `nk` and `nk` type codes passed to `IsKindOf`.
//!
//! | [`Op`]   | upstream                                                  | parameters |
//! |----------|-----------------------------------------------------------|------------|
//! | `CrInit` | `SmConstantRadiusFS::LoadInitialValues`                   | `solver do2 rail sel1 s1_index dim` `dim` x `min max`, `np per[np] ng guess[ng]` |
//! | `Guess`  | `SmConstantRadiusAssistedFS::GuessPointOnPlaneSolve`      | `solver(cls 2) orig[3] normal[3] d1[4] d2[4] uv1[2] uv2[2]` |
//! | `Plane`  | `SmConstantRadiusAssistedFS::PointOnPlaneSolve`           | as `Guess` |
//! | `Ss`     | `SmSurfaceSurfaceFS` constructor (and `CalcFilletGeom`)   | `surfaceA surfaceB r0 r1 o0 o1 g0[2] g1[2] calc kinds` |
//! | `Types`  | `IsKindOf` of the assisted, surface/surface and variable-radius solvers | `kinds` |
//!
//! Output: the op status (`0` = `SM_SUCCESS`, else the `SmStatus`) and then: `CrInit` the
//! rail of `rpSrf2Offset` (`-1` `NULL`), `rlSurface2Index`, the intervals, `np' per[np']`,
//! `ng' guess[ng']` and the offset state; `Guess` on success `found` and with
//! `found` both UV points, then the offset state; `Plane` as `Guess`, after the UV points
//! per surface its point, derivatives and unit normal and then the curve point; `Ss` the
//! radii, orientations, offset state, UV guesses, trim type, solver type, `GetType`
//! and the kinds (its status is that of `CalcFilletGeom` with `calc`, `-3` when the
//! shim did not run it, `0` without `calc`); `Types` per solver `GetType` and the kinds.
//! The offset state is per rail `offset umin vmin umax vmax has_extension`: the offset
//! distance, `GetNaturalUVDomain()` and `GetExtendedBaseSurface() != NULL` after the op.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: well-formed
//! clamped 3D B-spline surfaces (`1 <= p <= n`, at most [`MAX_INDEX`] control points per
//! direction, degrees at most [`MAX_DEGREE`], finite data, consistent rationality,
//! positive weights, non-decreasing knots with end multiplicity exactly `degree + 1` and
//! interior multiplicity at most the degree), planes with a non-reversed domain (an
//! offset surface's extension of a reversed one reads a half-built NLib surface, UB-187),
//! finite numbers, arrays of at most [`MAX_ARRAY`] entries, parameters within
//! [`MAX_PARAM`], a non-`NULL` first surface of `CrInit` and, with `do2`, the guess and
//! interval indices it reads and writes in range and rail 0 or 1; `Guess` and `Plane` only
//! on the assisted solver (`cls 2`). `CalcFilletGeom` past its parallel-normal exit would
//! step along the fillet and dereference the solver's `NULL` executive: the shim runs it
//! only when the offset normals at the guesses are parallel (its first exit).
//!
//! [`recalc`] (shim `harness/cpp/shim/blend_fillet_standard_solver_p2_static.inc`, included by
//! `blend_fillet_geom_p1.cpp`; `sc_blend_fillet_standard_solver_p2_recalc`) runs
//! `SmVariableRadiusFS::ReCalcFilletGeom` on a pseudo B-rep block of unit
//! `blend.fillet_geom_p1` ([`super::blend_fillet_geom_p1`]) followed by `g` (the geometry
//! put into `m_vFilletGeoms`, `-1` none), `nt` intersection points `e which crv_pos[3]
//! crv_deriv[3] t uv0[2] uv1[2]` set on the vertex use of edge use `which` of edge `e`, surfaces A and B,
//! the centre curve (the block's encodings), `d0 d1 o0 o1 tol ang tan r0 r1`, `law_orient`
//! and `has_ref [ref curve]`: an `SmVariableRadiusFS` over A, B, the centre curve and an
//! `SmLinearFilletLaw(r0, r1)` whose offsets are then replaced by offsets of A and B at
//! `d_i * o_i` (allowing extension) and whose first fillet geometry (the constructor's
//! default) by geometry `g`. Output `-2` without geometry (upstream reads
//! `m_vFilletGeoms[0]` out of bounds), else `status extend_before extend_after` and per
//! edge of the block and edge use `which` its vertex use's intersection point `t crv_pos[3]
//! uv0[2] uv1[2]`. [`recalc`] accepts what the block validator accepts, edge indices inside
//! the block, finite values, `tol > 0` and well-formed curves and surfaces.
//!
//! [`eval_edgeuse`] (shim `harness/cpp/shim/blend_fillet_standard_solver_p2_eu.cpp`,
//! `sc_blend_fillet_standard_solver_p2_eu_eval`) runs on a B-rep block of unit
//! `topology.edgeuse_p1` ([`super::topology_edgeuse_p1`]) followed by `mode`:
//!
//! - `mode 0`: the `SmConstantDistanceFS` constructor, `eu tol ang tan dist fu fv`: the
//!   edge use's index in the export's traversal order, the constructor's tolerances and
//!   distance and the UV fractions of each offset surface's natural domain where it is
//!   evaluated (output below).
//! - `mode 1`: `SmConstantRadiusFS::LoadInitialValues` with edge uses, `eu tol ang tan r
//!   rail fu fv ext`: the `SmConstantRadiusFS` edge-use constructor (radius `r`), its
//!   `m_dSurfaceExtensionFactor` set to `ext`, then `LoadInitialValues(1, rail,
//!   *GetSurface(rail), 0, ..)` with the guess at the UV fractions of that offset's
//!   natural domain (intervals `ExtentNd(4)`, the first two that domain). Output: `0`,
//!   or `-1 status`; `-2` as below; `1001` when the constructor signalled tangent
//!   surfaces; else `0`, the status, the rail of `rpSurface2` (`-1` other), the index
//!   (`999` untouched), on success intervals 2 and 3, `np per[np] ng guess[ng]`, and per
//!   rail `offset umin vmin umax vmax has_extension`.
//!
//! Mode 0 output: `0`, or `-1 status` when the B-rep could not be built; then `-2` when upstream
//! would dereference null (the edge use or its radial is not a loop edge use with edge,
//! curve, face use, face, surface, vertex uses and vertices), or `0 built o0 o1 distance`
//! (`built` `0` when the constructor signalled tangent surfaces and cleared its edge
//! uses) and when built per rail `eu offset face base_is_face_surface status P[3]`; then
//! the edge use's and its radial's `put_eu_state` (`EvaluateBinormal` may attach a UV
//! trim curve).

use crate::{call_with_buffer, FfiError};

/// Largest control point index accepted by [`eval`].
pub const MAX_INDEX: usize = 64;
/// Largest NURB degree accepted by [`eval`].
pub const MAX_DEGREE: usize = 9;
/// Largest array length (guesses, intervals, periodicities, type codes) accepted by
/// [`eval`].
pub const MAX_ARRAY: usize = 32;
/// Largest `|parameter|` (guesses, UV points) accepted by [`eval`].
pub const MAX_PARAM: f64 = 1.0e6;
/// `NL_NOZ`.
const NOZ: f64 = f64::MAX;
/// Largest `m_dSurfaceExtensionFactor` [`eval_edgeuse`] accepts (a generator bound: the
/// extended domains stay within [`MAX_PARAM`] of the B-reps' parameters).
pub const MAX_EXTENSION_FACTOR: f64 = 10.0;
/// `NL_NOW`.
const NOW: f64 = -f64::MAX;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `SmConstantRadiusFS::LoadInitialValues`.
    CrInit,
    /// `SmConstantRadiusAssistedFS::GuessPointOnPlaneSolve`.
    Guess,
    /// `SmConstantRadiusAssistedFS::PointOnPlaneSolve`.
    Plane,
    /// The `SmSurfaceSurfaceFS` constructor and `CalcFilletGeom`.
    Ss,
    /// `IsKindOf` of the assisted, surface/surface and variable-radius solvers.
    Types,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::CrInit => 0,
            Op::Guess => 1,
            Op::Plane => 2,
            Op::Ss => 3,
            Op::Types => 4,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_blend_fillet_standard_solver_p2_eu_eval(
            path: *const std::ffi::c_char,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_blend_fillet_standard_solver_p2_recalc(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_blend_fillet_standard_solver_p2_eval(
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

    fn param(&mut self) -> Result<f64, FfiError> {
        let v = self.finite()?;
        if v.abs() > MAX_PARAM {
            return Err(bad("parameter too large"));
        }
        Ok(v)
    }

    fn finites(&mut self, k: usize) -> Result<Vec<f64>, FfiError> {
        (0..k).map(|_| self.finite()).collect()
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

/// Checks a clamped knot vector of degree `p`.
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

/// A solver's base surface: a B-spline surface, or a plane whose domain is not reversed
/// (its NURB knots are the domain's corners, UB-187).
fn surface(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    if rd.index(1)? == 1 {
        let d = rd.finites(15)?;
        if let [.., u0, v0, u1, v1] = d[..] {
            if u0 > u1 || v0 > v1 {
                return Err(bad("plane: reversed domain (decreasing NURB knots)"));
            }
        }
        return Ok(());
    }
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
    Ok(())
}

fn kinds(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let nk = rd.index(MAX_ARRAY)?;
    for _ in 0..nk {
        rd.index(1_000_000)?;
    }
    Ok(())
}

/// The solver block; returns its class.
fn solver_block(rd: &mut Reader<'_>) -> Result<usize, FfiError> {
    let cls = rd.index(2)?;
    let same = rd.flag()?;
    surface(rd)?;
    if !same {
        surface(rd)?;
    }
    rd.param()?;
    rd.param()?;
    rd.flag()?;
    rd.flag()?;
    rd.finite()?;
    Ok(cls)
}

fn validate_cr_init(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    solver_block(rd)?;
    let do2 = rd.flag()?;
    let rail = rd.index(3)?;
    let sel1 = rd.index(4)?;
    if sel1 == 0 {
        return Err(bad("init: NULL first surface"));
    }
    let s1_index = rd.index(MAX_ARRAY)?;
    let dim = rd.index(MAX_ARRAY)?;
    rd.finites(2 * dim)?;
    let np = rd.index(MAX_ARRAY)?;
    for _ in 0..np {
        rd.flag()?;
    }
    let ng = rd.index(MAX_ARRAY)?;
    for _ in 0..ng {
        rd.param()?;
    }
    if do2 {
        if rail > 1 {
            return Err(bad("init: rail index above 1"));
        }
        // reads rGuessT[s1_index..+2], writes rIntervals[ng..+2]
        if s1_index + 1 >= ng || ng + 1 >= dim {
            return Err(bad("init: guess or interval index out of range"));
        }
    }
    Ok(())
}

fn validate_plane(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    if solver_block(rd)? != 2 {
        return Err(bad("plane solve: needs the assisted solver (cls 2)"));
    }
    rd.finites(6)?;
    rd.finites(8)?;
    for _ in 0..4 {
        rd.param()?;
    }
    Ok(())
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    let rd = &mut rd;
    match op {
        Op::CrInit => validate_cr_init(rd)?,
        Op::Guess | Op::Plane => validate_plane(rd)?,
        Op::Ss => {
            surface(rd)?;
            surface(rd)?;
            rd.param()?;
            rd.param()?;
            rd.flag()?;
            rd.flag()?;
            for _ in 0..4 {
                rd.param()?;
            }
            rd.flag()?;
            kinds(rd)?;
        }
        Op::Types => kinds(rd)?,
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
    call_with_buffer(65536, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above: well-formed clamped 3D surfaces, non-reversed
        // plane domains, every index, count and flag the shim passes to upstream, the
        // array sizes upstream indexes); buf is valid for buf.len() writes and the shim
        // honours the capacity; len is valid.
        unsafe {
            sys::sc_blend_fillet_standard_solver_p2_eval(
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

/// Runs the `SmConstantDistanceFS` edge-use constructor (layout in the module docs);
/// `path` is the `.smb` file of a file B-rep block (ignored otherwise).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed B-rep block, an edge use index outside
/// the export, non-finite arguments or UV fractions outside `[0, 1]`;
/// [`FfiError::Status`] for a shim failure.
pub fn eval_edgeuse(path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    use super::topology_edgeuse_p1 as p1;
    let mut rd = p1::Reader { input, at: 0 };
    p1::brep_block(&mut rd, path)?;
    let block = input.get(..rd.at).ok_or_else(|| bad("input too short"))?;
    let export = p1::eval(p1::Op::Export, path, block)?;
    let Some(c) = p1::counts(&export) else {
        // the B-rep could not be built: the op reports the same failure
        return Ok(export);
    };
    let n_eu = c.get(6).copied().unwrap_or(0);
    let mode = rd.below(2)?;
    rd.below(n_eu)?;
    for _ in 0..4 {
        rd.finite()?;
    }
    if mode == 1 {
        // the rail of crSurface1 (`1 - lRailIndex` underflows past 1)
        rd.below(2)?;
    }
    for _ in 0..2 {
        let f = rd.finite()?;
        if !(0.0..=1.0).contains(&f) {
            return Err(bad("UV fraction outside [0, 1]"));
        }
    }
    if mode == 1 {
        let ext = rd.finite()?;
        if !(0.0..=MAX_EXTENSION_FACTOR).contains(&ext) {
            return Err(bad("extension factor outside [0, MAX_EXTENSION_FACTOR]"));
        }
    }
    rd.done()?;
    let cpath = std::ffi::CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: cpath is a valid NUL-terminated string; input is valid for
        // input.len() reads and holds what the shim reads (validated above: the B-rep
        // block topology.edgeuse_p1 accepts, an edge use index inside its export and
        // finite arguments; the shim checks the pointers the constructor dereferences);
        // buf is valid for buf.len() writes and the shim honours the capacity; len is
        // valid.
        unsafe {
            sys::sc_blend_fillet_standard_solver_p2_eu_eval(
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

/// Largest number of intersection points [`recalc`] sets.
pub const MAX_TSECTS: usize = 64;

fn validate_recalc(input: &[f64]) -> Result<(), FfiError> {
    use super::blend_fillet_geom_p1::{
        curve, surface, validate_block, Reader as P1Reader, MAX_MAGNITUDE,
    };
    let mut rd = P1Reader {
        cursor: crate::reader::Cursor::new(input),
        bound: MAX_MAGNITUDE,
    };
    let b = validate_block(&mut rd)?;
    rd.opt_index(b.ng())?;
    let nt = rd.index(MAX_TSECTS)?;
    for _ in 0..nt {
        rd.opt_index(b.ne())?
            .ok_or_else(|| bad("intersection point: edge -1"))?;
        rd.index(1)?;
        rd.finites(11)?;
    }
    surface(&mut rd)?;
    surface(&mut rd)?;
    curve(&mut rd, 3, "centre curve")?;
    let v = rd.finites(9)?;
    if v.get(4).is_none_or(|&tol| tol <= 0.0) {
        return Err(bad("approx tolerance not positive"));
    }
    rd.flag()?;
    if rd.flag()? {
        curve(&mut rd, 3, "reference curve")?;
    }
    rd.done()
}

/// Runs `SmVariableRadiusFS::ReCalcFilletGeom` (layout in the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn recalc(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate_recalc(input)?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads (validated above: the pseudo B-rep block, indices inside it, finite
        // values, well-formed curves and surfaces); buf is valid for buf.len() writes and
        // the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_blend_fillet_standard_solver_p2_recalc(
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
