//! FFI wrappers for unit `surface.sphere`.
//!
//! Shim: `harness/cpp/shim/surface_sphere.cpp` (`sc_surface_sphere_eval`).
//! Upstream: `source/SMLib/src/SmSphere.cpp`.
//!
//! [`sphere_op`] builds a sphere from a [`SphereIn`] (`SmSphere::CreateCanonical`, or the
//! constructor followed by `MakeNurb` for the swapped, inside-out, partial and
//! explicit-generator spheres) and runs one [`Op`] on it. The shim's flat record is the
//! construction status (`0` = `SM_SUCCESS`, else the `SmStatus`), then on success the
//! op's record:
//!
//! * `Build`: the state (no status). A state is `is_sphere is_rev is_bspline is_surface
//!   type swap planar inside_out radius O[3] X[3] Y[3] D[4] MAXD[4] have_polar
//!   angle_count [first last] polar_curve gen_curve has_nurb [p q nu nv cpts(4 nu nv, v
//!   fastest) nku KU nkv KV]`; a curve is `has [type step0 step1 has_nurb [p n cpts(4 n)
//!   nk knots] is_ellipse [O[3] X[3] Y[3] rx ry inside_out]]`.
//! * `EvalStep`: status, then `(k+1)^2` vectors (or the point). `ToStep`, `ToNurbs`:
//!   status, `u v`. `Normal`: status, the normal. `Iso`: status, `gap curve` (the shim
//!   answers `SC_ERR_ARG` without status when `SmBSplineSurface::CreateIsoParametricCurve`
//!   fails, since the sphere override then reads an uninitialised curve). `Offset`:
//!   status, `has [rebuild_status] state`. `Rebuild`, `Transform`: status, the state
//!   (also after a failure). `Eq`: the other sphere's construction status, then
//!   `this == other`, `other == this`. `Copy`: status, `state copy == this`. `Closed`:
//!   `closed continuity closed_u closed_v`. `StepInv`: status (also `SM_ERR` for a point
//!   off the surface), `u v location`. `Trim`: status, `n curves`.
//! * the intersections: the other surface's construction status, then (on success) the
//!   status and `more ncurves curves ntypes types ndevs devs nuv1 nuv2 nnull1 nnull2`.
//! * `IsNurb`: `found has_sphere [state]` (the sphere's NURB as a plain B-spline surface,
//!   its homogeneous coordinates scaled and then moved by the given control point
//!   deltas).
//!
//! The wrappers accept only what upstream handles without undefined behaviour: finite
//! values, unit and independent placement axes, angular domains inside `[-360, 360]` at
//! most 360 long (latitudes inside `[-90, 90]`), generator circles with independent axes
//! and a positive radius, spheres built by the constructor with a positive radius, cones
//! with non-negative radii (not both zero) and a positive height, derivative orders up
//! to 3, ordered domains.

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_surface_sphere_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// `SmCircle(c, x, y, [d0, d1], r, 3)`, the optional generator (`pOptCircleNurb`).
#[derive(Debug, Clone, PartialEq)]
pub struct CircleIn {
    /// Centre.
    pub c: [f64; 3],
    /// X axis.
    pub x: [f64; 3],
    /// Y axis.
    pub y: [f64; 3],
    /// Angular domain in degrees.
    pub d: [f64; 2],
    /// Radius.
    pub r: f64,
}

/// How the sphere is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// `SmSphere::CreateCanonical(SmAxis2Placement(origin, x, y), radius)` (the other
    /// fields are ignored).
    Canonical,
    /// `SmSphere(origin, x, y, domain, radius, swap, inside_out, circle)` and `MakeNurb`.
    Ctor,
}

/// A sphere to build.
#[derive(Debug, Clone, PartialEq)]
pub struct SphereIn {
    /// Construction mode.
    pub mode: Mode,
    /// Sphere centre.
    pub origin: [f64; 3],
    /// Longitude start direction.
    pub x: [f64; 3],
    /// Longitude 90 degree direction.
    pub y: [f64; 3],
    /// Radius (negative: inside out, canonical mode only).
    pub radius: f64,
    /// `bSwapUV` (constructor mode).
    pub swap: bool,
    /// `bInsideOut` (constructor mode).
    pub inside_out: bool,
    /// Optional STEP domain `umin vmin umax vmax` in degrees (constructor mode).
    pub dom: Option<[f64; 4]>,
    /// Optional generator (constructor mode).
    pub circle: Option<CircleIn>,
}

