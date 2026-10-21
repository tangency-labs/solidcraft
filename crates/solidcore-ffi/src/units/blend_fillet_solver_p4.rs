//! FFI wrappers for unit `blend.fillet_solver_p4`.
//!
//! Shims: `harness/cpp/shim/blend_fillet_solver_p4.cpp` (`ComputePointValues`, the
//! `sdStaticStep` probe) and `harness/cpp/shim/blend_fillet_solver_p4_calc.cpp`
//! (`CalcFilletGeom`). Upstream: `source/SMLib/src/SmFilletSolver.cpp`
//! (`SmFilletSolver::CalcFilletGeom` 1495-2673, `SmFilletSolver::ComputePointValues`
//! 3544-3768).
//!
//! # `point_values`
//!
//! [`point_values`] builds surfaces A and B (B may be A itself; [`SurfaceIn`]: a real
//! `SmPlane`, `SmBSplineSurface` or `SmSurfOfRevolution`), the solver
//! `SmConstantRadiusFS(ctx, tol, ang_tol, tangency_tol, r0, r1, A, B, orient0, orient1)`
//! (with [`PointProgram::fd`], a derived class whose only override is
//! `OffsetRadiiCanChange() == TRUE`, so the finite-difference branch runs), and calls
//! `ComputePointValues(uv, dom1, dom2, dir, point, prev)`. The record is
//! `status point[80] offset0 offset1` (status `0` = `SM_SUCCESS`, else the `SmStatus`; the
//! point after the call, whatever the status; the solver's offset distances), or
//! `-1 build_status` when a surface of revolution fails to build.
//!
//! A point is [`POINT_LEN`] values: `type ptype[4] (uv pos, uv tangent) x 2, surface
//! matrix [3][3] x 2 (row major, 3 values each), curve pos, curve tangent, parameter,
//! angle, deviation, user[4]` (the layout of `blend_fillet_intersector`).
//!
//! # `static_step`
//!
//! [`static_step`] returns the value the library's function-static `sdStaticStep` (ND-3)
//! holds: the shim runs the library's `ComputePointValues` once on a fixed
//! finite-difference crossing point (which latches the static with `1e-5` when nothing
//! has latched it) and replays its finite-difference block with `1e-3` and `1e-5`; the
//! replay that reproduces the library's curve tangent bit for bit names the value.
//!
//! # `calc_geom`
//!
//! [`calc_geom`] builds `SmPrimitiveCreation::CreateRectangle(ctx, tol, w, h, frame)`
//! (with [`CalcProgram::uv_curves`] its face's UV trim curves made first by
//! `SmFace::CreateUVTrimCurves(FALSE, ..)`; exported in traversal order by
//! [`calc_target`], layout of `topology.edgeuse_p1`), a
//! solver derived from `SmConstantRadiusFS` (surface-only constructor over the face's
//! surface for both rails) whose only override is `SetExtensionFlags` (it sets the given
//! flags: the base version dereferences the executive, unset here), sets its edge uses to
//! the first (and second, or the first again) edge use of edge [`CalcProgram::edge`],
//! its G1 knots, optionally replaces the edge's curve with an `SmHermiteCurve`, optionally
//! adds one bare `SmFilletGeom(this, SM_FG_DEFAULT)` (no rails) with its split flag, and
//! calls `CalcFilletGeom`. The record is `status solver_status extend_before extend_after
//! n_g1 g1[n_g1] knots_in[n_g1]` (the G1 knots after the call; the knots it was given).
//!
//! Guards: every input finite and at most [`MAX_COORD`] in magnitude (the trace
//! direction `dir` only finite: upstream merely scales a cross product of unit normals
//! by it before a length test, `Unitize` and a sign test, so any finite value is
//! defined); well-formed clamped
//! B-spline surfaces (non-rational weights [`NOW`]); surfaces of revolution as unit
//! `surface.surf_of_revolution_p1` accepts them; point type codes in range. For
//! `calc_geom`: a B-spline edge curve (no Hermite replacement) needs the fillet geometry
//! (upstream reads `m_vFilletGeoms[0]` of an empty array otherwise); G1 knots are
//! fractions strictly inside the edge interval, at most [`MAX_G1`]; positive sizes.

