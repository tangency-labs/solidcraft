//! FFI wrappers for unit `blend.fillet_standard_solver_p1`.
//!
//! Shim: `harness/cpp/shim/blend_fillet_standard_solver_p1.cpp`
//! (`sc_blend_fillet_standard_solver_p1_eval`).
//! Upstream: `source/SMLib/src/SmFilletStandardSolver.cpp`.
//!
//! Encodings. A B-spline curve (`bcurve`) is `n p m`, `n+1` control points `x y z w`,
//! `m+1` knots (`SmBSplineCurve(gw_CURVE*, 3, FALSE)`). A curve is `kind`: `0 oob bcurve`
//! (out-of-bounds evaluation flag), `1 start[3] end[3]` (`SmLine(start, end, 3, ctx)`) or
//! `2 p1[3] d1[3] p2[3] d2[3]` (`SmHermiteCurve`). A surface is `kind`: `0 n m p q r s`,
//! `(n+1)(m+1)` control points (v index fastest), `r+1` u knots, `s+1` v knots
//! (`SmBSplineSurface(gw_SURFACE*, FALSE, NULL)`), or `1 o[3] x[3] y[3] sx sy u0 v0 u1 v1`
//! (`SmPlane(o, x, y, (sx, sy), domain, ctx)`). `kinds` is `nk` and `nk` type codes passed
//! to `IsKindOf`. A constant-radius solver block (`cr`) is `same surfaceA [surfaceB] r0 r1
//! o0 o1` (`SmConstantRadiusFS(ctx, 1e-6, 30deg, 2deg, r0, r1, A, B or A, o0, o1)`). A
//! constant-distance solver block (`cd`) is `same surfaceA [surfaceB] d0 d1 o0 o1 dist ext`:
//! an `SmConstantDistanceFS` built on an `SmApiCreateBox` edge use whose rail `i` offsets
//! its base by `d_i` with orientation `o_i`, distance `dist`, extension factor `ext` and no
//! edge uses. A variable-radius block (`vr`) is `surfaceA surfaceB o0 o1 curve law orient
//! fsg` with `law` `0 r0 r1` (`SmLinearFilletLaw`) or `1 bcurve e0 e1`
//! (`SmBSplineFilletLaw(curve, [e0, e1])`) and `fsg` 1 linear, 2 circular, 3 blend curve.
//! A surface selector is `0` `NULL`, `1` `GetSurface(0)`, `2` `GetSurface(1)`, `3` another
//! offset surface of A, `4` an offset surface of a copy of A.
//!
//! | [`Op`]      | upstream                                              | parameters |
//! |-------------|-------------------------------------------------------|------------|
//! | `Circ`      | `SmCircularCrossSectionFSG::CreateSurfaceFromCurves`   | `approx tolpct mirror complement tol g1 center rail1 rail2` (bcurves) |
//! | `Blend`     | `SmBlendCurveCrossSectionFSG::CreateSurfaceFromCurves` | `cont scale tol g1 center rail1 rail2 sides [surface1 uv1 surface2 uv2]` |
//! | `Linear`    | `SmLinearCrossSectionFSG::CreateSurface`               | `rail1 rail2` |
//! | `Base`      | `SmCurveBasedFS::EvaluateBaseCurve` (successive calls) | `curve calls` `calls` x `t nderiv`, `kinds` |
//! | `SsExt`     | `SmSurfaceSurfaceFS::SetupOffsetExtension`             | `ext surface u0 v0 u1 v1 kinds` |
//! | `Cr`        | `SmConstantRadiusFS` constructor, `SetupOffsetValues` and queries | `cr set d0 d1 setup user kinds` |
//! | `CrJac`     | `SmConstantRadiusFS::LoadJacobian`                     | `cr rail ne np sel1 off1 sel2 off2 nx x[nx] nf f[nf] has_j rows cols` |
//! | `CdSetup`   | `SmConstantDistanceFS::SetupOffsetValues` and queries  | `cd user kinds` |
//! | `CdJac`     | `SmConstantDistanceFS::LoadJacobian`                   | `cd rail ne np sel1 off1 sel2 off2 nx x[nx] nf f[nf] has_j rows cols` |
//! | `CdInit`    | `SmConstantDistanceFS::LoadInitialValues`              | `cd do2 rail sel1 s1_index s2_index dim` `dim` x `min max`, `np per[np] ng guess[ng]` |
//! | `Vr`        | `SmVariableRadiusFS` queries                           | `vr set e0 e1 kinds` |
//! | `VrSetup`   | `SmVariableRadiusFS::SetupOffsetValues`                | `vr user` |
//!
//! Output: the op status (`0` = `SM_SUCCESS`, else the `SmStatus`; `-1` when the shim
//! rejects the input, see below) and then: `Circ`, `Blend` the final centre line and rails
//! (`has n p m cpts knots`) and on success the surface (`has n m p q r s cpts u v`);
//! `Linear` on success the surface; `Base` per call its status and on success
//! `nderiv + 1` vectors, then the extended curve, the solver type and the types;
//! `SsExt` the domain `u0 v0 u1 v1`, the solver type and the types; `Cr` the offset
//! distance returned, both radii, orientations and offset distances, the conversion
//! tolerance, the extension factor, the Jacobian size, `OffsetRadiiCanChange`, the solver
//! type and the types; `CrJac` `ne np off1 off2 found f[nf]`, the Jacobian when given and
//! both offset distances; `CdSetup` the offset distance returned, both offset distances,
//! the distance, `CanMakeAnalytic`, the Jacobian size, the solver type and the types;
//! `CdJac` `ne np found f[nf]`, the Jacobian and both offset distances; `CdInit` the rail of
//! `rpSurface2` (`-1` `NULL`), `rlSurface2Index`, the intervals, `np' per[np']`,
//! `ng' guess[ng']` and both offset distances; `Vr` `OffsetRadiiCanChange`, the law
//! orientation, the law interval, whether the law is the one given, the Jacobian size, the
//! solver type and the types; `VrSetup` the offset distance returned and both offset
//! distances. Types are `GetType`, `len chars[len]` of `GetTypeString` and `IsKindOf` per
//! code.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: well-formed
//! clamped 3D curves and surfaces (`1 <= p <= n`, at most [`MAX_INDEX`] control points per
//! direction, degrees at most [`MAX_DEGREE`], finite data, consistent rationality,
//! positive weights, non-decreasing knots with end multiplicity exactly `degree + 1` and
//! interior multiplicity at most the degree); a non-negative tolerance; `Circ` and `Blend`
//! rails over the same interval (the skinning re-intersects rail 2 at
//! `pIntersectionsWithRails[k..2k]`, past the `k` section parameters, when synchronising
//! the rails changes an interval); `Blend` surfaces with its UV curves; `Base` at most
//! [`MAX_CALLS`] calls with at most [`MAX_DERIV`] derivatives and parameters within
//! [`MAX_PARAM`]; the Jacobians every `crX`, `rF` and Jacobian entry the upstream index
//! logic reads or writes in range and `crX` within [`MAX_PARAM`]; `CdInit` a non-`NULL`
//! first surface and the guess and interval indices it reads and writes in range (with
//! `do2` also rail 0 or 1); `Vr`, `VrSetup` a surface generator type 1 to 3; the base
//! surfaces of the solvers' offset surfaces, when planes, a domain with `u0 <= u1` and
//! `v0 <= v1` (`SmPlane::MakeNurb` takes its knots from the domain, so a reversed one
//! gives NLib a decreasing knot vector; the offset surface's extension then fails inside
//! `N_SrfEvalPtDerivsUnbounded` and its retry reads the half-built extension, a
//! segfault for a domain reversed by `1e10`). Two inputs
//! make upstream read out of bounds only after work the validation cannot predict; the
//! shim detects them with a dry run and rejects them (status `-1`): a ruled or revolved
//! circular-section surface with approximated arcs of different knot counts (the second
//! pass indexes past the three cross sections), and a blend-curve skin (skinned as
//! synchronised) through a point cross section or with the `G1` cross-boundary
//! derivative surfaces (the first skinning attempt reads a 2-point derivative curve as if
//! it had the cross sections' control point count), or whose knot insertion adds no knot
//! (upstream loops forever).
//!
//! [`eval_edgeuse`] (shim `harness/cpp/shim/blend_fillet_standard_solver_p1_eu.cpp`,
//! `sc_blend_fillet_standard_solver_p1_eu_eval`) runs the `SmConstantRadiusFS` edge-use
//! constructor on a B-rep block of unit `topology.edgeuse_p1`
//! ([`super::topology_edgeuse_p1`]) followed by `eu tol ang tan r has_r2 r2 fu fv`: the
//! edge use's index in the export's traversal order, the constructor's tolerances and
//! radii (`pdOptSecondRadius` is `&r2` when `has_r2`) and the UV fractions of each offset
//! surface's natural domain where it is evaluated. Output: `0`, or `-1 status` when the
//! B-rep could not be built; then `-2` when upstream would dereference null (the edge
//! use or its radial is not a loop edge use with edge, curve, face use, face, surface,
//! vertex uses and vertices), or the status (`SM_ERR` when the constructor signalled
//! tangent surfaces and cleared its edge uses) and on success per rail `eu radius
//! orientation offset face base_is_face_surface status P[3]` and the default fillet
//! geometry (the one the `SmFilletSolver` constructor adds, `m_vFilletGeoms[0]`):
//! `count solver_is_fs` and per rail `own` (`FinishConstruction` installed a copy, so
//! `GetOffsetSurface` does not fall back to the solver's) and with `own` `offset
//! base_is_face_surface status P[3]` (at `fu fv` of the copy's natural domain); then
//! the edge use's and its radial's `put_eu_state` (`EvaluateBinormal` may attach a UV
//! trim curve).