/// A NURB domain argument: the surface's natural domain, or explicit bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dom {
    /// `GetNaturalUVDomain()` of the surface.
    Natural,
    /// `umin vmin umax vmax`.
    Explicit([f64; 4]),
}

/// The other operand of the intersections.
#[derive(Debug, Clone, PartialEq)]
pub enum Plane {
    /// `SmPlane(origin, x, y, (su, sv), domain)`.
    Bounded {
        /// Origin.
        origin: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// UV scale.
        scale: [f64; 2],
        /// Domain `umin vmin umax vmax`.
        dom: [f64; 4],
    },
    /// `SmPlane(origin, normal)`.
    Infinite {
        /// Origin.
        origin: [f64; 3],
        /// Normal.
        normal: [f64; 3],
    },
}

/// `SmCone::CreateCanonical(A2P(o, x, y), r0, r1, h)`.
#[derive(Debug, Clone, PartialEq)]
pub struct ConeIn {
    /// Placement origin.
    pub origin: [f64; 3],
    /// Placement X axis.
    pub x: [f64; 3],
    /// Placement Y axis.
    pub y: [f64; 3],
    /// Bottom radius.
    pub r0: f64,
    /// Top radius.
    pub r1: f64,
    /// Height.
    pub h: f64,
}

/// A control point change of the `IsNurb` test surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Perturb {
    /// Row index (skipped by the wrappers when outside the net: use `0..` of the
    /// sphere's net, 4 rows at least).
    pub i: usize,
    /// Column index.
    pub j: usize,
    /// Change of `x`, `y`, `z`.
    pub d: [f64; 3],
}

/// The tolerance and domain arguments of an intersection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IsectArgs {
    /// This sphere's NURB domain.
    pub dom: Dom,
    /// The other surface's NURB domain.
    pub other_dom: Dom,
    /// Optional approximation tolerance.
    pub tol: Option<f64>,
}

