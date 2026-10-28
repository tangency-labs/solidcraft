//! FFI wrappers for unit `surface.surf_of_revolution_p1`.
//!
//! Shim: `harness/cpp/shim/surface_surf_of_revolution_p1.cpp`
//! (`sc_surface_surf_of_revolution_p1_eval`).
//! Upstream: `source/SMLib/src/SmSurfOfRevolution.cpp`.
//!
//! [`surface_op`] builds a surface of revolution from a [`Surf`] and runs one [`Op`] on
//! it; [`iso_circles`] runs `sm_TestForIsoCircles` on a B-spline surface. Both return
//! the shim's flat record:
//!
//! * surface ops: the construction status (`0` = `SM_SUCCESS`, else the `SmStatus`),
//!   then on success the op's record: its status and, on success, its outputs.
//!   `Build`: the state (no status). A state is `is_rev is_bspline is_surface bounded
//!   swap planar O[3] X[3] Y[3] D[4] start end MAXD[4] gen_dir[2] gen_param
//!   have_polar angle_count [first last] polar_curve gen_curve has_nurb [p q nu nv
//!   cpts(4 nu nv, v fastest) nku KU nkv KV]`, a curve `has [type step0 step1 has_nurb
//!   [p n cpts(4 n) nk knots]]`. `EvalStep`: `(k+1)^2` vectors (or the point);
//!   `ToStep`: `u v`; `Iso`: `gap curve`; `Directrix`: `radius axis[3] curve`;
//!   `Generator`: `curve`; `StartPlane`: `point[3] n angles(n) inside`; `Eq`: the
//!   other surface's construction status then (on success) `this == other`, `other
//!   == this`; `Plane`: `more ncurves curves types devs`.
//! * `iso_circles`: status then on success `found parallel same coaxial common planar
//!   radius a0 a1 frame(O X Y) line_point[3] line_vec[3]` (`NaN` where upstream
//!   writes nothing).
//!
//! The wrappers accept only what upstream handles without undefined behaviour: clamped
//! 3D curves and surfaces (`1 <= p <= n`, degrees at most [`MAX_DEGREE`], finite data,
//! positive weights or all non-rational, non-decreasing knots with end multiplicity
//! `p + 1` and interior multiplicity at most `p`), non-degenerate lines, circles with a
//! positive radius, independent axes and an angular domain inside `[-360, 360]` at most
//! 360 long, hyperbolas with positive semi axes and a STEP domain inside `[-10, 10]`
//! (the shims also refuse, as a C++ exception, a hyperbola whose constructor left no
//! finite NURB: `SmHyperbola` ignores a failing `MakeNurb`), finite rotation axes
//! (`CreateCanonical` makes a zero one `(0, 0, 1)`), independent unit placement axes
//! for the constructor (a longer Y axis leaves `IsOnPlane`'s output uninitialised),
//! ordered domains, finite parameters and derivative orders up to 3.

use crate::validate::{bad, NOW};
use crate::{call_with_buffer, FfiError};

/// Largest control point index accepted.
pub const MAX_INDEX: usize = 256;
/// Largest NURB degree accepted.
pub const MAX_DEGREE: usize = 9;

