//! FFI wrappers for unit `surface.plane_p2`.
//!
//! Shim: `harness/cpp/shim/surface_plane_p2.cpp` (`sc_surface_plane_p2_run`). Upstream:
//! `source/SMLib/src/SmPlane.cpp`, `source/SMLib/inc/SmPlane.h` (`SmPlane`).
//!
//! [`run`] runs one [`Op`] on freshly built objects and returns its record. Statuses are
//! `0` (`SM_SUCCESS`) or the `SmStatus`.
//!
//! A plane state is `type O[3] X[3] Y[3] Z[3] domain[4] scale[2] oob knot_type form` then
//! the plane's own NURB `has [n m p q r s cpts(x y z w) u v]`. A curve is `has` then
//! `type dim [P[3] V[3] dom[2] scale inside_out]` (lines) or `[C[3] X[3] Y[3] rx ry
//! dom[2] inside_out]` (ellipses and circles), then `knot_type form has [n p m cpts(x y z
//! w) knots]` and the natural interval. An intersection is `count curves`, `count`
//! UV-curve presence flags (twice), `count types`, `count deviations`.
//!
//! | [`Op`]        | upstream                                                | record |
//! |---------------|---------------------------------------------------------|--------|
//! | `Offset`      | `CreateOffsetSurface(ctx, dist, 0, s)`                  | `status [state]` |
//! | `Transform`   | `Transform(SmAxis2Placement(o, x, y), scale)`           | `status state` |
//! | `TrimDomain`  | `TrimWithDomain(dom)`                                   | `status state` |
//! | `IsNurbPlane` | `IsNurbSurfacePlane(ctx, bss, p, scale)`                | `0 result has [state]` |
//! | `Intersect`   | `IntersectWithPlane(ctx, da, b, db, {1,1}, tol, NULL, ..)` | `status [needs_more intersection]` |
//! | `Global`      | `GlobalSurfaceIntersect(ctx, da, b, db, {1,1}, tol, NULL, ..)` | `status [intersection]` |
//! | `TrimCurve`   | `TrimCurveToPlaneDomain(dom, curve, tol, out)`          | `status [count curves]` |
//! | `Line`        | `MakeNewPlaneLine(ctx, dom, p0, p1, tol, found)`        | `0 found curve` |
//! | `Ellipse`     | `MakeNewPlaneEllipse(ctx, dom, c, rx, ry, x, y, ivl, tol, out)` | `status [count curves]` |
//! | `ReadDb`      | `ReadFromDB(SmPlane_TYPE, db, ctx, s, version)`         | `status [state]` |
//!
//! [`run`] accepts only finite inputs of magnitude at most [`MAX_COORD`], well-formed
//! clamped B-splines (at most [`MAX_INDEX`] + 1 control points per direction, degree at
//! most [`MAX_DEGREE`], positive weights or none), segments with distinct end points,
//! planes with a non-zero normal and ellipses and circles without a zero radius (the
//! upstream construction crashes). A bounded plane with an empty or inverted domain has
//! no NURB (`MakeNurb` fails): `Transform` of one, and a plane/plane intersection with a
//! bounded one, dereference the NULL NURB upstream and are rejected.

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_surface_plane_p2_run(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Largest control point index `n` (curves: 32, surfaces: 16 per direction).
pub const MAX_INDEX: usize = 16;
/// Largest degree.
pub const MAX_DEGREE: usize = 8;
/// Largest input magnitude.
pub const MAX_COORD: f64 = 1e12;
/// Most database bytes.
pub const MAX_BYTES: usize = 1 << 20;
/// `NL_NOW`: the weight of a non-rational control point.
pub const NOW: f64 = -f64::MAX;

/// How a plane is built, and whether its NURB is made right away
/// (`GetOrCreateGwNurbPointer`).
#[derive(Debug, Clone, PartialEq)]
pub struct PlaneIn {
    /// The constructor.
    pub make: Make,
    /// Make the NURB after construction.
    pub nurb: bool,
}

/// A plane constructor.
#[derive(Debug, Clone, PartialEq)]
pub enum Make {
    /// `SmPlane(o, x, y, SmVector2d(scale), SmExtent2d(domain), ctx)`.
    Bounded {
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
    /// `SmPlane(o, normal, ctx)`.
    Normal {
        /// Origin.
        o: [f64; 3],
        /// Normal.
        n: [f64; 3],
    },
    /// `SmPlane::CreateCanonical(ctx, SmAxis2Placement(o, x, y), p)`.
    Canonical {
        /// Origin.
        o: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
    },
}

/// A clamped B-spline curve: `SmBSplineCurve(dim, n, m, p, Pw, U)`.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveData {
    /// Image dimension, 2 or 3.
    pub dim: u32,
    /// Degree.
    pub p: usize,
    /// Control points as stored (`x y z w`, `w` = [`NOW`] when non-rational).
    pub cpts: Vec<[f64; 4]>,
    /// Knots (`n + p + 2` of them).
    pub knots: Vec<f64>,
}

/// A curve operand.
#[derive(Debug, Clone, PartialEq)]
pub enum CurveIn {
    /// `SmLine::CreateLineSegment(ctx, 3, a, b, l)`.
    Segment([f64; 3], [f64; 3]),
    /// A B-spline curve.
    BSpline(CurveData),
    /// `SmEllipse(c, x, y, SmExtent1d(d0, d1), rx, ry, 3, ctx)`.
    Ellipse {
        /// Centre.
        c: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// Angular domain (degrees).
        dom: [f64; 2],
        /// Radii.
        r: [f64; 2],
    },
    /// `SmCircle(c, x, y, SmExtent1d(d0, d1), r, 3, ctx)`.
    Circle {
        /// Centre.
        c: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// Angular domain (degrees).
        dom: [f64; 2],
        /// Radius.
        r: f64,
    },
}

/// A clamped B-spline surface: `SmBSplineSurface(gw_SURFACE*, FALSE, ctx)`.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceData {
    /// u degree.
    pub p: usize,
    /// v degree.
    pub q: usize,
    /// Control net, `net[i][j]` (`x y z w`).
    pub net: Vec<Vec<[f64; 4]>>,
    /// u knots.
    pub u: Vec<f64>,
    /// v knots.
    pub v: Vec<f64>,
}

