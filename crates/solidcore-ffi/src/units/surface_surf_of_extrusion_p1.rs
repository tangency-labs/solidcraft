//! FFI wrappers for unit `surface.surf_of_extrusion_p1`.
//!
//! Shim: `harness/cpp/shim/surface_surf_of_extrusion_p1.cpp`
//! (`sc_surface_surf_of_extrusion_p1_eval`).
//! Upstream: `source/SMLib/src/SmSurfOfExtrusion.cpp`.
//!
//! [`surface_op`] builds a surface of extrusion from a [`Surf`] and runs one [`Op`] on
//! it. It returns the shim's flat record: the construction status (`0` =
//! `SM_SUCCESS`, else the `SmStatus`), then on success the op's record: its status and,
//! on success, its outputs.
//!
//! * A state is `is_ext is_bspline is_surface type swap oob orient O[3] E[3] CP[3]
//!   CV[3] D[4] lin0 lin1 lin_param gen0 gen1 gen_param MAXD[4] canon_is_gen
//!   canon_vec[3] gen_curve has_nurb [p q nu nv cpts(4 nu nv, v fastest) nku KU nkv
//!   KV]`, a curve `has [type step0 step1 has_nurb [p n cpts(4 n) nk knots]]`.
//! * `Build`: the state (no status). `EvalStep`: `(k+1)^2` vectors (or the point).
//!   `ToNurbs` / `ToStep`: `u v`. `Adjust`, `Update`, `Reparam`, `Transform`: the
//!   state. `Directrix`: a curve. `DropFast`: `n` then per solution `type nvars nobjs
//!   names_surface value u v`. `StepInv`: `u v location`. `Eq`: the other surface's
//!   construction status then (on success) `this == other`, `other == this`. `Copy`:
//!   `this == copy` then the copy's state. `Generators`: `more ncurves curves ntypes
//!   types`.
//!
//! The wrappers accept only what upstream handles without undefined behaviour: clamped
//! 3D B-spline generators (`1 <= p <= n`, degrees at most [`MAX_DEGREE`], finite data,
//! positive weights or all non-rational, non-decreasing knots with end multiplicity
//! `p + 1` and interior multiplicity at most `p`), non-degenerate lines, circles and
//! ellipses with positive radii and (ellipses, HANG-23: a far centre with a tiny radius
//! spins the polar set-up) centres within ±1e8, independent axes and an angular domain inside `[-360,
//! 360]` at most 360 long, hyperbolas (STEP domain inside `[-10, 10]`) and parabolas
//! (inside `[-1000, 1000]`) with positive semi axes / focal distance at most
//! [`MAX_CONIC`] and a centre inside `[-MAX_CONIC, MAX_CONIC]`, independent axes (whose
//! squared lengths do not overflow) and an ordered domain, finite vectors and points, constructor domains with `u0 <
//! u1` and `v0 < v1` (`MakeNurb` builds the V knots `{v0, v0, v1, v1}`), edit domains
//! ordered with positive extents, independent placement axes, finite parameters and
//! tolerances, derivative orders up to [`MAX_ORDER`] (above 5 upstream fails before
//! writing) except order 5 of a B-spline generator (upstream's `EvaluateSTEP` lets it
//! through to `sm_CrvDerivs`, which overruns its `NL_MAXDER = 4` derivative table), `DropPointFast` operations `SM_SO_INTERSECT`, `SM_SO_MINIMIZE`,
//! `SM_SO_MAXIMIZE`, `SM_SO_NORMALIZE` (the ones it documents) and generator
//! intersections with a finite other vector and ordered interval.

use crate::validate::{bad, NOW};
use crate::{call_with_buffer, FfiError};

/// Largest control point index accepted.
pub const MAX_INDEX: usize = 256;
/// Largest NURB degree accepted.
pub const MAX_DEGREE: usize = 9;
/// Largest STEP derivative order accepted.
pub const MAX_ORDER: usize = 7;