mod sys {
    extern "C" {
        pub fn sc_surface_surf_of_revolution_p1_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// A clamped 3D B-spline curve: control points `[x, y, z, w]` as stored (`w` is
/// `NL_NOW` for a non-rational curve), degree, knots.
#[derive(Debug, Clone, PartialEq)]
pub struct BSplineData {
    /// Control points as stored.
    pub cpts: Vec<[f64; 4]>,
    /// Degree.
    pub p: usize,
    /// Knots (`cpts.len() + p + 1` of them).
    pub knots: Vec<f64>,
}

/// A clamped B-spline surface: control net as stored (rows in u), degrees, knots.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceData {
    /// Control net `[x, y, z, w]`, `net[i][j]` for u index `i`, v index `j`.
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

/// The generating curve.
#[derive(Debug, Clone, PartialEq)]
pub enum Gen {
    /// `SmBSplineCurve(gw_CURVE*, 3, FALSE)`.
    BSpline(BSplineData),
    /// `SmLine(a, b, 3)` with its NURB made.
    Line {
        /// Start point.
        a: [f64; 3],
        /// End point.
        b: [f64; 3],
    },
    /// `SmCircle(c, x, y, [d0, d1], r, 3)`.
    Circle {
        /// Centre.
        c: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// Angular domain in degrees.
        d: [f64; 2],
        /// Radius.
        r: f64,
    },
    /// `SmHyperbola(c, x, y, [d0, d1], a, b, 3)`.
    Hyperbola {
        /// Centre.
        c: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// STEP domain (`|t| <= 10`).
        d: [f64; 2],
        /// Semi axis.
        a: f64,
        /// Semi imaginary axis.
        b: f64,
    },
}

/// How the surface is made.
#[derive(Debug, Clone, PartialEq)]
pub enum Build {
    /// `CreateCanonical(gen, point, axis, .., swap)`.
    Canonical {
        /// Point on the axis.
        point: [f64; 3],
        /// Axis direction.
        axis: [f64; 3],
    },
    /// The constructor `(gen, origin, x, y, domain, swap)` then `MakeNurb`.
    Ctor {
        /// Origin on the axis.
        origin: [f64; 3],
        /// X axis (0 degrees).
        x: [f64; 3],
        /// Y axis (90 degrees).
        y: [f64; 3],
        /// STEP domain `umin vmin umax vmax` (degrees by generator parameters).
        domain: [f64; 4],
    },
}

/// A surface of revolution to build.
#[derive(Debug, Clone, PartialEq)]
pub struct Surf {
    /// Generator.
    pub gen: Gen,
    /// Construction.
    pub build: Build,
    /// Out-of-bounds evaluation (set after construction).
    pub oob: bool,
    /// `bSwapUV`.
    pub swap: bool,
}

/// The upstream member run on the surface.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// The construction state only.
    Build,
    /// `EvaluateSTEP(uv, k, k, ..)`, or `EvaluateSTEPPoint` when `point`.
    EvalStep {
        /// Angle in degrees.
        u: f64,
        /// Generator parameter.
        v: f64,
        /// Derivative order (both directions), at most 3.
        k: usize,
        /// Use `EvaluateSTEPPoint`.
        point: bool,
    },
    /// `ConvertUVFromNURBSToSTEP`.
    ToStep {
        /// NURB u.
        u: f64,
        /// NURB v.
        v: f64,
    },
    /// `CreateIsoParametricCurve`.
    Iso {
        /// `0` = `SM_SP_U`, `1` = `SM_SP_V`.
        param: u8,
        /// Iso parameter.
        iso: f64,
        /// Optional trim domain.
        dom: Option<[f64; 4]>,
    },
    /// `CreateDirectrixAtGeneratorParam`.
    Directrix {
        /// Generator NURB parameter.
        t: f64,
    },
    /// `CreateGeneratorFromAngle`.
    Generator {
        /// Angle in degrees.
        angle: f64,
    },
    /// `TransformPointToStartPlane`.
    StartPlane {
        /// The point.
        p: [f64; 3],
        /// Seam tolerance.
        tol: f64,
        /// `bSnapToSeams`.
        snap: bool,
    },
    /// `operator==` with another surface.
    Eq {
        /// The other surface.
        other: Box<Surf>,
    },
    /// `IntersectWithPlane` with `SmPlane::CreateCanonical(placement)`.
    Plane {
        /// Plane origin.
        origin: [f64; 3],
        /// Plane X axis.
        x: [f64; 3],
        /// Plane Y axis.
        y: [f64; 3],
        /// Plane UV domain.
        dom: [f64; 4],
        /// Optional approximation tolerance.
        tol: Option<f64>,
    },
}

impl Op {
    fn code(&self) -> i32 {
        match self {
            Op::Build => 0,
            Op::EvalStep { .. } => 1,
            Op::ToStep { .. } => 2,
            Op::Iso { .. } => 3,
            Op::Directrix { .. } => 4,
            Op::Generator { .. } => 5,
            Op::StartPlane { .. } => 6,
            Op::Eq { .. } => 7,
            Op::Plane { .. } => 8,
        }
    }
}

const OP_ISO_CIRCLES: i32 = 20;

fn finite(v: &[f64], what: &str) -> Result<(), FfiError> {
    if v.iter().all(|x| x.is_finite()) {
        Ok(())
    } else {
        Err(bad(format!("{what}: non-finite value")))
    }
}

fn len(v: [f64; 3]) -> f64 {
    let [x, y, z] = v;
    (x * x + y * y + z * z).sqrt()
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx]
}

/// Checks two axes are long enough, far from parallel, and of a length whose square
/// does not overflow: unitizing an axis of infinite length gives a zero axis, so a
/// generator circle on it gets no NURB, which `SmSurfOfRevolution`'s constructor then
/// evaluates (`EvaluatePoint` dereferences the NULL NURB).
fn axes(x: [f64; 3], y: [f64; 3], what: &str) -> Result<(), FfiError> {
    finite(&x, what)?;
    finite(&y, what)?;
    let (lx, ly) = (len(x), len(y));
    if !lx.is_finite() || !ly.is_finite() {
        return Err(bad(format!("{what}: axis length overflows")));
    }
    if lx < 1e-6 || ly < 1e-6 || len(cross(x, y)) < 1e-3 * lx * ly {
        return Err(bad(format!("{what}: degenerate or parallel axes")));
    }
    Ok(())
}