/// The upstream member run on the sphere.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// The construction state only.
    Build,
    /// `EvaluateSTEP(uv, k, k, ..)`, or `EvaluateSTEPPoint` when `point`.
    EvalStep {
        /// Longitude in degrees.
        u: f64,
        /// Latitude in degrees.
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
    /// `ConvertUVFromSTEPToNURBS`.
    ToNurbs {
        /// STEP u.
        u: f64,
        /// STEP v.
        v: f64,
    },
    /// `EvaluateNormal` at NURB parameters.
    Normal {
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
    /// `CreateOffsetSurface`, then optionally `RebuildSTEPFromNURBParameters` on the
    /// offset sphere.
    Offset {
        /// Signed offset distance.
        dist: f64,
        /// Approximation tolerance.
        tol: f64,
        /// Rebuild the offset sphere.
        rebuild: bool,
    },
    /// `RebuildSTEPFromNURBParameters`.
    Rebuild,
    /// `Transform(SmAxis2Placement(origin, x, y), scale)`.
    Transform {
        /// Placement origin.
        origin: [f64; 3],
        /// Placement X axis.
        x: [f64; 3],
        /// Placement Y axis.
        y: [f64; 3],
        /// Optional scale.
        scale: Option<[f64; 3]>,
    },
    /// `operator==` with another sphere.
    Eq {
        /// The other sphere.
        other: Box<SphereIn>,
    },
    /// `Copy`.
    Copy,
    /// `IsClosed(dom, param)`, `IsClosedU`, `IsClosedV`.
    Closed {
        /// `0` = `SM_SP_U`, `1` = `SM_SP_V`, `2` = `SM_SP_BOTH`.
        param: u8,
        /// The NURB domain to check.
        dom: Dom,
    },
    /// `STEPInversion`.
    StepInv {
        /// The domain limit: `None` the sphere's STEP domain.
        dom: Option<[f64; 4]>,
        /// The point.
        point: [f64; 3],
        /// Distance tolerance.
        tol: f64,
        /// Optional parameter guess.
        guess: Option<[f64; 2]>,
    },
    /// `CreateTrimmedCircleSegments`.
    Trim {
        /// Circle centre.
        c: [f64; 3],
        /// Circle X axis.
        x: [f64; 3],
        /// Circle Y axis.
        y: [f64; 3],
        /// Circle interval in degrees.
        d: [f64; 2],
        /// Tolerance.
        tol: f64,
    },
    /// `IntersectWithPlane`.
    IsectPlane {
        /// The plane.
        plane: Plane,
        /// Domains and tolerance.
        args: IsectArgs,
    },
    /// `IntersectWithSphere`.
    IsectSphere {
        /// The other sphere.
        other: Box<SphereIn>,
        /// Domains and tolerance.
        args: IsectArgs,
    },
    /// `IntersectWithCone`.
    IsectCone {
        /// The cone.
        cone: ConeIn,
        /// Domains and tolerance.
        args: IsectArgs,
    },
    /// `GlobalSurfaceIntersect` with a plane.
    Global {
        /// The plane.
        plane: Plane,
        /// Domains and tolerance.
        args: IsectArgs,
    },
    /// `IsNurbSurfaceSphere` on the sphere's NURB as a plain B-spline surface.
    IsNurb {
        /// Tolerance scale.
        scale: f64,
        /// Scale of the homogeneous `x`, `y`, `z`.
        net_scale: [f64; 3],
        /// Control point changes.
        perturb: Vec<Perturb>,
    },
}

impl Op {
    fn code(&self) -> i32 {
        match self {
            Op::Build => 0,
            Op::EvalStep { .. } => 1,
            Op::ToStep { .. } => 2,
            Op::ToNurbs { .. } => 3,
            Op::Iso { .. } => 4,
            Op::Offset { .. } => 5,
            Op::Rebuild => 6,
            Op::Transform { .. } => 7,
            Op::Eq { .. } => 8,
            Op::Copy => 9,
            Op::Closed { .. } => 10,
            Op::StepInv { .. } => 11,
            Op::Trim { .. } => 12,
            Op::Normal { .. } => 13,
            Op::IsectPlane { .. } => 14,
            Op::IsectSphere { .. } => 15,
            Op::IsectCone { .. } => 16,
            Op::Global { .. } => 17,
            Op::IsNurb { .. } => 18,
        }
    }
}

fn bad(msg: impl Into<String>) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

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

/// Checks two axes are long enough and far from parallel.
fn axes(x: [f64; 3], y: [f64; 3], what: &str) -> Result<(), FfiError> {
    finite(&x, what)?;
    finite(&y, what)?;
    let (lx, ly) = (len(x), len(y));
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

fn angular(d0: f64, d1: f64, lo: f64, hi: f64, what: &str) -> Result<(), FfiError> {
    finite(&[d0, d1], what)?;
    if !(d0 < d1 && d0 >= lo && d1 <= hi && d1 - d0 <= 360.0) {
        return Err(bad(format!("{what}: bad angular domain")));
    }
    Ok(())
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

fn push_dom(o: &mut Vec<f64>, d: Dom, what: &str) -> Result<(), FfiError> {
    match d {
        Dom::Natural => o.extend([1.0, 0.0, 0.0, 0.0, 0.0]),
        Dom::Explicit(e) => {
            domain(e, what)?;
            o.push(0.0);
            o.extend_from_slice(&e);
        }
    }
    Ok(())
}

fn push_sphere(o: &mut Vec<f64>, s: &SphereIn) -> Result<(), FfiError> {
    finite(&s.origin, "origin")?;
    axes(s.x, s.y, "placement")?;
    unit(s.x, "placement X")?;
    unit(s.y, "placement Y")?;
    if !s.radius.is_finite() {
        return Err(bad("radius: non-finite"));
    }
    let ctor = s.mode == Mode::Ctor;
    if ctor && s.radius <= 1e-6 {
        return Err(bad("radius: not positive"));
    }
    o.push(if ctor { 1.0 } else { 0.0 });
    o.extend_from_slice(&s.origin);
    o.extend_from_slice(&s.x);
    o.extend_from_slice(&s.y);
    o.extend([s.radius, flag(s.swap), flag(s.inside_out)]);
    let d = s.dom.unwrap_or([0.0, -90.0, 360.0, 90.0]);
    if ctor {
        o.push(flag(s.dom.is_some()));
        domain(d, "sphere domain")?;
        let [u0, v0, u1, v1] = d;
        angular(u0, u1, -360.0, 360.0, "sphere domain u")?;
        angular(v0, v1, -90.0, 90.0, "sphere domain v")?;
    } else {
        o.push(0.0);
    }
    o.extend_from_slice(&d);
    let circle = if ctor { s.circle.as_ref() } else { None };
    o.push(flag(circle.is_some()));
    match circle {
        Some(CircleIn { c, x, y, d, r }) => {
            finite(c, "circle")?;
            axes(*x, *y, "circle")?;
            let [d0, d1] = *d;
            angular(d0, d1, -360.0, 360.0, "circle")?;
            if !(r.is_finite() && *r > 1e-6) {
                return Err(bad("circle: radius"));
            }
            o.extend_from_slice(c);
            o.extend_from_slice(x);
            o.extend_from_slice(y);
            o.extend([d0, d1, *r]);
        }
        None => o.extend([0.0; 12]),
    }
    Ok(())
}

fn push_plane(o: &mut Vec<f64>, p: &Plane) -> Result<(), FfiError> {
    match p {
        Plane::Bounded {
            origin,
            x,
            y,
            scale,
            dom,
        } => {
            finite(origin, "plane origin")?;
            axes(*x, *y, "plane")?;
            finite(scale, "plane scale")?;
            if !(scale.iter().all(|s| *s > 1e-6)) {
                return Err(bad("plane: scale"));
            }
            domain(*dom, "plane domain")?;
            o.push(0.0);
            o.extend_from_slice(origin);
            o.extend_from_slice(x);
            o.extend_from_slice(y);
            o.extend_from_slice(scale);
            o.extend_from_slice(dom);
        }
        Plane::Infinite { origin, normal } => {
            finite(origin, "plane origin")?;
            finite(normal, "plane normal")?;
            if len(*normal) < 1e-3 {
                return Err(bad("plane: normal"));
            }
            o.push(1.0);
            o.extend_from_slice(origin);
            o.extend_from_slice(normal);
        }
    }
    Ok(())
}

fn push_args(o: &mut Vec<f64>, a: &IsectArgs) -> Result<(), FfiError> {
    push_dom(o, a.dom, "domain")?;
    push_dom(o, a.other_dom, "other domain")?;
    let tol = a.tol.unwrap_or(0.0);
    if !(tol.is_finite() && tol >= 0.0) {
        return Err(bad("tolerance"));
    }
    o.extend([flag(a.tol.is_some()), tol]);
    Ok(())
}

fn push_op(o: &mut Vec<f64>, op: &Op) -> Result<(), FfiError> {
    match op {
        Op::Build | Op::Rebuild | Op::Copy => {}
        Op::EvalStep { u, v, k, point } => {
            finite(&[*u, *v], "eval")?;
            if *k > 3 {
                return Err(bad("eval: order above 3"));
            }
            o.extend([*u, *v, *k as f64, flag(*point)]);
        }
        Op::ToStep { u, v } | Op::ToNurbs { u, v } | Op::Normal { u, v } => {
            finite(&[*u, *v], "convert")?;
            o.extend([*u, *v]);
        }
        Op::Iso { param, iso, dom } => {
            if *param > 1 {
                return Err(bad("iso: param"));
            }
            finite(&[*iso], "iso")?;
            o.extend([f64::from(*param), *iso, flag(dom.is_some())]);
            let d = dom.unwrap_or([0.0, 0.0, 0.0, 0.0]);
            domain(d, "iso domain")?;
            o.extend_from_slice(&d);
        }
        Op::Offset { dist, tol, rebuild } => {
            finite(&[*dist, *tol], "offset")?;
            o.extend([*dist, *tol, flag(*rebuild)]);
        }
        Op::Transform {
            origin,
            x,
            y,
            scale,
        } => {
            finite(origin, "transform")?;
            axes(*x, *y, "transform")?;
            let s = scale.unwrap_or([1.0, 1.0, 1.0]);
            finite(&s, "transform scale")?;
            o.extend_from_slice(origin);
            o.extend_from_slice(x);
            o.extend_from_slice(y);
            o.push(flag(scale.is_some()));
            o.extend_from_slice(&s);
        }
        Op::Eq { other } => push_sphere(o, other)?,
        Op::Closed { param, dom } => {
            if *param > 2 {
                return Err(bad("closed: param"));
            }
            o.push(f64::from(*param));
            push_dom(o, *dom, "closed domain")?;
        }
        Op::StepInv {
            dom,
            point,
            tol,
            guess,
        } => {
            o.push(flag(dom.is_none()));
            let d = dom.unwrap_or([0.0; 4]);
            domain(d, "step domain")?;
            o.extend_from_slice(&d);
            finite(point, "point")?;
            if !(tol.is_finite() && *tol >= 0.0) {
                return Err(bad("tolerance"));
            }
            let g = guess.unwrap_or([0.0, 0.0]);
            finite(&g, "guess")?;
            o.extend_from_slice(point);
            o.extend([*tol, flag(guess.is_some())]);
            o.extend_from_slice(&g);
        }
        Op::Trim { c, x, y, d, tol } => {
            finite(c, "circle centre")?;
            axes(*x, *y, "circle")?;
            unit(*x, "circle X")?;
            unit(*y, "circle Y")?;
            angular(d[0], d[1], -360.0, 360.0, "circle")?;
            if !(tol.is_finite() && *tol >= 0.0) {
                return Err(bad("tolerance"));
            }
            o.extend_from_slice(c);
            o.extend_from_slice(x);
            o.extend_from_slice(y);
            o.extend([d[0], d[1], *tol]);
        }
        Op::IsectPlane { plane, args } | Op::Global { plane, args } => {
            push_plane(o, plane)?;
            push_args(o, args)?;
        }
        Op::IsectSphere { other, args } => {
            push_sphere(o, other)?;
            push_args(o, args)?;
        }
        Op::IsectCone { cone, args } => {
            finite(&cone.origin, "cone origin")?;
            axes(cone.x, cone.y, "cone")?;
            finite(&[cone.r0, cone.r1, cone.h], "cone")?;
            if !(cone.r0 >= 0.0 && cone.r1 >= 0.0 && cone.r0 + cone.r1 > 1e-6 && cone.h > 1e-6) {
                return Err(bad("cone: radii or height"));
            }
            o.extend_from_slice(&cone.origin);
            o.extend_from_slice(&cone.x);
            o.extend_from_slice(&cone.y);
            o.extend([cone.r0, cone.r1, cone.h]);
            push_args(o, args)?;
        }
        Op::IsNurb {
            scale,
            net_scale,
            perturb,
        } => {
            finite(&[*scale], "scale")?;
            if *scale <= 0.0 {
                return Err(bad("scale"));
            }
            finite(net_scale, "net scale")?;
            if perturb.len() > 8 {
                return Err(bad("too many control point changes"));
            }
            o.push(*scale);
            o.extend_from_slice(net_scale);
            o.push(perturb.len() as f64);
            for p in perturb {
                finite(&p.d, "control point change")?;
                o.extend([p.i as f64, p.j as f64]);
                o.extend_from_slice(&p.d);
            }
        }
    }
    Ok(())
}

/// Builds the sphere `s` and runs `op` on it; returns the shim's record (layout in the
/// module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn sphere_op(s: &SphereIn, op: &Op) -> Result<Vec<f64>, FfiError> {
    let mut input = Vec::new();
    push_sphere(&mut input, s)?;
    push_op(&mut input, op)?;
    let code = op.code();
    call_with_buffer(8192, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `code` (built and validated above: finite values, unit independent
        // placement axes, non-degenerate circles, ordered angular domains, orders up to
        // 3); buf is valid for buf.len() writes and the shim honours the capacity; len
        // is valid.
        unsafe {
            sys::sc_surface_sphere_eval(
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