/// The other surface of an intersection.
#[derive(Debug, Clone, PartialEq)]
pub enum OtherIn {
    /// The first plane itself.
    Same,
    /// Another plane.
    Plane(PlaneIn),
    /// A B-spline surface.
    BSpline(SurfaceData),
    /// A real `SmCone` (`CreateCanonical(origin, bot, top, height)`) or, with
    /// `cylinder`, `SmCylinder` (`CreateCanonical(origin, bot)`).
    Cone {
        /// Placement origin, X and Y axes.
        frame: [[f64; 3]; 3],
        /// `SmCylinder` instead of `SmCone`.
        cylinder: bool,
        /// Bottom radius (the cylinder's radius).
        bot: f64,
        /// Top radius.
        top: f64,
        /// Height (`0`: infinite cone, semi-angle `top` degrees).
        height: f64,
        /// Intersect over the cone's natural domain instead of `db`.
        natural: bool,
    },
}

/// One op.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// `CreateOffsetSurface`.
    Offset {
        /// The plane.
        a: PlaneIn,
        /// Signed distance.
        dist: f64,
    },
    /// `Transform`.
    Transform {
        /// The plane.
        a: PlaneIn,
        /// Placement origin, X and Y axes.
        frame: [[f64; 3]; 3],
        /// Optional scale.
        scale: Option<[f64; 3]>,
    },
    /// `TrimWithDomain`.
    TrimDomain {
        /// The plane.
        a: PlaneIn,
        /// Domain.
        dom: [f64; 4],
    },
    /// `IsNurbSurfacePlane`.
    IsNurbPlane {
        /// The surface.
        s: SurfaceData,
        /// Tolerance scale.
        scale: f64,
    },
    /// `IntersectWithPlane` (`global == false`) or `GlobalSurfaceIntersect`.
    Intersect {
        /// `GlobalSurfaceIntersect`.
        global: bool,
        /// This plane.
        a: PlaneIn,
        /// Its domain.
        da: [f64; 4],
        /// The other surface (a plane for `IntersectWithPlane`).
        b: OtherIn,
        /// Its domain.
        db: [f64; 4],
        /// Approximation tolerance.
        tol: Option<f64>,
    },
    /// `TrimCurveToPlaneDomain`.
    TrimCurve {
        /// The plane.
        a: PlaneIn,
        /// Domain.
        dom: [f64; 4],
        /// The curve.
        curve: CurveIn,
        /// Tolerance.
        tol: f64,
    },
    /// `MakeNewPlaneLine`.
    Line {
        /// The plane.
        a: PlaneIn,
        /// Domain.
        dom: [f64; 4],
        /// End points.
        p: [[f64; 3]; 2],
        /// Tolerance.
        tol: f64,
    },
    /// `MakeNewPlaneEllipse`.
    Ellipse {
        /// The plane.
        a: PlaneIn,
        /// Domain.
        dom: [f64; 4],
        /// Centre.
        c: [f64; 3],
        /// Radii.
        r: [f64; 2],
        /// X and Y axes.
        axes: [[f64; 3]; 2],
        /// Angular interval (degrees).
        ivl: [f64; 2],
        /// Tolerance.
        tol: f64,
    },
    /// `ReadFromDB`.
    ReadDb {
        /// Binary file.
        binary: bool,
        /// Database version.
        version: u32,
        /// The stream bytes.
        bytes: Vec<u8>,
    },
}