/// `SM_EFF_ZERO`.
const EFF_ZERO: f64 = 1e-12;

/// Checks a vector is a unit vector within `SM_EFF_ZERO` on its squared length.
fn unit(v: [f64; 3], what: &str) -> Result<(), FfiError> {
    let [x, y, z] = v;
    if ((x * x + y * y + z * z) - 1.0).abs() > EFF_ZERO {
        return Err(bad(format!("{what}: not a unit vector")));
    }
    Ok(())
}

fn domain(d: [f64; 4], what: &str) -> Result<(), FfiError> {
    finite(&d, what)?;
    let [a, b, c, e] = d;
    if !(a <= c && b <= e) {
        return Err(bad(format!("{what}: domain not ordered")));
    }
    Ok(())
}

fn angular(d0: f64, d1: f64, what: &str) -> Result<(), FfiError> {
    finite(&[d0, d1], what)?;
    if !(d0 < d1 && d0 >= -360.0 && d1 <= 360.0 && d1 - d0 <= 360.0) {
        return Err(bad(format!("{what}: bad angular domain")));
    }
    Ok(())
}

/// Checks a clamped knot vector of degree `p`.
fn check_knots(knots: &[f64], p: usize, what: &str) -> Result<(), FfiError> {
    finite(knots, what)?;
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

/// Checks 3D control points sharing their rational marker.
fn check_points<'a>(
    pts: impl IntoIterator<Item = &'a [f64; 4]>,
    what: &str,
) -> Result<(), FfiError> {
    let mut first: Option<bool> = None;
    for &[x, y, z, w] in pts {
        finite(&[x, y, z], what)?;
        let rational = w != NOW;
        if *first.get_or_insert(rational) != rational {
            return Err(bad(format!("{what}: mixed rationality")));
        }
        if rational && !(w.is_finite() && w > 0.0) {
            return Err(bad(format!("{what}: non-positive weight")));
        }
    }
    if first.is_none() {
        return Err(bad(format!("{what}: no points")));
    }
    Ok(())
}

fn check_bspline(c: &BSplineData) -> Result<(), FfiError> {
    let n = c
        .cpts
        .len()
        .checked_sub(1)
        .ok_or_else(|| bad("curve: no points"))?;
    if c.p == 0 || c.p > n || c.p > MAX_DEGREE || n > MAX_INDEX {
        return Err(bad("curve: need 1 <= p <= n, bounded sizes"));
    }
    if c.knots.len() != n + c.p + 2 {
        return Err(bad("curve: need n + p + 2 knots"));
    }
    check_points(&c.cpts, "curve")?;
    check_knots(&c.knots, c.p, "curve")
}

pub(crate) fn check_surface(s: &SurfaceData) -> Result<(), FfiError> {
    let n = s
        .net
        .len()
        .checked_sub(1)
        .ok_or_else(|| bad("surface: empty net"))?;
    let cols = s.net.first().map_or(0, Vec::len);
    let m = cols
        .checked_sub(1)
        .ok_or_else(|| bad("surface: empty net row"))?;
    if s.net.iter().any(|r| r.len() != cols) {
        return Err(bad("surface: net rows differ in length"));
    }
    if s.p == 0
        || s.p > n
        || s.q == 0
        || s.q > m
        || s.p > MAX_DEGREE
        || s.q > MAX_DEGREE
        || n > MAX_INDEX
        || m > MAX_INDEX
    {
        return Err(bad("surface: need 1 <= p <= n, 1 <= q <= m, bounded sizes"));
    }
    if s.u.len() != n + s.p + 2 || s.v.len() != m + s.q + 2 {
        return Err(bad("surface: knot counts"));
    }
    check_points(s.net.iter().flatten(), "surface")?;
    check_knots(&s.u, s.p, "surface u")?;
    check_knots(&s.v, s.q, "surface v")
}