use crate::{call_with_buffer, FfiError};

/// Largest control point index accepted by [`eval`].
pub const MAX_INDEX: usize = 64;
/// Largest NURB degree accepted by [`eval`].
pub const MAX_DEGREE: usize = 9;
/// Largest array length (`x`, `f`, guesses, intervals, Jacobian sides, type codes)
/// accepted by [`eval`].
pub const MAX_ARRAY: usize = 32;
/// Largest number of `EvaluateBaseCurve` calls accepted by [`eval`].
pub const MAX_CALLS: usize = 16;
/// Largest derivative count of an `EvaluateBaseCurve` call accepted by [`eval`].
pub const MAX_DERIV: usize = 3;
/// Largest `|parameter|` (curve parameters, `crX`, guesses) accepted by [`eval`].
pub const MAX_PARAM: f64 = 1.0e6;
/// Largest blend continuity accepted by [`eval`].
pub const MAX_CONTINUITY: usize = 8;
/// `NL_NOZ`.
const NOZ: f64 = f64::MAX;
/// `NL_NOW`.
const NOW: f64 = -f64::MAX;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `SmCircularCrossSectionFSG::CreateSurfaceFromCurves`.
    Circ,
    /// `SmBlendCurveCrossSectionFSG::CreateSurfaceFromCurves`.
    Blend,
    /// `SmLinearCrossSectionFSG::CreateSurface`.
    Linear,
    /// `SmCurveBasedFS::EvaluateBaseCurve`.
    Base,
    /// `SmSurfaceSurfaceFS::SetupOffsetExtension`.
    SsExt,
    /// The `SmConstantRadiusFS` surface constructor, `SetupOffsetValues` and queries.
    Cr,
    /// `SmConstantRadiusFS::LoadJacobian`.
    CrJac,
    /// `SmConstantDistanceFS::SetupOffsetValues` and queries.
    CdSetup,
    /// `SmConstantDistanceFS::LoadJacobian`.
    CdJac,
    /// `SmConstantDistanceFS::LoadInitialValues`.
    CdInit,
    /// The `SmVariableRadiusFS` queries.
    Vr,
    /// `SmVariableRadiusFS::SetupOffsetValues`.
    VrSetup,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::Circ => 0,
            Op::Blend => 1,
            Op::Linear => 2,
            Op::Base => 3,
            Op::SsExt => 4,
            Op::Cr => 5,
            Op::CrJac => 6,
            Op::CdSetup => 7,
            Op::CdJac => 8,
            Op::CdInit => 9,
            Op::Vr => 10,
            Op::VrSetup => 11,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_blend_fillet_standard_solver_p1_eu_eval(
            path: *const std::ffi::c_char,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_blend_fillet_standard_solver_p1_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

pub(crate) fn bad(msg: impl Into<String>) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

/// Sequential reader over the input.
pub(crate) struct Reader<'a> {
    pub(crate) input: &'a [f64],
    pub(crate) at: usize,
}