use crate::units::surface_surf_of_revolution_p1::{push_surf, Surf};
use crate::{call_with_buffer, check, FfiError};

mod sys {
    extern "C" {
        pub fn sc_blend_fillet_solver_p4_point_values(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_blend_fillet_solver_p4_static_step(step: *mut f64) -> i32;
        pub fn sc_blend_fillet_solver_p4_calc_target(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_blend_fillet_solver_p4_calc_geom(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Largest input magnitude.
pub const MAX_COORD: f64 = 1e9;
/// `NL_NOW`: the weight of a non-rational control point.
pub const NOW: f64 = -f64::MAX;
/// An offset the solver does not have.
pub const UNSET: f64 = -777.0;
/// Values per point.
pub const POINT_LEN: usize = 80;
/// Most G1 knots of a `calc_geom` program.
pub const MAX_G1: usize = 8;

/// A surface.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceIn {
    /// `SmPlane(o, x, y, SmVector2d(scale), SmExtent2d(domain), ctx)`.
    Plane {
        /// Origin.
        o: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// u and v scales.
        scale: [f64; 2],
        /// `umin vmin umax vmax`.
        domain: [f64; 4],
    },
    /// `SmBSplineSurface(gw_SURFACE*, FALSE, ctx)`.
    BSpline {
        /// u degree.
        p: usize,
        /// v degree.
        q: usize,
        /// Rows (u index) of `[x, y, z, w]` (`w` = [`NOW`] when non-rational).
        net: Vec<Vec<[f64; 4]>>,
        /// u knots.
        u: Vec<f64>,
        /// v knots.
        v: Vec<f64>,
    },
    /// An `SmSurfOfRevolution` (cylinder, cone, sphere, torus shapes), the block of unit
    /// `surface.surf_of_revolution_p1`.
    Revolution(Surf),
}

/// A `ComputePointValues` program (see the module docs).
#[derive(Debug, Clone, PartialEq)]
pub struct PointProgram {
    /// Surface A.
    pub a: SurfaceIn,
    /// Surface B, `None` for A itself.
    pub b: Option<SurfaceIn>,
    /// `crUVDomain1`.
    pub dom1: [f64; 4],
    /// `crUVDomain2`.
    pub dom2: [f64; 4],
    /// Offset radii of A and B.
    pub radii: [f64; 2],
    /// `bOrientationSurface1/2`.
    pub orient: [bool; 2],
    /// `dThisApproxTol3d`.
    pub tol: f64,
    /// `dAngleTolerance`.
    pub ang_tol: f64,
    /// `dTangencyTolerance`.
    pub tangency_tol: f64,
    /// `OffsetRadiiCanChange() == TRUE` (the finite-difference solver).
    pub fd: bool,
    /// `aUVValues`.
    pub uv: [[f64; 2]; 2],
    /// `dCurveTraceDirection`.
    pub dir: f64,
    /// The point ([`POINT_LEN`] values).
    pub point: Vec<f64>,
    /// `pOptPreviousPnt` ([`POINT_LEN`] values), `None` for `NULL`.
    pub prev: Option<Vec<f64>>,
}

/// A `CalcFilletGeom` program (see the module docs).
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::struct_excessive_bools, reason = "independent upstream flags")]
pub struct CalcProgram {
    /// Rectangle width (along X).
    pub w: f64,
    /// Rectangle height (along Y).
    pub h: f64,
    /// B-rep tolerance.
    pub tol: f64,
    /// Make the face's UV trim curves first (`SmFace::CreateUVTrimCurves(FALSE, ..)`,
    /// status ignored), before the export and the call.
    pub uv_curves: bool,
    /// Edge index in traversal order (0..4).
    pub edge: usize,
    /// `m_pEdgeuses[1]` is `m_pEdgeuses[0]` (else the edge's second edge use when it has
    /// one).
    pub eu1_same: bool,
    /// Replace the edge's curve with an `SmHermiteCurve` (not an `SmBSplineCurve`).
    pub hermite: bool,
    /// Add one bare fillet geometry.
    pub geom: bool,
    /// Its split flag.
    pub split: bool,
    /// `m_bExtendBefore`, `m_bExtendAfter` set by `SetExtensionFlags`.
    pub extend: [bool; 2],
    /// G1 knots as fractions of the edge interval (`SmExtent1d::Evaluate`).
    pub g1: Vec<f64>,
    /// Offset radii.
    pub radii: [f64; 2],
    /// Orientations.
    pub orient: [bool; 2],
}

fn bad(msg: &str) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

fn finite(x: f64) -> Result<f64, FfiError> {
    if x.is_finite() && x.abs() <= MAX_COORD {
        Ok(x)
    } else {
        Err(bad("value not finite or too large"))
    }
}

/// Clamped, non-decreasing knots with interior multiplicity at most `p`.
fn check_knots(knots: &[f64], ncp: usize, p: usize) -> Result<(), FfiError> {
    if knots.len() != ncp + p + 1 {
        return Err(bad("knot count"));
    }
    if knots.windows(2).any(|w| w.first() > w.last()) {
        return Err(bad("knots decreasing"));
    }
    let (Some(&k0), Some(&km)) = (knots.first(), knots.last()) else {
        return Err(bad("knot count"));
    };
    if km <= k0
        || knots.iter().take(p + 1).any(|&k| k != k0)
        || knots.iter().rev().take(p + 1).any(|&k| k != km)
    {
        return Err(bad("knots not clamped"));
    }
    let mut run = 0usize;
    let mut last = f64::NAN;
    for &k in knots.iter().filter(|&&k| k != k0 && k != km) {
        run = if k == last { run + 1 } else { 1 };
        last = k;
        if run > p {
            return Err(bad("interior knot multiplicity"));
        }
    }
    Ok(())
}

/// All [`NOW`] or all in `(0, MAX_COORD]`.
fn check_weights<'a>(mut ws: impl Iterator<Item = &'a [f64; 4]>) -> Result<(), FfiError> {
    let Some(first) = ws.next() else {
        return Err(bad("no control points"));
    };
    let rational = first[3] != NOW;
    let ok = |w: f64| {
        if rational {
            w > 0.0 && w <= MAX_COORD
        } else {
            w == NOW
        }
    };
    if !ok(first[3]) || ws.any(|c| !ok(c[3])) {
        return Err(bad("weights"));
    }
    Ok(())
}

#[derive(Default)]
struct Enc {
    v: Vec<f64>,
}

impl Enc {
    fn num(&mut self, x: f64) -> Result<(), FfiError> {
        self.v.push(finite(x)?);
        Ok(())
    }
    fn nums(&mut self, xs: &[f64]) -> Result<(), FfiError> {
        xs.iter().try_for_each(|&x| self.num(x))
    }
    /// A value guarded by finiteness only (any finite magnitude is well defined).
    fn any_finite(&mut self, x: f64) -> Result<(), FfiError> {
        if !x.is_finite() {
            return Err(bad("value not finite"));
        }
        self.v.push(x);
        Ok(())
    }
    fn int(&mut self, x: usize) {
        self.v.push(x as f64);
    }
    fn flag(&mut self, b: bool) {
        self.v.push(if b { 1.0 } else { 0.0 });
    }