fn push_gen(o: &mut Vec<f64>, g: &Gen) -> Result<(), FfiError> {
    match g {
        Gen::BSpline(c) => {
            check_bspline(c)?;
            o.push(0.0);
            let n = c.cpts.len() - 1;
            o.extend([n as f64, c.p as f64, (c.knots.len() - 1) as f64]);
            for p in &c.cpts {
                o.extend_from_slice(p);
            }
            o.extend_from_slice(&c.knots);
        }
        Gen::Line { a, b } => {
            finite(a, "line")?;
            finite(b, "line")?;
            let [ax, ay, az] = *a;
            let [bx, by, bz] = *b;
            if len([bx - ax, by - ay, bz - az]) < 1e-6 {
                return Err(bad("line: degenerate"));
            }
            o.push(1.0);
            o.extend_from_slice(a);
            o.extend_from_slice(b);
        }
        Gen::Circle { c, x, y, d, r } => {
            finite(c, "circle")?;
            axes(*x, *y, "circle")?;
            let [d0, d1] = *d;
            angular(d0, d1, "circle")?;
            if !(r.is_finite() && *r > 1e-6) {
                return Err(bad("circle: radius"));
            }
            o.push(2.0);
            o.extend_from_slice(c);
            o.extend_from_slice(x);
            o.extend_from_slice(y);
            o.extend([d0, d1, *r]);
        }
        Gen::Hyperbola { c, x, y, d, a, b } => {
            // (the guard of the surface.surf_of_extrusion_p1 FFI: a bounded STEP domain
            // keeps `cosh` finite, positive semi axes)
            finite(c, "hyperbola")?;
            axes(*x, *y, "hyperbola")?;
            let [d0, d1] = *d;
            finite(&[d0, d1], "hyperbola")?;
            if !(d0 < d1 && d0 >= -10.0 && d1 <= 10.0) {
                return Err(bad("hyperbola: domain"));
            }
            if !(a.is_finite() && *a > 1e-6 && b.is_finite() && *b > 1e-6) {
                return Err(bad("hyperbola: semi axes"));
            }
            o.push(3.0);
            o.extend_from_slice(c);
            o.extend_from_slice(x);
            o.extend_from_slice(y);
            o.extend([d0, d1, *a, *b]);
        }
    }
    Ok(())
}

pub(crate) fn push_surf(o: &mut Vec<f64>, s: &Surf) -> Result<(), FfiError> {
    match &s.build {
        Build::Canonical { point, axis } => {
            finite(point, "axis point")?;
            // any finite axis: `CreateCanonical` unitizes it ignoring the status, so a
            // zero one becomes (0, 0, 1)
            finite(axis, "axis")?;
            o.push(0.0);
            push_gen(o, &s.gen)?;
            o.extend([f64::from(u8::from(s.oob)), f64::from(u8::from(s.swap))]);
            o.extend_from_slice(point);
            o.extend_from_slice(axis);
        }
        Build::Ctor {
            origin,
            x,
            y,
            domain: d,
        } => {
            finite(origin, "origin")?;
            axes(*x, *y, "placement")?;
            // Unit axes only: the constructor passes the Y axis as is to
            // `SmBSplineCurve::IsOnPlane`, whose `smgu_PlanePointDistance` fails for
            // `|y|^2 - 1 > SM_EFF_ZERO` and leaves `bOnPlane` uninitialised (read by the
            // constructor) for a planar generator that is not a line.
            unit(*x, "placement X")?;
            unit(*y, "placement Y")?;
            domain(*d, "surface domain")?;
            let [u0, v0, u1, v1] = *d;
            angular(u0, u1, "surface domain")?;
            // A circle/ellipse generator's v parameter is an angle in degrees;
            // huge v domains hang in C++ (HANG-23 / HANG-1: polar set-up shifts
            // one turn per iteration). Bound them like the generator's angular
            // domain. Hyperbola/parabola generators bound v to their STEP range.
            match &s.gen {
                Gen::Circle { .. } => {
                    angular(v0, v1, "surface domain")?;
                }
                Gen::Hyperbola { .. } if !(v0 < v1 && v0 >= -10.0 && v1 <= 10.0) => {
                    return Err(bad("surface domain: bad hyperbola v domain"));
                }
                _ => {}
            }
            o.push(1.0);
            push_gen(o, &s.gen)?;
            o.extend([f64::from(u8::from(s.oob)), f64::from(u8::from(s.swap))]);
            o.extend_from_slice(origin);
            o.extend_from_slice(x);
            o.extend_from_slice(y);
            o.extend_from_slice(d);
        }
    }
    Ok(())
}