impl Reader<'_> {
    pub(crate) fn num(&mut self) -> Result<f64, FfiError> {
        let v = self
            .input
            .get(self.at)
            .copied()
            .ok_or_else(|| bad("input too short"))?;
        self.at += 1;
        Ok(v)
    }

    pub(crate) fn finite(&mut self) -> Result<f64, FfiError> {
        let v = self.num()?;
        if !v.is_finite() {
            return Err(bad("non-finite value"));
        }
        Ok(v)
    }

    pub(crate) fn param(&mut self) -> Result<f64, FfiError> {
        let v = self.finite()?;
        if v.abs() > MAX_PARAM {
            return Err(bad("parameter too large"));
        }
        Ok(v)
    }

    pub(crate) fn finites(&mut self, k: usize) -> Result<Vec<f64>, FfiError> {
        (0..k).map(|_| self.finite()).collect()
    }

    pub(crate) fn index(&mut self, max: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
            return Err(bad(format!("bad index/count {v}")));
        }
        Ok(v as usize)
    }

    pub(crate) fn flag(&mut self) -> Result<bool, FfiError> {
        Ok(self.index(1)? == 1)
    }

    pub(crate) fn done(&self) -> Result<(), FfiError> {
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
            return Err(bad(format!("{what}: interior multiplicity above the degree")));
        }
    }
    Ok(())
}