    fn surface(&mut self, s: &SurfaceIn) -> Result<(), FfiError> {
        match s {
            SurfaceIn::Plane {
                o,
                x,
                y,
                scale,
                domain,
            } => {
                self.int(0);
                self.nums(o)?;
                self.nums(x)?;
                self.nums(y)?;
                self.nums(scale)?;
                self.nums(domain)
            }
            SurfaceIn::BSpline { p, q, net, u, v } => {
                let nu = net.len();
                let nv = net.first().map_or(0, Vec::len);
                if !(1..=8).contains(p) || !(1..=8).contains(q) {
                    return Err(bad("surface degree"));
                }
                if !(2..=17).contains(&nu) || !(2..=17).contains(&nv) || *p >= nu || *q >= nv {
                    return Err(bad("surface size"));
                }
                if net.iter().any(|r| r.len() != nv) {
                    return Err(bad("ragged net"));
                }
                check_knots(u, nu, *p)?;
                check_knots(v, nv, *q)?;
                check_weights(net.iter().flatten())?;
                self.int(1);
                self.int(*p);
                self.int(*q);
                self.int(nu);
                self.int(nv);
                for c in net.iter().flatten() {
                    self.nums(&c[..3])?;
                    self.v.push(c[3]);
                }
                self.nums(u)?;
                self.nums(v)
            }
            SurfaceIn::Revolution(r) => {
                let mut block = Vec::new();
                push_surf(&mut block, r)?;
                // the revolution guard bounds coordinates only by finiteness
                if block
                    .iter()
                    .any(|&x| x != NOW && !(x.is_finite() && x.abs() <= MAX_COORD))
                {
                    return Err(bad("revolution value too large"));
                }
                self.int(3);
                self.v.extend(block);
                Ok(())
            }
        }
    }