fn push_op(o: &mut Vec<f64>, op: &Op) -> Result<(), FfiError> {
    match op {
        Op::Build => {}
        Op::EvalStep { u, v, k, point } => {
            finite(&[*u, *v], "eval")?;
            if *k > 3 {
                return Err(bad("eval: order above 3"));
            }
            o.extend([*u, *v, *k as f64, f64::from(u8::from(*point))]);
        }
        Op::ToStep { u, v } => {
            finite(&[*u, *v], "to_step")?;
            o.extend([*u, *v]);
        }
        Op::Iso { param, iso, dom } => {
            if *param > 1 {
                return Err(bad("iso: param"));
            }
            finite(&[*iso], "iso")?;
            o.extend([f64::from(*param), *iso, f64::from(u8::from(dom.is_some()))]);
            let d = dom.unwrap_or([0.0, 0.0, 0.0, 0.0]);
            domain(d, "iso domain")?;
            o.extend_from_slice(&d);
        }
        Op::Directrix { t } => {
            finite(&[*t], "directrix")?;
            o.push(*t);
        }
        Op::Generator { angle } => {
            finite(&[*angle], "generator")?;
            o.push(*angle);
        }
        Op::StartPlane { p, tol, snap } => {
            finite(p, "start plane")?;
            finite(&[*tol], "start plane")?;
            o.extend_from_slice(p);
            o.extend([*tol, f64::from(u8::from(*snap))]);
        }
        Op::Eq { other } => push_surf(o, other)?,
        Op::Plane {
            origin,
            x,
            y,
            dom,
            tol,
        } => {
            finite(origin, "plane")?;
            axes(*x, *y, "plane")?;
            domain(*dom, "plane domain")?;
            // the shim reads it into an `SmApproxTol3d`, which takes |tol| (and maps
            // SM_USE_DEFAULT to the zone tolerance), so any finite value is valid
            let t = tol.unwrap_or(0.0);
            if !t.is_finite() {
                return Err(bad("plane: tolerance"));
            }
            o.extend_from_slice(origin);
            o.extend_from_slice(x);
            o.extend_from_slice(y);
            o.extend_from_slice(dom);
            o.extend([f64::from(u8::from(tol.is_some())), t]);
        }
    }
    Ok(())
}

fn call(op: i32, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (built and validated above: well-formed clamped 3D curves and
        // surfaces, bounded counts, finite values, non-degenerate axes, lines, circles
        // and domains, orders up to 3); buf is valid for buf.len() writes and the shim
        // honours the capacity; len is valid.
        unsafe {
            sys::sc_surface_surf_of_revolution_p1_eval(
                op,
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Builds the surface `s` and runs `op` on it; returns the shim's record (layout in
/// the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn surface_op(s: &Surf, op: &Op) -> Result<Vec<f64>, FfiError> {
    let mut input = Vec::new();
    push_surf(&mut input, s)?;
    push_op(&mut input, op)?;
    call(op.code(), &input)
}

/// `sm_TestForIsoCircles(surface, param (0 = u, 1 = v), tol, ang_tol_rad)`; returns the
/// shim's record (layout in the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn iso_circles(
    surface: &SurfaceData,
    param: u8,
    tol: f64,
    ang_tol_rad: f64,
) -> Result<Vec<f64>, FfiError> {
    check_surface(surface)?;
    if param > 1 {
        return Err(bad("iso circles: param"));
    }
    finite(&[tol, ang_tol_rad], "iso circles")?;
    let n = surface.net.len() - 1;
    let m = surface.net.first().map_or(0, Vec::len) - 1;
    let mut input = vec![n as f64, m as f64, surface.p as f64, surface.q as f64];
    for row in &surface.net {
        for p in row {
            input.extend_from_slice(p);
        }
    }
    input.extend_from_slice(&surface.u);
    input.extend_from_slice(&surface.v);
    input.extend([f64::from(param), tol, ang_tol_rad]);
    call(OP_ISO_CIRCLES, &input)
}

/// The surface of revolution as its constructor leaves it (no `MakeNurb`; a
/// [`Build::Ctor`] surface),
/// then the shim's `sc::put_const_make` sequence: has-NURB, the `const`
/// `GetNaturalUVDomain` (which makes the NURB and keeps it), has-NURB, the NURB,
/// `HavePolarConversion`, `EvaluatePoint` at the domain's `(0.3, 0.6)`, `Copy` (status,
/// the copy's has-NURB and NURB, `*copy == *this`, its `EvaluatePoint` at `(0.7, 0.2)`) and
/// `Transform` by a fixed rotation and move (status, has-NURB, NURB, the natural domain,
/// `EvaluatePoint`). The record starts with the construction status.
///
/// # Errors
///
/// Exactly the constructor guards of [`surface_op`], and a canonical build.
pub fn const_make(s: &Surf) -> Result<Vec<f64>, FfiError> {
    if !matches!(s.build, Build::Ctor { .. }) {
        return Err(bad("const_make: constructor build required"));
    }
    let mut input = Vec::new();
    push_surf(&mut input, s)?;
    call(21, &input)
}