mod sys {
    extern "C" {
        pub fn sc_surface_surf_of_extrusion_p1_eval(
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
    /// `SmLine(a, b, 3)` with its NURB left lazy.
    LazyLine {
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
    /// `SmEllipse(c, x, y, [d0, d1], rx, ry, 3)`.
    Ellipse {
        /// Centre.
        c: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// Angular domain in degrees.
        d: [f64; 2],
        /// Radius along X.
        rx: f64,
        /// Radius along Y.
        ry: f64,
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
    /// `SmParabola(c, x, y, [d0, d1], f, 3)`.
    Parabola {
        /// Apex.
        c: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// STEP domain (`|t| <= 1000`).
        d: [f64; 2],
        /// Focal distance.
        f: f64,
    },
}

/// How the surface is made.
#[derive(Debug, Clone, PartialEq)]
pub enum Build {
    /// `CreateCanonical(gen, vec)` (never swapped).
    Canonical {
        /// Extrusion vector.
        vec: [f64; 3],
    },
    /// The finite constructor `(gen, origin, vec, domain, swap)` then `MakeNurb`.
    Ctor {
        /// Origin (a point on the generator).
        origin: [f64; 3],
        /// Extrusion vector.
        vec: [f64; 3],
        /// STEP domain `umin vmin umax vmax`.
        domain: [f64; 4],
    },
    /// The infinite constructor `(gen, vec, swap)` then `MakeNurb`.
    Infinite {
        /// Extrusion vector.
        vec: [f64; 3],
    },
}

/// A surface of extrusion to build.
#[derive(Debug, Clone, PartialEq)]
pub struct Surf {
    /// Generator.
    pub gen: Gen,
    /// Construction.
    pub build: Build,
    /// Out-of-bounds evaluation (set after construction).
    pub oob: bool,
    /// `bSwapUV` (not used by `CreateCanonical`).
    pub swap: bool,
}

/// The upstream member run on the surface.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// The construction state only.
    Build,
    /// `EvaluateSTEP(uv, k, k, ..)`, or `EvaluateSTEPPoint` when `point`.
    EvalStep {
        /// Generator parameter.
        u: f64,
        /// Extrusion parameter.
        v: f64,
        /// Derivative order (both directions).
        k: usize,
        /// Use `EvaluateSTEPPoint`.
        point: bool,
    },
    /// `ConvertUVFromSTEPToNURBS`.
    ToNurbs {
        /// STEP u.
        u: f64,
        /// STEP v.
        v: f64,
    },
    /// `ConvertUVFromNURBSToSTEP`.
    ToStep {
        /// NURB u.
        u: f64,
        /// NURB v.
        v: f64,
    },
    /// `AdjustSTEPUVDomain`.
    Adjust {
        /// New STEP domain.
        dom: [f64; 4],
    },
    /// `UpdateAnalyticalDomain`.
    Update {
        /// New NURB domain.
        dom: [f64; 4],
    },
    /// `Reparameterize`.
    Reparam {
        /// New NURB domain.
        dom: [f64; 4],
    },
    /// `Transform(SmAxis2Placement(origin, x, y), scale)`.
    Transform {
        /// Frame origin.
        origin: [f64; 3],
        /// Frame X axis.
        x: [f64; 3],
        /// Frame Y axis.
        y: [f64; 3],
        /// Optional scale.
        scale: Option<[f64; 3]>,
    },
    /// `CreateDirectrixFromPoint`.
    Directrix {
        /// The point.
        p: [f64; 3],
        /// Tolerance.
        tol: f64,
    },
    /// `DropPointFast`.
    DropFast {
        /// Search domain (NURB).
        dom: [f64; 4],
        /// `SmSolverOperationType` value.
        op: u8,
        /// Test point.
        p: [f64; 3],
        /// Optional in-pointing vector.
        vec: Option<[f64; 3]>,
        /// Distance tolerance.
        tol: f64,
        /// Optional target distance.
        target: Option<f64>,
        /// `0` = `SM_SR_SINGLE`, `1` = `SM_SR_ALL`.
        requested: u8,
    },
    /// `STEPInversion`.
    StepInv {
        /// STEP domain.
        dom: [f64; 4],
        /// The point.
        p: [f64; 3],
        /// Tolerance.
        tol: f64,
        /// Optional guess.
        guess: Option<[f64; 2]>,
    },
    /// `operator==` with another surface.
    Eq {
        /// The other surface.
        other: Box<Surf>,
    },
    /// `Copy`, then `operator==` and the copy's state.
    Copy,
    /// `sm_IntersectGenerators(this, tol, other, vec, ivl, ..)`.
    Generators {
        /// The other generator.
        other: Gen,
        /// The other extrusion vector.
        vec: [f64; 3],
        /// The other sweep interval.
        ivl: [f64; 2],
        /// Tolerance.
        tol: f64,
    },
}

impl Op {
    const fn code(&self) -> i32 {
        match self {
            Op::Build => 0,
            Op::EvalStep { .. } => 1,
            Op::ToNurbs { .. } => 2,
            Op::ToStep { .. } => 3,
            Op::Adjust { .. } => 4,
            Op::Update { .. } => 5,
            Op::Reparam { .. } => 6,
            Op::Transform { .. } => 7,
            Op::Directrix { .. } => 8,
            Op::DropFast { .. } => 9,
            Op::StepInv { .. } => 10,
            Op::Eq { .. } => 11,
            Op::Copy => 12,
            Op::Generators { .. } => 13,
        }
    }
}

pub(crate) fn finite(v: &[f64], what: &str) -> Result<(), FfiError> {
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
/// does not overflow: unitizing an axis of infinite length gives a zero axis, so a conic
/// generator on it gets no NURB, which `SmSurfOfExtrusion`'s constructor then evaluates
/// (`EvaluatePoint` dereferences the NULL NURB).
pub(crate) fn axes(x: [f64; 3], y: [f64; 3], what: &str) -> Result<(), FfiError> {
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

/// Checks an ordered finite domain, with positive extents when `strict`.
pub(crate) fn domain(d: [f64; 4], strict: bool, what: &str) -> Result<(), FfiError> {
    finite(&d, what)?;
    let [a, b, c, e] = d;
    let ok = if strict {
        a < c && b < e
    } else {
        a <= c && b <= e
    };
    if !ok {
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
pub(crate) fn check_knots(knots: &[f64], p: usize, what: &str) -> Result<(), FfiError> {
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
pub(crate) fn check_points(pts: &[[f64; 4]], what: &str) -> Result<(), FfiError> {
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

/// Largest centre coordinate and size of a hyperbola or parabola generator.
pub const MAX_CONIC: f64 = 1e12;

/// `SmHyperbola::MakeNurb` / `SmParabola::MakeNurb` place the middle control point at
/// the intersection of the end tangents (`smgu_LineLineClosestPoint`); with a centre or
/// size near the top of the double range the points collapse or the products overflow,
/// that fails, and the extrusion constructor then evaluates the generator's NULL NURB.
/// Bounds the centre and sizes by [`MAX_CONIC`].
fn conic_size(c: &[f64; 3], sizes: &[f64], what: &str) -> Result<(), FfiError> {
    if c.iter().chain(sizes).any(|v| v.abs() > MAX_CONIC) {
        return Err(bad(format!("{what}: centre or size above MAX_CONIC")));
    }
    Ok(())
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
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
        Gen::Line { a, b } | Gen::LazyLine { a, b } => {
            finite(a, "line")?;
            finite(b, "line")?;
            let [ax, ay, az] = *a;
            let [bx, by, bz] = *b;
            if len([bx - ax, by - ay, bz - az]) <= 0.0 {
                return Err(bad("line: degenerate"));
            }
            o.push(if matches!(g, Gen::LazyLine { .. }) {
                6.0
            } else {
                1.0
            });
            o.extend_from_slice(a);
            o.extend_from_slice(b);
        }
        Gen::Circle { c, x, y, d, r } => {
            finite(c, "circle")?;
            axes(*x, *y, "circle")?;
            let [d0, d1] = *d;
            angular(d0, d1, "circle")?;
            if !(r.is_finite() && *r > 0.0) {
                return Err(bad("circle: radius"));
            }
            o.push(2.0);
            o.extend_from_slice(c);
            o.extend_from_slice(x);
            o.extend_from_slice(y);
            o.extend([d0, d1, *r]);
        }
        Gen::Ellipse { c, x, y, d, rx, ry } => {
            finite(c, "ellipse")?;
            axes(*x, *y, "ellipse")?;
            let [d0, d1] = *d;
            angular(d0, d1, "ellipse")?;
            if !(rx.is_finite() && *rx > 0.0 && ry.is_finite() && *ry > 0.0) {
                return Err(bad("ellipse: radii"));
            }
            // HANG-23 (fuzz01428): an ellipse extrusion whose minor radius is so
            // small that `rx / ry` overflows (ry below 1/f64::MAX, the radius whose
            // reciprocal exceeds DBL_MAX) hangs the C++ adjust: the STEP evaluation
            // collapses onto the centre and the point-inversion solve (SM_CP_POINT
            // _INVERSION inside the polar converter's ConvertToNURBSParameter) never
            // converges. Probing the C++: every such centre but exactly [0, 0, 0]
            // spins (1-3 ms bite marks at the subnormal edge), while at the origin
            // the folded conic takes the degenerate branch and returns (review6 keeps
            // ry = 5e-324 there); normal-scale minor radii pass at any centre, and
            // circles with denormal radii never hang.
            if (rx / ry).is_infinite() && c.iter().any(|v| *v != 0.0) {
                return Err(bad("ellipse: minor radius below double resolution"));
            }
            o.push(3.0);
            o.extend_from_slice(c);
            o.extend_from_slice(x);
            o.extend_from_slice(y);
            o.extend([d0, d1, *rx, *ry]);
        }
        Gen::Hyperbola { c, x, y, d, a, b } => {
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
            conic_size(c, &[*a, *b], "hyperbola")?;
            o.push(4.0);
            o.extend_from_slice(c);
            o.extend_from_slice(x);
            o.extend_from_slice(y);
            o.extend([d0, d1, *a, *b]);
        }
        Gen::Parabola { c, x, y, d, f } => {
            finite(c, "parabola")?;
            axes(*x, *y, "parabola")?;
            let [d0, d1] = *d;
            finite(&[d0, d1], "parabola")?;
            if !(d0 < d1 && d0 >= -1000.0 && d1 <= 1000.0) {
                return Err(bad("parabola: domain"));
            }
            if !(f.is_finite() && *f > 1e-6) {
                return Err(bad("parabola: focal distance"));
            }
            conic_size(c, &[*f], "parabola")?;
            o.push(5.0);
            o.extend_from_slice(c);
            o.extend_from_slice(x);
            o.extend_from_slice(y);
            o.extend([d0, d1, *f]);
        }
    }
    Ok(())
}

pub(crate) fn push_surf(o: &mut Vec<f64>, s: &Surf) -> Result<(), FfiError> {
    let (mode, tail): (f64, Vec<f64>) = match &s.build {
        Build::Canonical { vec } => {
            finite(vec, "extrusion vector")?;
            (0.0, vec.to_vec())
        }
        Build::Ctor {
            origin,
            vec,
            domain: d,
        } => {
            finite(origin, "origin")?;
            finite(vec, "extrusion vector")?;
            domain(*d, true, "surface domain")?;
            let mut t = origin.to_vec();
            t.extend_from_slice(vec);
            t.extend_from_slice(d);
            (1.0, t)
        }
        Build::Infinite { vec } => {
            finite(vec, "extrusion vector")?;
            (2.0, vec.to_vec())
        }
    };
    o.push(mode);
    push_gen(o, &s.gen)?;
    o.extend([flag(s.oob), flag(s.swap)]);
    o.extend(tail);
    Ok(())
}

fn push_op(o: &mut Vec<f64>, op: &Op) -> Result<(), FfiError> {
    match op {
        Op::Build | Op::Copy => {}
        Op::EvalStep { u, v, k, point } => {
            finite(&[*u, *v], "eval")?;
            if *k > MAX_ORDER {
                return Err(bad("eval: order too high"));
            }
            o.extend([*u, *v, *k as f64, flag(*point)]);
        }
        Op::ToNurbs { u, v } | Op::ToStep { u, v } => {
            finite(&[*u, *v], "conversion")?;
            o.extend([*u, *v]);
        }
        Op::Adjust { dom } | Op::Update { dom } | Op::Reparam { dom } => {
            domain(*dom, true, "edit domain")?;
            o.extend_from_slice(dom);
        }
        Op::Transform {
            origin,
            x,
            y,
            scale,
        } => {
            finite(origin, "transform")?;
            axes(*x, *y, "transform")?;
            let sc = scale.unwrap_or([1.0, 1.0, 1.0]);
            finite(&sc, "transform scale")?;
            o.extend_from_slice(origin);
            o.extend_from_slice(x);
            o.extend_from_slice(y);
            o.push(flag(scale.is_some()));
            o.extend_from_slice(&sc);
        }
        Op::Directrix { p, tol } => {
            finite(p, "directrix")?;
            finite(&[*tol], "directrix")?;
            o.extend_from_slice(p);
            o.push(*tol);
        }
        Op::DropFast {
            dom,
            op,
            p,
            vec,
            tol,
            target,
            requested,
        } => {
            domain(*dom, false, "drop domain")?;
            // SM_SO_INTERSECT, SM_SO_MINIMIZE, SM_SO_MAXIMIZE, SM_SO_NORMALIZE
            if !matches!(op, 0 | 3 | 4 | 5) || *requested > 1 {
                return Err(bad("drop: operation"));
            }
            finite(p, "drop")?;
            let v = vec.unwrap_or([0.0, 0.0, 0.0]);
            finite(&v, "drop vector")?;
            let t = target.unwrap_or(0.0);
            finite(&[*tol, t], "drop")?;
            o.extend_from_slice(dom);
            o.push(f64::from(*op));
            o.extend_from_slice(p);
            o.push(flag(vec.is_some()));
            o.extend_from_slice(&v);
            o.extend([*tol, flag(target.is_some()), t, f64::from(*requested)]);
        }
        Op::StepInv { dom, p, tol, guess } => {
            domain(*dom, false, "inversion domain")?;
            finite(p, "inversion")?;
            let g = guess.unwrap_or([0.0, 0.0]);
            finite(&[*tol, g[0], g[1]], "inversion")?;
            o.extend_from_slice(dom);
            o.extend_from_slice(p);
            o.extend([*tol, flag(guess.is_some()), g[0], g[1]]);
        }
        Op::Eq { other } => push_surf(o, other)?,
        Op::Generators {
            other,
            vec,
            ivl,
            tol,
        } => {
            push_gen(o, other)?;
            finite(vec, "generators vector")?;
            let [i0, i1] = *ivl;
            finite(&[i0, i1, *tol], "generators")?;
            if i0 > i1 {
                return Err(bad("generators: interval not ordered"));
            }
            o.extend_from_slice(vec);
            o.extend([i0, i1, *tol]);
        }
    }
    Ok(())
}

fn call(op: i32, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (built and validated above: well-formed clamped 3D curves,
        // bounded counts and orders, finite values, non-degenerate axes, lines, circles,
        // ellipses, hyperbolas, parabolas and domains); buf is valid for buf.len() writes and the shim honours
        // the capacity; len is valid.
        unsafe {
            sys::sc_surface_surf_of_extrusion_p1_eval(
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
    // EvaluateSTEP allows order 5 (its MAXDER comment), but a B-spline generator
    // evaluates through sm_CrvDerivs, whose NL_DERIVATIVES rows stop at NL_MAXDER = 4:
    // order 5 smashes the stack. The analytic generators evaluate order 5 fine.
    if let (
        Op::EvalStep {
            k: 5, point: false, ..
        },
        Gen::BSpline(_),
    ) = (op, &s.gen)
    {
        return Err(bad(
            "eval: order 5 of a B-spline generator overruns NL_MAXDER",
        ));
    }
    // `AdjustSTEPUVDomain` of an ellipse/circle extrusion to an angular u end
    // outside [-360, 360] hangs in C++ (HANG-23): the polar set-up shifts one
    // turn per iteration. Bound the STEP u domain like the generator's angular
    // domain.
    if let Op::Adjust { dom } = op {
        if matches!(s.gen, Gen::Circle { .. } | Gen::Ellipse { .. }) {
            angular(dom[0], dom[2], "adjust domain")?;
        }
    }
    let mut input = Vec::new();
    push_surf(&mut input, s)?;
    push_op(&mut input, op)?;
    call(op.code(), &input)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds `AdjustSTEPUVDomain` on an ellipse extrusion with centre [0, c, 0].
    fn adjust(c: f64, ry: f64) -> Result<Vec<f64>, FfiError> {
        surface_op(
            &Surf {
                gen: Gen::Ellipse {
                    c: [0.0, c, 0.0],
                    x: [1.0, 0.0, 0.0],
                    y: [0.0, 1.0, 0.0],
                    d: [0.0, 180.0],
                    rx: 1.0,
                    ry,
                },
                build: Build::Infinite { vec: [-1.0, 0.0, 0.0] },
                oob: false,
                swap: true,
            },
            &Op::Adjust { dom: [0.1, 0.1, 0.5, 0.5] },
        )
    }

    /// HANG-23: `rx / ry` overflowing (fuzz01428: centre 2^32, ry 5e-324; probes
    /// pinned the onset at a razor near `1 / f64::MAX * rx`: the same centre hangs
    /// just below the razor and passes just above it) spins C++ on any centre
    /// but exactly [0, 0, 0].
    #[test]
    fn overflow_aspect_is_rejected_on_adjust() {
        for (c, ry) in [
            (4294967296.0, 5e-324),
            (0.25, 5e-324),
            (1.0, 5e-324),
            (1e300, 5e-324),
            (1.0, 5e-309),
            (1.0, 1e-309),
            (1e300, 1e-309),
        ] {
            match adjust(c, ry) {
                Err(FfiError::InvalidArgument(m)) => {
                    assert!(
                        m.contains("minor radius below double resolution"),
                        "{m}",
                    );
                }
                Err(other) => panic!("expected argument rejection for ({c}, {ry}), got {other}"),
                Ok(_) => panic!("C++ hangs for ({c}, {ry}): the guard must reject it"),
            }
        }
    }

    /// Neighbours of the hanging class that C++ handles must stay covered: a
    /// denormal minor radius at the origin, a finite rx/ry ratio at any centre
    /// magnitude, denormal major radii, and denormal circles.
    #[test]
    fn finite_aspect_still_runs_on_adjust() {
        // the razor: 5.5e-309 hangs, 5.6e-309 passes (repeatable in C++).
        adjust(0.0, 5e-324).expect("origin: the degenerate branch returns");
        for (c, ry) in [
            (1.0, 5.6e-309),
            (1e300, 5.9e-309),
            (1.0, 1e-16),
            (1e300, 0.7),
            (1.0, 1.0),
        ] {
            adjust(c, ry).expect("finite rx/ry stays covered");
        }
    }

    /// HANG-23: far adjusted STEP u ends hang C++ (fuzz00222 / fuzz01418).
    #[test]
    fn adjust_u_end_beyond_the_anglar_domain_is_rejected() {
        for u in [9223372036854775807.0, 1e300, 361.0] {
            match surface_op(
                &Surf {
                    gen: Gen::Ellipse {
                        c: [1.0, 2.0, -0.5],
                        x: [1.0, 0.0, 0.0],
                        y: [0.0, 1.0, 0.0],
                        d: [45.0, 135.0],
                        rx: 3.0,
                        ry: 0.7,
                    },
                    build: Build::Canonical { vec: [0.0, 0.0, 1.0] },
                    oob: false,
                    swap: false,
                },
                &Op::Adjust { dom: [90.0, 0.0, u, 1.0] },
            ) {
                Err(FfiError::InvalidArgument(m)) => assert!(
                    m.contains("adjust domain: bad angular domain"),
                    "{m}"
                ),
                other => panic!("expected rejection for u={u}, got {:?}", other.map(|_| ())),
            }
        }
    }
}