    fn point(&mut self, p: &[f64]) -> Result<(), FfiError> {
        if p.len() != POINT_LEN {
            return Err(bad("point length"));
        }
        let code = |x: f64, hi: f64| (0.0..=hi).contains(&x) && x.fract() == 0.0;
        let ok = p
            .iter()
            .take(5)
            .enumerate()
            .all(|(k, &t)| code(t, if k == 0 { 7.0 } else { 5.0 }));
        if !ok {
            return Err(bad("point type codes"));
        }
        self.nums(p)
    }
}

/// The flat program [`point_values`] sends (validated).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for anything [`point_values`] rejects (module docs).
pub fn encode_point(prog: &PointProgram) -> Result<Vec<f64>, FfiError> {
    let mut e = Enc::default();
    e.surface(&prog.a)?;
    match &prog.b {
        None => e.int(2),
        Some(b) => e.surface(b)?,
    }
    e.nums(&prog.dom1)?;
    e.nums(&prog.dom2)?;
    e.nums(&prog.radii)?;
    e.flag(prog.orient[0]);
    e.flag(prog.orient[1]);
    e.nums(&[prog.tol, prog.ang_tol, prog.tangency_tol])?;
    e.flag(prog.fd);
    for uv in &prog.uv {
        e.nums(uv)?;
    }
    // dCurveTraceDirection only scales the normals' cross product (and the stepped-off
    // one) before a length test, Unitize and a sign test: any finite value is defined
    // (an overflowing length is inf, Unitize then gives zeros; inf * 0 is NaN, which
    // only fails comparisons), so it is bounded by finiteness alone.
    e.any_finite(prog.dir)?;
    e.point(&prog.point)?;
    match &prog.prev {
        None => e.flag(false),
        Some(p) => {
            e.flag(true);
            e.point(p)?;
        }
    }
    Ok(e.v)
}

/// Runs `ComputePointValues` on `prog` (see the module docs) and returns the record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a rejected program; [`FfiError::Status`] for a shim
/// failure.
pub fn point_values(prog: &PointProgram) -> Result<Vec<f64>, FfiError> {
    let input = encode_point(prog)?;
    let n_in = i64::try_from(input.len()).map_err(|_| bad("program too long"))?;
    call_with_buffer(128, |buf: &mut [f64], len| {
        // SAFETY: `input` is valid for `n_in` reads; `buf` for `buf.len()` writes; `len`
        // is a valid i64; nothing is retained.
        unsafe {
            sys::sc_blend_fillet_solver_p4_point_values(
                input.as_ptr(),
                n_in,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// The value the library's `sdStaticStep` holds (latching it with `1e-5` first when
/// nothing has; see the module docs).
///
/// # Errors
///
/// [`FfiError::Status`] when the probe's `ComputePointValues` fails or it cannot tell
/// the value (`SC_ERR_EXCEPTION`).
pub fn static_step() -> Result<f64, FfiError> {
    let mut step = 0.0_f64;
    // SAFETY: `step` is a valid f64 the shim writes once; nothing is retained.
    let status = unsafe { sys::sc_blend_fillet_solver_p4_static_step(&raw mut step) };
    check(status)?;
    Ok(step)
}

fn rect(e: &mut Enc, w: f64, h: f64, tol: f64, uv_curves: bool) -> Result<(), FfiError> {
    let sized = |x: f64| x.is_finite() && (1e-6..=1e6).contains(&x);
    if !sized(w) || !sized(h) {
        return Err(bad("rectangle size"));
    }
    if !(tol.is_finite() && tol > 0.0 && tol <= 0.01 * w.min(h)) {
        return Err(bad("rectangle tolerance"));
    }
    e.nums(&[w, h, tol])?;
    e.flag(uv_curves);
    Ok(())
}

/// The flat program [`calc_geom`] sends (validated).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for anything [`calc_geom`] rejects (module docs).
pub fn encode_calc(prog: &CalcProgram) -> Result<Vec<f64>, FfiError> {
    let mut e = Enc::default();
    rect(&mut e, prog.w, prog.h, prog.tol, prog.uv_curves)?;
    if prog.edge >= 4 {
        return Err(bad("edge index"));
    }
    if !prog.hermite && !prog.geom {
        return Err(bad(
            "a B-spline edge curve reaches m_vFilletGeoms[0] of an empty array",
        ));
    }
    if prog.g1.len() > MAX_G1 || prog.g1.iter().any(|&f| !(f > 0.0 && f < 1.0)) {
        return Err(bad("G1 knot fractions"));
    }
    e.int(prog.edge);
    e.flag(prog.eu1_same);
    e.flag(prog.hermite);
    e.flag(prog.geom);
    e.flag(prog.split);
    e.flag(prog.extend[0]);
    e.flag(prog.extend[1]);
    e.int(prog.g1.len());
    e.nums(&prog.g1)?;
    e.nums(&prog.radii)?;
    e.flag(prog.orient[0]);
    e.flag(prog.orient[1]);
    Ok(e.v)
}

fn run_buffer(
    input: &[f64],
    f: unsafe extern "C" fn(*const f64, i64, *mut f64, i64, *mut i64) -> i32,
) -> Result<Vec<f64>, FfiError> {
    let n_in = i64::try_from(input.len()).map_err(|_| bad("program too long"))?;
    call_with_buffer(1024, |buf: &mut [f64], len| {
        // SAFETY: `input` is valid for `n_in` reads; `buf` for `buf.len()` writes; `len`
        // is a valid i64; nothing is retained.
        unsafe {
            f(
                input.as_ptr(),
                n_in,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// The rectangle B-rep of a `calc_geom` program (with its UV trim curves made first when
/// `uv_curves`, as [`CalcProgram::uv_curves`]), exported as unit `topology.edgeuse_p1`
/// does (`0` then the graph).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for bad sizes; [`FfiError::Status`] for a shim failure.
pub fn calc_target(w: f64, h: f64, tol: f64, uv_curves: bool) -> Result<Vec<f64>, FfiError> {
    let mut e = Enc::default();
    rect(&mut e, w, h, tol, uv_curves)?;
    run_buffer(&e.v, sys::sc_blend_fillet_solver_p4_calc_target)
}

/// Runs `CalcFilletGeom` on `prog` (see the module docs) and returns the record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a rejected program; [`FfiError::Status`] for a shim
/// failure.
pub fn calc_geom(prog: &CalcProgram) -> Result<Vec<f64>, FfiError> {
    run_buffer(
        &encode_calc(prog)?,
        sys::sc_blend_fillet_solver_p4_calc_geom,
    )
}