/// Checks that all points are 3D and share their rational marker.
fn check_points(pts: &[[f64; 4]], what: &str) -> Result<(), FfiError> {
    let [_, _, _, w0] = *pts.first().ok_or_else(|| bad(format!("{what}: no points")))?;
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

/// Reads and validates a B-spline curve `n p m cpts knots`; returns its interval.
pub(crate) fn bcurve(rd: &mut Reader<'_>) -> Result<(f64, f64), FfiError> {
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
    let a = knots.first().copied().unwrap_or(0.0);
    let b = knots.last().copied().unwrap_or(0.0);
    Ok((a, b))
}

/// Reads and validates a curve `kind ...`.
fn curve(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match rd.index(2)? {
        0 => {
            rd.flag()?;
            bcurve(rd)?;
        }
        1 => {
            rd.finites(6)?;
        }
        _ => {
            rd.finites(12)?;
        }
    }
    Ok(())
}

/// Reads and validates a surface `kind ...`.
fn surface(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    if rd.index(1)? == 1 {
        rd.finites(15)?;
        return Ok(());
    }
    bspline_surface(rd)
}

/// The B-spline part of [`surface`] (after its kind).
fn bspline_surface(rd: &mut Reader<'_>) -> Result<(), FfiError> {
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

/// A solver's base surface ([`surface`]): a plane's domain must not be reversed (its
/// NURB knots are the domain's corners, see the module docs).
fn offset_base(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    if rd.index(1)? == 1 {
        let d = rd.finites(15)?;
        if let [.., u0, v0, u1, v1] = d[..] {
            if u0 > u1 || v0 > v1 {
                return Err(bad("plane: reversed domain (decreasing NURB knots)"));
            }
        }
        return Ok(());
    }
    bspline_surface(rd)
}

fn kinds(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let nk = rd.index(MAX_ARRAY)?;
    for _ in 0..nk {
        rd.index(1_000_000)?;
    }
    Ok(())
}

fn tolerance(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    if rd.finite()? < 0.0 {
        return Err(bad("negative tolerance"));
    }
    Ok(())
}

/// The centre line and rails of the generators; the rails share their interval.
fn three_curves(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    bcurve(rd)?;
    let r1 = bcurve(rd)?;
    let r2 = bcurve(rd)?;
    if r1 != r2 {
        return Err(bad("rails over different intervals"));
    }
    Ok(())
}

fn validate_circ(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    rd.flag()?;
    rd.finite()?;
    rd.flag()?;
    rd.flag()?;
    tolerance(rd)?;
    rd.flag()?;
    three_curves(rd)
}

fn validate_blend(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    rd.index(MAX_CONTINUITY)?;
    rd.finite()?;
    tolerance(rd)?;
    rd.flag()?;
    three_curves(rd)?;
    if rd.flag()? {
        surface(rd)?;
        bcurve(rd)?;
        surface(rd)?;
        bcurve(rd)?;
    }
    Ok(())
}

fn validate_base(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    curve(rd)?;
    let calls = rd.index(MAX_CALLS)?;
    for _ in 0..calls {
        rd.param()?;
        rd.index(MAX_DERIV)?;
    }
    kinds(rd)
}

/// A constant-radius or constant-distance block; returns whether both rails share
/// their base.
fn solver_block(rd: &mut Reader<'_>, distance: bool) -> Result<bool, FfiError> {
    let same = rd.flag()?;
    offset_base(rd)?;
    if !same {
        offset_base(rd)?;
    }
    if distance {
        rd.finites(6)?;
    } else {
        rd.finites(2)?;
        rd.flag()?;
        rd.flag()?;
    }
    Ok(same)
}

/// The base identity of a selector (`0` A, `1` B, `2` the copy of A).
fn selector_base(sel: usize, same: bool) -> Option<usize> {
    match sel {
        1 | 3 => Some(0),
        2 => Some(usize::from(!same)),
        4 => Some(2),
        _ => None,
    }
}

/// The `LoadJacobian` arguments (selectors as base identities).
struct JacArgs {
    rail: usize,
    ne: usize,
    np: usize,
    b1: Option<usize>,
    off1: usize,
    b2: Option<usize>,
    off2: usize,
    nx: usize,
    nf: usize,
    jac: Option<(usize, usize)>,
}

fn jac_args(rd: &mut Reader<'_>, same: bool) -> Result<JacArgs, FfiError> {
    let rail = rd.index(3)?;
    let ne = rd.index(MAX_ARRAY)?;
    let np = rd.index(MAX_ARRAY)?;
    let sel1 = rd.index(4)?;
    let off1 = rd.index(MAX_ARRAY)?;
    let sel2 = rd.index(4)?;
    let off2 = rd.index(MAX_ARRAY)?;
    let nx = rd.index(MAX_ARRAY)?;
    for _ in 0..nx {
        rd.param()?;
    }
    let nf = rd.index(MAX_ARRAY)?;
    rd.finites(nf)?;
    let has_j = rd.flag()?;
    let rows = rd.index(MAX_ARRAY)?;
    let cols = rd.index(MAX_ARRAY)?;
    Ok(JacArgs {
        rail,
        ne,
        np,
        b1: selector_base(sel1, same),
        off1,
        b2: selector_base(sel2, same),
        off2,
        nx,
        nf,
        jac: has_j.then_some((rows, cols)),
    })
}

fn validate_cr_jac(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let same = solver_block(rd, false)?;
    let a = jac_args(rd, same)?;
    let (base0, base1) = (0, usize::from(!same));
    // SmConstantRadiusFS::LoadJacobian's index logic
    let mut s1 = a.np;
    let mut n = a.np + 2;
    if a.rail == 0 && a.b1 == Some(base0) {
        s1 = a.off1;
        n = a.np;
    } else if a.rail == 1 && a.b2 == Some(base0) {
        s1 = a.off2;
        n = a.np;
    }
    let mut s2 = n;
    if a.rail == 1 && a.b1 == Some(base1) {
        s2 = a.off1;
    }
    if a.rail == 0 && a.b2 == Some(base1) {
        s2 = a.off2;
    }
    if s1 + 1 >= a.nx || s2 + 1 >= a.nx {
        return Err(bad("jacobian: crX too short for the UV offsets"));
    }
    if a.nf < a.ne + 3 {
        return Err(bad("jacobian: rF too short"));
    }
    if let Some((rows, cols)) = a.jac {
        if rows < a.ne + 3 || cols <= s1.max(s2) + 1 {
            return Err(bad("jacobian: matrix too small"));
        }
    }
    Ok(())
}

fn validate_cd_jac(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let same = solver_block(rd, true)?;
    let a = jac_args(rd, same)?;
    let (base0, base1) = (0, usize::from(!same));
    // SmConstantDistanceFS::LoadJacobian's index logic
    let mut n = a.np;
    let mut s1 = n;
    if a.rail == 0 && a.b1 == Some(base0) {
        s1 = a.off1;
    } else if a.rail == 1 && a.b2 == Some(base0) {
        s1 = a.off2;
    } else {
        n += 2;
    }
    let mut s2 = n;
    if a.rail == 1 && a.b1 == Some(base1) {
        s2 = a.off1;
    } else if a.rail == 0 && a.b2 == Some(base1) {
        s2 = a.off2;
    } else {
        n += 2;
    }
    let radius = n;
    if s1 + 1 >= a.nx || s2 + 1 >= a.nx || radius >= a.nx {
        return Err(bad("jacobian: crX too short"));
    }
    if a.nf < a.ne + 4 {
        return Err(bad("jacobian: rF too short"));
    }
    if let Some((rows, cols)) = a.jac {
        if rows < a.ne + 4 || cols <= s1.max(s2) + 2 {
            return Err(bad("jacobian: matrix too small"));
        }
    }
    Ok(())
}

fn validate_cd_init(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    solver_block(rd, true)?;
    let do2 = rd.flag()?;
    let rail = rd.index(3)?;
    let sel1 = rd.index(4)?;
    if sel1 == 0 {
        return Err(bad("init: NULL first surface"));
    }
    let s1_index = rd.index(MAX_ARRAY)?;
    let s2_index = rd.index(MAX_ARRAY)?;
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
        if s1_index + 1 >= ng || ng + 2 >= dim {
            return Err(bad("init: guess or interval index out of range"));
        }
    } else if s2_index + 2 >= dim {
        return Err(bad("init: interval index out of range"));
    }
    Ok(())
}

fn vr_block(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    offset_base(rd)?;
    offset_base(rd)?;
    rd.finites(2)?;
    curve(rd)?;
    if rd.index(1)? == 0 {
        rd.finites(2)?;
    } else {
        bcurve(rd)?;
        rd.finites(2)?;
    }
    rd.flag()?;
    let fsg = rd.index(3)?;
    if fsg == 0 {
        return Err(bad("vr: no fillet surface generator"));
    }
    Ok(())
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    let rd = &mut rd;
    match op {
        Op::Circ => validate_circ(rd)?,
        Op::Blend => validate_blend(rd)?,
        Op::Linear => {
            bcurve(rd)?;
            bcurve(rd)?;
        }
        Op::Base => validate_base(rd)?,
        Op::SsExt => {
            rd.finite()?;
            surface(rd)?;
            rd.finites(4)?;
            kinds(rd)?;
        }
        Op::Cr => {
            solver_block(rd, false)?;
            rd.flag()?;
            rd.finites(2)?;
            rd.flag()?;
            rd.finite()?;
            kinds(rd)?;
        }
        Op::CrJac => validate_cr_jac(rd)?,
        Op::CdSetup => {
            solver_block(rd, true)?;
            rd.finite()?;
            kinds(rd)?;
        }
        Op::CdJac => validate_cd_jac(rd)?,
        Op::CdInit => validate_cd_init(rd)?,
        Op::Vr => {
            vr_block(rd)?;
            rd.flag()?;
            rd.finites(2)?;
            kinds(rd)?;
        }
        Op::VrSetup => {
            vr_block(rd)?;
            rd.param()?;
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
    call_with_buffer(65536, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above: well-formed clamped 3D curves and surfaces,
        // every index, count and flag the shim passes to upstream, the array sizes
        // upstream indexes); buf is valid for buf.len() writes and the shim honours the
        // capacity; len is valid.
        unsafe {
            sys::sc_blend_fillet_standard_solver_p1_eval(
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

/// Runs the `SmConstantRadiusFS` edge-use constructor (layout in the module docs);
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
    rd.below(n_eu)?;
    for _ in 0..4 {
        rd.finite()?;
    }
    rd.flag()?;
    rd.finite()?;
    for _ in 0..2 {
        let f = rd.finite()?;
        if !(0.0..=1.0).contains(&f) {
            return Err(bad("UV fraction outside [0, 1]"));
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
            sys::sc_blend_fillet_standard_solver_p1_eu_eval(
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