fn finite(x: f64) -> Result<f64, FfiError> {
    if x.is_finite() && x.abs() <= MAX_COORD {
        Ok(x)
    } else {
        Err(bad("non-finite or huge value"))
    }
}

fn check_knots(knots: &[f64], n1: usize, p: usize) -> Result<(), FfiError> {
    if knots.len() != n1 + p + 1 {
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

/// `SM_IS_INFINITE`: the unbounded parameter markers.
fn is_infinite_param(a: f64) -> bool {
    a == 1_234_567.0 || a == -1_234_567.0 || a == 1111.111 || a == -1111.111
}

/// A plane `SmPlane::MakeNurb` cannot make the NURB of: a bounded construction whose
/// domain is empty or inverted in u or v (`N_IgesValidate` needs `min < max`; NaN is
/// rejected by the finite check), so
/// `GetOrCreateGwNurbPointer` returns NULL.
fn nurb_less(p: &PlaneIn) -> bool {
    match &p.make {
        Make::Bounded { domain, .. } => {
            let [u0, v0, u1, v1] = *domain;
            u0 >= u1 || v0 >= v1
        }
        Make::Normal { .. } | Make::Canonical { .. } => false,
    }
}

/// `SmPlane::IsBounded` (no domain value is an `SM_IS_INFINITE` marker) for a plane
/// whose NURB cannot be made.
fn bounded_nurb_less(p: &PlaneIn) -> bool {
    match &p.make {
        Make::Bounded { domain, .. } => {
            nurb_less(p) && !domain.iter().any(|&d| is_infinite_param(d))
        }
        Make::Normal { .. } | Make::Canonical { .. } => false,
    }
}

fn len_sq(v: &[f64; 3]) -> f64 {
    v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
}

struct Enc(Vec<f64>);

impl Enc {
    fn num(&mut self, x: f64) -> Result<(), FfiError> {
        self.0.push(finite(x)?);
        Ok(())
    }
    fn int(&mut self, x: usize) {
        self.0.push(x as f64);
    }
    fn flag(&mut self, b: bool) {
        self.0.push(if b { 1.0 } else { 0.0 });
    }
    fn nums(&mut self, v: &[f64]) -> Result<(), FfiError> {
        v.iter().try_for_each(|&x| self.num(x))
    }
    fn cpt(&mut self, c: &[f64; 4]) -> Result<(), FfiError> {
        self.nums(&c[..3])?;
        // the weight was checked (NL_NOW or positive)
        self.0.push(c[3]);
        Ok(())
    }

    fn plane(&mut self, p: &PlaneIn) -> Result<(), FfiError> {
        match &p.make {
            Make::Bounded {
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
                self.nums(domain)?;
            }
            Make::Normal { o, n } => {
                // SmVector3d::MakeUnitOrthoVectors fails for |n|^2 < SM_EFF_ZERO_SQ and the
                // constructor then reads uninitialised axes (SE ignores the status)
                if len_sq(n) < 1.0e-24 {
                    return Err(bad("zero plane normal"));
                }
                self.int(1);
                self.nums(o)?;
                self.nums(n)?;
            }
            Make::Canonical { o, x, y } => {
                self.int(2);
                self.nums(o)?;
                self.nums(x)?;
                self.nums(y)?;
            }
        }
        self.flag(p.nurb);
        Ok(())
    }

    fn curve(&mut self, c: &CurveIn) -> Result<(), FfiError> {
        match c {
            CurveIn::Segment(a, b) => {
                if a == b {
                    return Err(bad("degenerate segment"));
                }
                self.int(0);
                self.nums(a)?;
                self.nums(b)
            }
            CurveIn::BSpline(d) => {
                let n1 = d.cpts.len();
                if !(2..=2 * MAX_INDEX + 1).contains(&n1)
                    || !(1..=MAX_DEGREE).contains(&d.p)
                    || d.p >= n1
                    || (d.dim != 2 && d.dim != 3)
                {
                    return Err(bad("B-spline curve size, degree or dimension"));
                }
                check_knots(&d.knots, n1, d.p)?;
                check_weights(d.cpts.iter())?;
                self.int(1);
                self.int(d.dim as usize);
                self.int(n1 - 1);
                self.int(d.p);
                self.int(d.knots.len() - 1);
                for c in &d.cpts {
                    self.cpt(c)?;
                }
                self.nums(&d.knots)
            }
            CurveIn::Ellipse { c, x, y, dom, r } => {
                Self::check_radii(r)?;
                self.int(2);
                self.nums(c)?;
                self.nums(x)?;
                self.nums(y)?;
                self.nums(dom)?;
                self.nums(r)
            }
            CurveIn::Circle { c, x, y, dom, r } => {
                Self::check_radii(&[*r])?;
                self.int(3);
                self.nums(c)?;
                self.nums(x)?;
                self.nums(y)?;
                self.nums(dom)?;
                self.num(*r)
            }
        }
    }

    /// A zero radius crashes the ellipse construction upstream.
    fn check_radii(r: &[f64]) -> Result<(), FfiError> {
        if r.contains(&0.0) {
            return Err(bad("zero ellipse radius"));
        }
        Ok(())
    }

    fn surface(&mut self, d: &SurfaceData) -> Result<(), FfiError> {
        let n1 = d.net.len();
        let m1 = d.net.first().map_or(0, Vec::len);
        if !(2..=MAX_INDEX + 1).contains(&n1)
            || !(2..=MAX_INDEX + 1).contains(&m1)
            || d.net.iter().any(|r| r.len() != m1)
            || !(1..=MAX_DEGREE).contains(&d.p)
            || !(1..=MAX_DEGREE).contains(&d.q)
            || d.p >= n1
            || d.q >= m1
        {
            return Err(bad("B-spline surface size or degree"));
        }
        check_knots(&d.u, n1, d.p)?;
        check_knots(&d.v, m1, d.q)?;
        check_weights(d.net.iter().flatten())?;
        self.int(n1 - 1);
        self.int(m1 - 1);
        self.int(d.p);
        self.int(d.q);
        self.int(d.u.len() - 1);
        self.int(d.v.len() - 1);
        for c in d.net.iter().flatten() {
            self.cpt(c)?;
        }
        self.nums(&d.u)?;
        self.nums(&d.v)
    }

    fn opt(&mut self, t: Option<f64>) -> Result<(), FfiError> {
        self.flag(t.is_some());
        self.num(t.unwrap_or(0.0))
    }

    fn op(&mut self, op: &Op) -> Result<(), FfiError> {
        match op {
            Op::Offset { a, dist } => {
                self.int(1);
                self.plane(a)?;
                self.num(*dist)
            }
            Op::Transform { a, frame, scale } => {
                // SmBSplineSurface::Transform hands GetOrCreateGwNurbPointer() to
                // N_SrfTransform unchecked: NULL for a plane without a NURB
                if nurb_less(a) {
                    return Err(bad("transform of a plane whose NURB cannot be made"));
                }
                self.int(2);
                self.plane(a)?;
                for v in frame {
                    self.nums(v)?;
                }
                self.flag(scale.is_some());
                self.nums(&scale.unwrap_or([1.0; 3]))
            }
            Op::TrimDomain { a, dom } => {
                self.int(3);
                self.plane(a)?;
                self.nums(dom)
            }
            Op::IsNurbPlane { s, scale } => {
                self.int(4);
                self.surface(s)?;
                self.num(*scale)
            }
            Op::Intersect {
                global,
                a,
                da,
                b,
                db,
                tol,
            } => {
                // IntersectWithPlane of non-parallel planes boxes each bounded plane
                // through CalculateBoundingBox, which reads GetOrCreateGwNurbPointer()->net
                // unchecked (parallel planes without a NURB return first; rejected too)
                if let OtherIn::Plane(p) = b {
                    if bounded_nurb_less(a) || bounded_nurb_less(p) {
                        return Err(bad("bounded plane whose NURB cannot be made"));
                    }
                }
                self.int(if *global { 6 } else { 5 });
                self.plane(a)?;
                self.nums(da)?;
                match b {
                    OtherIn::Same => self.flag(true),
                    OtherIn::Plane(p) => {
                        self.flag(false);
                        self.int(0);
                        self.plane(p)?;
                    }
                    OtherIn::BSpline(s) => {
                        if !*global {
                            return Err(bad("IntersectWithPlane needs a plane"));
                        }
                        self.flag(false);
                        self.int(1);
                        self.surface(s)?;
                    }
                    OtherIn::Cone {
                        frame,
                        cylinder,
                        bot,
                        top,
                        height,
                        natural,
                    } => {
                        if !*global {
                            return Err(bad("IntersectWithPlane needs a plane"));
                        }
                        // a zero or parallel placement axis makes a degenerate frame
                        let [_, x, y] = frame;
                        let c = [
                            x[1] * y[2] - x[2] * y[1],
                            x[2] * y[0] - x[0] * y[2],
                            x[0] * y[1] - x[1] * y[0],
                        ];
                        if len_sq(&c).partial_cmp(&(1e-12 * len_sq(x) * len_sq(y))) != Some(core::cmp::Ordering::Greater) {
                            return Err(bad("cone placement axes zero or parallel"));
                        }
                        self.flag(false);
                        self.int(2);
                        for v in frame {
                            self.nums(v)?;
                        }
                        self.flag(*cylinder);
                        self.nums(&[*bot, *top, *height])?;
                        self.flag(*natural);
                    }
                }
                self.nums(db)?;
                self.opt(*tol)
            }
            Op::TrimCurve { a, dom, curve, tol } => {
                self.int(7);
                self.plane(a)?;
                self.nums(dom)?;
                self.curve(curve)?;
                self.num(*tol)
            }
            Op::Line { a, dom, p, tol } => {
                self.int(8);
                self.plane(a)?;
                self.nums(dom)?;
                self.nums(&p[0])?;
                self.nums(&p[1])?;
                self.num(*tol)
            }
            Op::Ellipse {
                a,
                dom,
                c,
                r,
                axes,
                ivl,
                tol,
            } => {
                self.int(9);
                self.plane(a)?;
                self.nums(dom)?;
                self.nums(c)?;
                self.nums(r)?;
                self.nums(&axes[0])?;
                self.nums(&axes[1])?;
                self.nums(ivl)?;
                self.num(*tol)
            }
            Op::ReadDb {
                binary,
                version,
                bytes,
            } => {
                if *version > 100 || bytes.len() > MAX_BYTES {
                    return Err(bad("version or byte count"));
                }
                self.int(10);
                self.flag(*binary);
                self.int(*version as usize);
                self.int(bytes.len());
                for &b in bytes {
                    self.int(usize::from(b));
                }
                Ok(())
            }
        }
    }
}

/// The shim program of `op`.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for inputs [`run`] does not accept.
pub fn encode(op: &Op) -> Result<Vec<f64>, FfiError> {
    let mut e = Enc(Vec::new());
    e.op(op)?;
    Ok(e.0)
}

/// Runs `op` through the shim and returns its record (module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for inputs [`run`] does not accept; the shim's status
/// otherwise.
pub fn run(op: &Op) -> Result<Vec<f64>, FfiError> {
    let input = encode(op)?;
    call_with_buffer(256, |buf, len| {
        // SAFETY: input valid for input.len() reads; buf valid for buf.len() writes; len valid.
        unsafe {
            sys::sc_surface_plane_p2_run(
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
