//! Runner for unit `surface.offset_surface`. Ported from upstream
//! `source/SMLib/src/SmOffsetSurface.cpp`; checked against its golden. Rust:
//! `solidcore::surface::offset_surface`.
//!
//! A case builds an `SmOffsetSurface` over a base surface (a B-spline, an upstream `.sms`
//! file, a plane, a curve-bounded or STEP surface, a surface of revolution, or a nested
//! offset) and runs a script of ops on that one object, so that the extended surface
//! upstream's `const` evaluators store is seen by the later ops (the Rust side runs the
//! storing `*_extending` evaluators; an op marked `via` runs the `&self` evaluators through
//! the `Surface` interface instead and is only generated where no extension can be
//! stored). A `read_db` case reads an upstream `WriteToDB` record of an offset (frozen in
//! `fixtures/surface.offset_surface/written_records.json`: solidcore has no writer, #2857).
//! The `avNNNN` cases (their own seed stream, so the `genNNNN` inputs are unchanged) run
//! `AssertValid` on an offset after an optional edit and compare every report.
//!
//! Output `out`: the construction status, then each op's status and outputs; a failure
//! is the upstream `SmStatus` (`InvalidInput` is `SM_ERR_INVALID_INPUT`, the zero-vector
//! status of `Unitize`). `Unported` on the Rust side defers the case.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::assert_array::{AssertArray, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm, SurfParamType};
use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use solidcore::nurbs::knots::Side;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::curve_bounded_surface::CurveBoundedSurface;
use solidcore::surface::offset_surface::{OffsetSurface, SS_UNKNOWN};
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::p2::OFFSET_SURFACE_TYPE;
use solidcore::surface::surface::{StepSurface, Surface, SurfaceKind};
use solidcore::topology::database_io::{DatabaseReader, FileType, InputStream};
use solidcore::SolidError;

use super::surface_surf_of_revolution_p1 as rev;
use crate::case::CaseContext;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

#[path = "fixtures/surface_offset_surface_sms.rs"]
mod surface_offset_surface_sms;

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `surface.offset_surface` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BssIn {
    #[serde(default)]
    oob: bool,
    #[serde(default = "default_form")]
    form: u8,
    net: Vec<Vec<[f64; 4]>>,
    p: usize,
    q: usize,
    u: Vec<f64>,
    v: Vec<f64>,
}

fn default_form() -> u8 {
    BSplineSurfaceForm::Unspecified as u8
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SrfIn {
    Bspline(BssIn),
    /// An upstream `.sms` file (absolute path after fixture expansion).
    Sms(String),
    Plane {
        o: [f64; 3],
        n: [f64; 3],
    },
    Bounded {
        basis: Box<SrfIn>,
        implicit: bool,
    },
    Step {
        base: Box<SrfIn>,
    },
    Revolution(rev::SurfIn),
    Offset(Box<OffIn>),
}

const fn yes() -> bool {
    true
}

const fn unknown() -> u32 {
    SS_UNKNOWN
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OffIn {
    d: f64,
    #[serde(default = "yes")]
    owns: bool,
    #[serde(default = "unknown")]
    sing: u32,
    #[serde(default)]
    allow: bool,
    base: SrfIn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum StepIn {
    State,
    Eval {
        uv: [f64; 2],
        hu: usize,
        hv: usize,
        #[serde(default = "yes")]
        ul: bool,
        #[serde(default = "yes")]
        vl: bool,
        #[serde(default)]
        upper: bool,
        #[serde(default = "yes")]
        nzt: bool,
        #[serde(default)]
        via: bool,
    },
    EvalSimple {
        uv: [f64; 2],
        hu: usize,
        hv: usize,
        #[serde(default = "yes")]
        ul: bool,
        #[serde(default = "yes")]
        vl: bool,
        #[serde(default)]
        upper: bool,
        #[serde(default)]
        via: bool,
    },
    EvalPt {
        uv: [f64; 2],
        #[serde(default)]
        via: bool,
    },
    Normal {
        uv: [f64; 2],
        #[serde(default = "yes")]
        ul: bool,
        #[serde(default = "yes")]
        vl: bool,
        #[serde(default)]
        via: bool,
    },
    Knots {
        param: u8,
        #[serde(default)]
        ivl: Option<[f64; 2]>,
    },
    MaxAnal,
    #[serde(rename = "bbox")]
    BBox {
        dom: [f64; 4],
        want: [bool; 3],
        #[serde(default = "yes")]
        expand: bool,
    },
    StepInv {
        dom: [f64; 4],
        p: [f64; 3],
        tol: f64,
        #[serde(default)]
        guess: Option<[f64; 2]>,
    },
    Reverse {
        param: u8,
    },
    Swap,
    Transform {
        origin: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        #[serde(default)]
        scale: Option<[f64; 3]>,
    },
    Trim {
        dom: [f64; 4],
    },
    Split {
        t: f64,
        param: u8,
    },
    CreateOffset {
        d: f64,
    },
    Copy,
    Eq {
        other: OffIn,
    },
    SetDist {
        d: f64,
    },
    SetAllow {
        b: bool,
    },
    IsPlanar {
        tol: f64,
    },
    IsKindOf {
        t: i64,
    },
    BaseOffset {
        d: f64,
        tol: f64,
    },
    AssertValid {
        level: u8,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Inputs {
    Script {
        off: OffIn,
        steps: Vec<StepIn>,
    },
    /// `bytes` absent only in a generated draw, never in a case.
    ReadDb {
        off: OffIn,
        binary: bool,
        version: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bytes: Option<Vec<u8>>,
    },
}

/// Reads an upstream `.sms` text file: `n m`, `p q`, rational flag, `(n+1)(m+1)` points
/// (`x y z`, or `wx wy wz w` when rational, v index fastest), `n+p+2` u knots, `m+q+2` v
/// knots.
fn read_sms(path: &str) -> Result<BssIn, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    let parsed = surface_offset_surface_sms::parse_sms(&text, path)?;
    Ok(BssIn {
        oob: false,
        form: default_form(),
        net: parsed.net,
        p: parsed.p,
        q: parsed.q,
        u: parsed.u,
        v: parsed.v,
    })
}

// ---- generated input contract ---------------------------------------------------------------
//
// The generator keeps a draw only inside the domain upstream's entry points are defined on,
// without undefined behaviour or runaway work: well-formed clamped B-splines, planes with a
// non-zero normal, offsets nested at most [`MAX_NESTING`] deep, finite values, parameters
// within [`MAX_REACH`] domain lengths of the base domain, bounded derivative orders, no
// bounding boxes or planarity tests over a curve-bounded or STEP base (their polar box
// recurses without bound upstream), no box whose domain may lie outside the base domain, no
// planarity test once an extension may exist, and surfaces of revolution as unit
// `surface.surf_of_revolution_p1` builds them. Generated cases exercise the working path;
// hostile inputs are the fixtures' job.

/// Largest control point index.
const MAX_INDEX: usize = 24;
/// Largest degree.
const MAX_DEGREE: usize = 5;
/// Deepest offset nesting in a block.
const MAX_NESTING: usize = 3;
/// Largest number of ops in a script.
const MAX_OPS: usize = 16;
/// How many domain lengths outside the base domain a parameter may be.
const MAX_REACH: f64 = 3.0;
/// Largest derivative order.
const MAX_ORDER: usize = 64;
/// Largest control point index of a surface of revolution's B-spline generator.
const MAX_REV_INDEX: usize = 256;
/// Largest degree of a surface of revolution's B-spline generator.
const MAX_REV_DEGREE: usize = 9;
/// `SM_EFF_ZERO`.
const EFF_ZERO: f64 = 1e-12;

fn all_finite(v: &[f64]) -> bool {
    v.iter().all(|c| c.is_finite())
}

fn nonzero(v: [f64; 3]) -> bool {
    all_finite(&v) && v.iter().map(|c| c * c).sum::<f64>() >= 1e-12
}

/// A finite domain with `u0 <= u1` and `v0 <= v1`.
fn domain_ok(d: [f64; 4]) -> bool {
    let [u0, v0, u1, v1] = d;
    all_finite(&d) && u0 <= u1 && v0 <= v1
}

/// A clamped knot vector of degree `p`, optionally of a parameter range of at least
/// `1e-6` relative; its ends.
fn knots_ok(knots: &[f64], p: usize, min_range: bool) -> Option<(f64, f64)> {
    if !all_finite(knots) || knots.windows(2).any(|w| w.first() > w.last()) {
        return None;
    }
    let m = knots.len().checked_sub(1)?;
    let (&a, &b) = (knots.first()?, knots.last()?);
    if m < 2 * p + 1 {
        return None;
    }
    let (head, tail, inner) = (knots.get(..=p)?, knots.get(m - p..)?, knots.get(p + 1..m - p)?);
    if a >= b || head.iter().any(|&k| k != a) || tail.iter().any(|&k| k != b) {
        return None;
    }
    if min_range && b - a < 1e-6 * a.abs().max(b.abs()).max(1.0) {
        return None;
    }
    if inner.iter().any(|&k| k == a || k == b) {
        return None;
    }
    let mut run = 1;
    for w in inner.windows(2) {
        run = if w.first() == w.last() { run + 1 } else { 1 };
        if run > p {
            return None;
        }
    }
    Some((a, b))
}

/// A B-spline base's natural domain, if it is well formed.
fn bss_domain(s: &BssIn) -> Option<[f64; 4]> {
    if s.form > 11 {
        return None;
    }
    let n = s.net.len().checked_sub(1)?;
    let cols = s.net.first().map_or(0, Vec::len);
    let m = cols.checked_sub(1)?;
    if s.net.iter().any(|r| r.len() != cols) {
        return None;
    }
    if n > MAX_INDEX || m > MAX_INDEX || s.p > MAX_DEGREE || s.q > MAX_DEGREE {
        return None;
    }
    if s.p == 0 || s.p > n || s.q == 0 || s.q > m {
        return None;
    }
    if s.u.len() != n + s.p + 2 || s.v.len() != m + s.q + 2 {
        return None;
    }
    let mut rational = None;
    for &[x, y, z, w] in s.net.iter().flatten() {
        let r = w != NOW;
        if !all_finite(&[x, y, z]) || *rational.get_or_insert(r) != r {
            return None;
        }
        if r && !(1e-3..=1e3).contains(&w) {
            return None;
        }
    }
    let (u0, u1) = knots_ok(&s.u, s.p, true)?;
    let (v0, v1) = knots_ok(&s.v, s.q, true)?;
    Some([u0, v0, u1, v1])
}

fn len3(v: [f64; 3]) -> f64 {
    v.iter().map(|c| c * c).sum::<f64>().sqrt()
}

/// Two axes long enough, far from parallel, and of a length whose square does not
/// overflow (a generator circle on a zero unitized axis gets no NURB).
fn axes_ok(x: [f64; 3], y: [f64; 3]) -> bool {
    let (lx, ly) = (len3(x), len3(y));
    all_finite(&x)
        && all_finite(&y)
        && lx.is_finite()
        && ly.is_finite()
        && lx >= 1e-6
        && ly >= 1e-6
        && len3(rev::cross3(x, y)) >= 1e-3 * lx * ly
}

/// A unit vector within `SM_EFF_ZERO` on its squared length.
fn unit_ok(v: [f64; 3]) -> bool {
    (v.iter().map(|c| c * c).sum::<f64>() - 1.0).abs() <= EFF_ZERO
}

/// An angular domain of at most one turn within `[-360, 360]`.
fn angular_ok(d0: f64, d1: f64) -> bool {
    all_finite(&[d0, d1]) && d0 < d1 && d0 >= -360.0 && d1 <= 360.0 && d1 - d0 <= 360.0
}

/// A hyperbola domain bounded so that `cosh` stays finite.
fn hyperbola_domain_ok(d0: f64, d1: f64) -> bool {
    d0 < d1 && d0 >= -10.0 && d1 <= 10.0
}

/// A generator of a surface of revolution: a B-spline with consistent weights, a line
/// or circle of at least `1e-6`, or a hyperbola with a bounded domain and positive semi
/// axes.
fn gen_ok(g: &rev::GenIn) -> bool {
    match g {
        rev::GenIn::BSpline(c) => {
            let Some(n) = c.cpts.len().checked_sub(1) else {
                return false;
            };
            if c.weights.as_ref().is_some_and(|w| w.len() != c.cpts.len()) {
                return false;
            }
            if c.p == 0 || c.p > n || c.p > MAX_REV_DEGREE || n > MAX_REV_INDEX {
                return false;
            }
            if c.u.len() != n + c.p + 2 {
                return false;
            }
            let pts_ok = c.cpts.iter().enumerate().all(|(i, &[x, y, z])| match &c.weights {
                None => all_finite(&[x, y, z]),
                Some(w) => w.get(i).is_some_and(|&w| {
                    all_finite(&[x * w, y * w, z * w]) && w != NOW && w.is_finite() && w > 0.0
                }),
            });
            pts_ok && knots_ok(&c.u, c.p, false).is_some()
        }
        rev::GenIn::Line { a, b } => {
            all_finite(a) && all_finite(b) && len3([b[0] - a[0], b[1] - a[1], b[2] - a[2]]) >= 1e-6
        }
        rev::GenIn::Circle { c, x, y, d, r } => {
            all_finite(c) && axes_ok(*x, *y) && angular_ok(d[0], d[1]) && r.is_finite() && *r > 1e-6
        }
        rev::GenIn::Hyperbola { c, x, y, d, a, b } => {
            all_finite(c)
                && axes_ok(*x, *y)
                && all_finite(d)
                && hyperbola_domain_ok(d[0], d[1])
                && a.is_finite()
                && *a > 1e-6
                && b.is_finite()
                && *b > 1e-6
        }
    }
}

/// A surface of revolution inside the domain upstream's constructors are defined on (also
/// the generated input contract of unit `surface.surf_of_revolution_p3`): a well-formed
/// generator, unit placement axes for the constructor (its `IsOnPlane` test reads an
/// uninitialised flag otherwise) and bounded angular domains (huge ones hang the polar
/// set-up).
pub(crate) fn revolution_ok(s: &rev::SurfIn) -> bool {
    if !gen_ok(&s.gen) {
        return false;
    }
    match &s.build {
        rev::BuildIn::Canonical { point, axis } => all_finite(point) && all_finite(axis),
        rev::BuildIn::Ctor {
            origin,
            x,
            y,
            domain,
        } => {
            let [u0, v0, u1, v1] = *domain;
            let v_ok = match &s.gen {
                rev::GenIn::Circle { .. } => angular_ok(v0, v1),
                rev::GenIn::Hyperbola { .. } => hyperbola_domain_ok(v0, v1),
                _ => true,
            };
            all_finite(origin)
                && axes_ok(*x, *y)
                && unit_ok(*x)
                && unit_ok(*y)
                && domain_ok(*domain)
                && angular_ok(u0, u1)
                && v_ok
        }
    }
}

/// What the contract knows about a base: its natural domain (`None` when not known in
/// advance) and whether a bounding box of it recurses upstream.
#[derive(Debug, Clone, Copy)]
struct BaseInfo {
    dom: Option<[f64; 4]>,
    unboxable: bool,
}

fn srf_info(s: &SrfIn, depth: usize) -> Option<BaseInfo> {
    Some(match s {
        SrfIn::Bspline(bs) => BaseInfo {
            dom: Some(bss_domain(bs)?),
            unboxable: false,
        },
        SrfIn::Sms(path) => BaseInfo {
            dom: Some(bss_domain(&read_sms(path).ok()?)?),
            unboxable: false,
        },
        SrfIn::Plane { o, n } => {
            if !all_finite(o) || !nonzero(*n) {
                return None;
            }
            BaseInfo {
                dom: None,
                unboxable: false,
            }
        }
        SrfIn::Bounded { basis, .. } => {
            if !matches!(**basis, SrfIn::Bspline(_) | SrfIn::Sms(_) | SrfIn::Plane { .. }) {
                return None;
            }
            BaseInfo {
                dom: srf_info(basis, depth)?.dom,
                unboxable: true,
            }
        }
        SrfIn::Step { base } => {
            let bspline = matches!(**base, SrfIn::Bspline(_) | SrfIn::Sms(_));
            if !bspline && !matches!(**base, SrfIn::Plane { .. } | SrfIn::Revolution(_)) {
                return None;
            }
            let info = srf_info(base, depth)?;
            BaseInfo {
                // (the STEP domain of a surface of revolution differs)
                dom: if bspline { info.dom } else { None },
                unboxable: true,
            }
        }
        SrfIn::Revolution(r) => {
            if !revolution_ok(r) {
                return None;
            }
            BaseInfo {
                dom: None,
                unboxable: false,
            }
        }
        SrfIn::Offset(off) => {
            if depth >= MAX_NESTING {
                return None;
            }
            off_info(off, depth + 1)?
        }
    })
}

fn off_info(off: &OffIn, depth: usize) -> Option<BaseInfo> {
    if !off.d.is_finite() || off.sing > 16 {
        return None;
    }
    srf_info(&off.base, depth)
}

/// A parameter within [`MAX_REACH`] domain lengths of `dom` (any finite one within a
/// fixed reach when the domain is not known in advance).
fn uv_ok(uv: [f64; 2], dom: Option<[f64; 4]>) -> bool {
    let [u, v] = uv;
    all_finite(&uv)
        && match dom {
            Some([u0, v0, u1, v1]) => {
                let (du, dv) = ((u1 - u0) * MAX_REACH, (v1 - v0) * MAX_REACH);
                u >= u0 - du && u <= u1 + du && v >= v0 - dv && v <= v1 + dv
            }
            None => u.abs() <= 1.0e4 && v.abs() <= 1.0e4,
        }
}

fn contained(inner: [f64; 4], outer: Option<[f64; 4]>) -> bool {
    let tol = 1e-12;
    outer.is_some_and(|[u0, v0, u1, v1]| {
        inner[0] >= u0 - tol && inner[1] >= v0 - tol && inner[2] <= u1 + tol && inner[3] <= v1 + tol
    })
}

/// The base a box asks for its boxes, through nested offsets.
fn box_base(off: &OffIn) -> &SrfIn {
    match &off.base {
        SrfIn::Offset(inner) => box_base(inner),
        s => s,
    }
}

/// Whether a script on `off` is inside the contract.
fn script_ok(off: &OffIn, steps: &[StepIn]) -> bool {
    let Some(info) = off_info(off, 0) else {
        return false;
    };
    if steps.len() > MAX_OPS {
        return false;
    }
    // the base domain as the script goes (unknown once an edit may have changed it)
    let mut dom = info.dom;
    let unboxable =
        info.unboxable || matches!(box_base(off), SrfIn::Bounded { .. } | SrfIn::Step { .. });
    // an extension may exist once an evaluation ran with extension allowed
    let mut allow = off.allow;
    let mut ext_possible = false;
    for s in steps {
        let ok = match s {
            StepIn::State | StepIn::MaxAnal | StepIn::Copy => true,
            StepIn::Eval { uv, hu, hv, .. } | StepIn::EvalSimple { uv, hu, hv, .. } => {
                ext_possible |= allow;
                uv_ok(*uv, dom) && *hu <= MAX_ORDER && *hv <= MAX_ORDER
            }
            StepIn::EvalPt { uv, .. } | StepIn::Normal { uv, .. } => {
                ext_possible |= allow;
                uv_ok(*uv, dom)
            }
            StepIn::Knots { param, ivl } => *param <= 1 && all_finite(&ivl.unwrap_or([0.0, 0.0])),
            StepIn::BBox { dom: d, .. } => {
                domain_ok(*d) && !unboxable && (dom.is_none() || contained(*d, dom))
            }
            StepIn::StepInv { dom: d, p, tol, guess } => {
                ext_possible |= allow;
                domain_ok(*d)
                    && all_finite(p)
                    && tol.is_finite()
                    && *tol > 0.0
                    && *tol <= 1.0
                    && uv_ok(guess.unwrap_or([0.0, 0.0]), dom)
            }
            StepIn::Reverse { param } => {
                dom = None;
                *param <= 1
            }
            StepIn::Swap => {
                dom = None;
                true
            }
            StepIn::Transform {
                origin,
                x,
                y,
                scale,
            } => {
                all_finite(origin)
                    && nonzero(*x)
                    && nonzero(*y)
                    && rev::cross3(*x, *y).iter().map(|v| v * v).sum::<f64>() >= 1e-6
                    && all_finite(&scale.unwrap_or([1.0, 1.0, 1.0]))
            }
            StepIn::Trim { dom: d } => {
                let ok = domain_ok(*d)
                    && d[2] - d[0] >= 1e-6
                    && d[3] - d[1] >= 1e-6
                    && uv_ok([d[0], d[1]], dom)
                    && uv_ok([d[2], d[3]], dom);
                ext_possible |= allow;
                dom = None;
                ok
            }
            StepIn::Split { t, param } => {
                // strictly inside the current base domain (a plane or a surface of
                // revolution, or a base an earlier op edited, has none known here)
                t.is_finite()
                    && *param <= 1
                    && dom.is_some_and(|d| {
                        let (a, z) = if *param == 0 { (d[0], d[2]) } else { (d[1], d[3]) };
                        *t > a && *t < z
                    })
            }
            StepIn::CreateOffset { d } | StepIn::SetDist { d } => d.is_finite(),
            StepIn::Eq { other } => off_info(other, 1).is_some(),
            StepIn::SetAllow { b } => {
                allow = *b;
                true
            }
            StepIn::IsPlanar { tol } => {
                !unboxable && !ext_possible && tol.is_finite() && *tol >= 0.0
            }
            StepIn::IsKindOf { t } => t.unsigned_abs() <= 1 << 20,
            StepIn::BaseOffset { d, tol } => d.is_finite() && tol.is_finite() && *tol >= 0.0,
            // the self-intersection boxes the natural domain, as IsPlanar's box
            StepIn::AssertValid { level } => {
                !unboxable && !ext_possible && matches!(level, 0 | 1 | 2 | 4)
            }
        };
        if !ok {
            return false;
        }
    }
    true
}

/// Whether a generated draw is inside the contract. A `read_db` draw is judged on the
/// offset whose record it reads.
fn admits(i: &Inputs) -> bool {
    match i {
        Inputs::Script { off, steps } => script_ok(off, steps),
        Inputs::ReadDb { off, .. } => off_info(off, 0).is_some(),
    }
}

// ---- Rust side ------------------------------------------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn p2(p: [f64; 2]) -> Point2 {
    Point2::new(p[0], p[1])
}

fn dom_of(d: [f64; 4]) -> Extent2d {
    Extent2d::from_bounds(d[0], d[1], d[2], d[3])
}

fn side(left: bool) -> Side {
    if left {
        Side::Left
    } else {
        Side::Right
    }
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

fn param_of(v: u8) -> Result<SurfParamType, HarnessError> {
    SurfParamType::from_code(u64::from(v)).ok_or_else(|| harness(format!("bad param {v}")))
}

/// The `SmStatus` of a Rust error.
fn status(e: &SolidError) -> f64 {
    match e {
        e if solidcore::error::nurbs::upstream_status(e).is_some() => {
            f64::from(solidcore::error::nurbs::upstream_status(e).unwrap_or(-1))
        }
        // SM_ERR_INVALID_INPUT (the zero-vector status of Unitize)
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
        _ => -1.0,
    }
}

/// A Rust failure: deferred for `Unported`, else a harness error.
enum Fail {
    Deferred(Box<Output>),
    Harness(HarnessError),
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Fail::Harness(e)
    }
}

fn defer(e: &SolidError) -> Option<Fail> {
    Output::unported(e).map(|o| Fail::Deferred(Box::new(o)))
}

/// Pushes `0` and the outputs of `v` on success, the status on failure.
fn push_res<T>(
    o: &mut Vec<f64>,
    r: solidcore::Result<T>,
    f: impl FnOnce(&mut Vec<f64>, T) -> Result<(), Fail>,
) -> Result<(), Fail> {
    match r {
        Ok(v) => {
            o.push(0.0);
            f(o, v)
        }
        Err(e) => {
            if let Some(d) = defer(&e) {
                Err(d)
            } else {
                o.push(status(&e));
                Ok(())
            }
        }
    }
}

fn pv(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn put_dom(o: &mut Vec<f64>, d: &Extent2d) {
    o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
}

fn bss_of(b: &BssIn) -> Result<BSplineSurface, HarnessError> {
    let net = b
        .net
        .iter()
        .map(|r| {
            r.iter()
                .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
                .collect()
        })
        .collect();
    let nl = NlSurface::new(net, b.p, b.q, b.u.clone(), b.v.clone())
        .map_err(|e| harness(format!("surface input: {e}")))?;
    let mut s = BSplineSurface::from_nurb(nl, false);
    *s.out_of_bounds_enabled_mut() = b.oob;
    let form = BSplineSurfaceForm::from_code(u64::from(b.form))
        .ok_or_else(|| harness(format!("bad form {}", b.form)))?;
    s.set_b_spline_surface_form(form);
    Ok(s)
}

/// Builds a base; `Ok(Err(e))` when a construction fails (as upstream's does).
fn rust_srf(k: &Kernel, s: &SrfIn) -> Result<solidcore::Result<Surface>, HarnessError> {
    Ok(match s {
        SrfIn::Bspline(b) => Ok(Surface::from(bss_of(b)?)),
        SrfIn::Sms(path) => Ok(Surface::from(bss_of(&read_sms(path)?)?)),
        SrfIn::Plane { o, n } => Ok(Surface::from(Plane::from_normal(v3(*o), v3(*n)))),
        SrfIn::Bounded { basis, implicit } => rust_srf(k, basis)?.and_then(|b| {
            CurveBoundedSurface::new(b, Vec::new(), Vec::new(), *implicit).map(Surface::from)
        }),
        SrfIn::Step { base } => {
            rust_srf(k, base)?.and_then(|b| StepSurface::new(b, true).map(Surface::from))
        }
        SrfIn::Revolution(r) => rev::rust_surf(k, r)?.map(Surface::from),
        SrfIn::Offset(o) => rust_off(k, o)?.map(Surface::from),
    })
}

/// Builds an offset (`SmOffsetSurface(d, base, owns, sing)` then `SetAllowExtension`).
fn rust_off(k: &Kernel, o: &OffIn) -> Result<solidcore::Result<OffsetSurface>, HarnessError> {
    Ok(match rust_srf(k, &o.base)? {
        Ok(base) => OffsetSurface::new(k, o.d, base, o.owns, o.sing).map(|mut s| {
            s.set_allow_extension(o.allow);
            s
        }),
        Err(e) => Err(e),
    })
}

/// The 3x3 `EvaluatePoint` grid over the natural domain.
fn put_grid(k: &Kernel, o: &mut Vec<f64>, s: &Surface) -> Result<(), Fail> {
    let d = s.natural_uv_domain_with_nurb(k).map_err(|e| HarnessError::new(e.to_string()))?;
    for i in 0..3 {
        for j in 0..3 {
            let uv = d.evaluate(f64::from(i) / 2.0, f64::from(j) / 2.0);
            push_res(o, s.evaluate_point(k, uv), |o, p| {
                pv(o, p);
                Ok(())
            })?;
        }
    }
    Ok(())
}

fn put_state(k: &Kernel, o: &mut Vec<f64>, s: &OffsetSurface) -> Result<(), Fail> {
    put_dom(o, &s.natural_uv_domain());
    o.push(s.offset_distance());
    o.push(flag(s.allow_extension()));
    o.push(flag(s.owns_surface()));
    let [pu, pv_] = s.is_periodic();
    o.push(f64::from(pu));
    o.push(f64::from(pv_));
    o.push(f64::from(s.singularities()));
    // GetExtendedBaseSurface (with an extension a `&self` evaluator made)
    s.with_extended_base_surface(|e| match e {
        Some(e) => {
            o.push(1.0);
            // (an extension is a B-spline surface: never lazy)
            put_dom(o, &e.natural_uv_domain_if_built());
            o.push(e.type_code() as f64);
        }
        None => o.push(0.0),
    });
    let b = s.base_surface();
    o.push(b.type_code() as f64);
    put_dom(o, &b.natural_uv_domain_with_nurb(k).map_err(|e| HarnessError::new(e.to_string()))?);
    o.push(s.type_code() as f64);
    put_grid(k, o, &Surface::from(s.clone()))
}

fn put_result(k: &Kernel, o: &mut Vec<f64>, r: &Surface) -> Result<(), Fail> {
    o.push(r.type_code() as f64);
    if let SurfaceKind::OffsetSurface(off) = r.kind() {
        put_state(k, o, off)
    } else {
        put_dom(o, &r.natural_uv_domain_with_nurb(k).map_err(|e| HarnessError::new(e.to_string()))?);
        put_grid(k, o, r)
    }
}

/// The entries upstream writes: all, or with `bOnlyUpperHalf` those with `i + j` up to
/// the larger order (the others are untouched caller memory).
fn put_matrix(
    o: &mut Vec<f64>,
    m: &[Vec<Vec3>],
    hu: usize,
    hv: usize,
    upper: bool,
) -> Result<(), Fail> {
    for i in 0..=hu {
        for j in 0..=hv {
            if !upper || i + j <= hu.max(hv) {
                let v = m
                    .get(i)
                    .and_then(|r| r.get(j))
                    .copied()
                    .ok_or_else(|| harness("evaluation matrix too small"))?;
                pv(o, v);
            }
        }
    }
    Ok(())
}

fn flags([ul, vl, upper, nzt]: [bool; 4]) -> EvaluateFlags {
    EvaluateFlags {
        u_side: side(ul),
        v_side: side(vl),
        only_upper_half: upper,
        non_zero_tangents: nzt,
    }
}

fn run_step(
    k: &Kernel,
    s: &mut OffsetSurface,
    step: &StepIn,
    o: &mut Vec<f64>,
) -> Result<(), Fail> {
    match step {
        StepIn::State => {
            o.push(0.0);
            put_state(k, o, s)
        }
        StepIn::Eval {
            uv,
            hu,
            hv,
            ul,
            vl,
            upper,
            nzt,
            via,
        } => {
            let f = flags([*ul, *vl, *upper, *nzt]);
            let r = if *via {
                Surface::from(s.clone()).evaluate(k, p2(*uv), *hu, *hv, f)
            } else {
                s.evaluate_extending(k, p2(*uv), *hu, *hv, f)
            };
            push_res(o, r, |o, m| put_matrix(o, &m, *hu, *hv, *upper))
        }
        StepIn::EvalSimple {
            uv,
            hu,
            hv,
            ul,
            vl,
            upper,
            via,
        } => {
            let f = flags([*ul, *vl, *upper, false]);
            let r = if *via {
                Surface::from(s.clone()).evaluate_simple(k, p2(*uv), *hu, *hv, f)
            } else {
                s.evaluate_simple_extending(k, p2(*uv), *hu, *hv, f)
            };
            push_res(o, r, |o, m| put_matrix(o, &m, *hu, *hv, *upper))
        }
        StepIn::EvalPt { uv, via } => {
            let r = if *via {
                Surface::from(s.clone()).evaluate_point(k, p2(*uv))
            } else {
                s.evaluate_point_extending(k, p2(*uv))
            };
            push_res(o, r, |o, p| {
                pv(o, p);
                Ok(())
            })
        }
        StepIn::Normal { uv, ul, vl, via } => {
            let r = if *via {
                Surface::from(s.clone()).evaluate_normal(k, p2(*uv), side(*ul), side(*vl))
            } else {
                s.evaluate_normal_extending(k, p2(*uv), side(*ul), side(*vl))
            };
            push_res(o, r, |o, n| {
                pv(o, n);
                Ok(())
            })
        }
        StepIn::Knots { param, ivl } => {
            let ivl = ivl.map(|[a, b]| Extent1d::from_min_max(a, b));
            push_res(
                o,
                s.knots(param_of(*param)?, ivl.as_ref()),
                |o, (knots, mults)| {
                    o.push(knots.len() as f64);
                    o.extend(knots);
                    o.push(mults.len() as f64);
                    o.extend(mults.into_iter().map(|m| m as f64));
                    Ok(())
                },
            )
        }
        StepIn::MaxAnal => push_res(o, s.max_analytic_domain(k), |o, d| {
            put_dom(o, &d);
            Ok(())
        }),
        StepIn::BBox { dom, want, expand } => {
            let w = Surface::from(s.clone());
            push_res(
                o,
                w.calculate_bounding_box(k, &dom_of(*dom), *want, *expand),
                |o, b| {
                    if want[0] {
                        let nb = b.normal_box.ok_or_else(|| harness("no normal box"))?;
                        pv(o, nb.min());
                        pv(o, nb.max());
                    }
                    if want[1] {
                        let pb = b.pseudo_box.ok_or_else(|| harness("no pseudo box"))?;
                        for v in pb.basis() {
                            pv(o, *v);
                        }
                        for i in pb.intervals() {
                            o.extend([i.min(), i.max()]);
                        }
                    }
                    if want[2] {
                        let qb = b.polar_box.ok_or_else(|| harness("no polar box"))?;
                        o.push(f64::from(qb.state().code()));
                        for v in qb.basis() {
                            pv(o, *v);
                        }
                        put_dom(o, &qb.domain());
                    }
                    Ok(())
                },
            )
        }
        StepIn::StepInv { dom, p, tol, guess } => push_res(
            o,
            s.step_inversion_extending(k, &dom_of(*dom), v3(*p), *tol, guess.map(p2)),
            |o, (uv, loc)| {
                o.extend([uv.x, uv.y, f64::from(loc as i32)]);
                Ok(())
            },
        ),
        StepIn::Reverse { param } => {
            let p = param_of(*param)?;
            push_res(o, s.reverse(k, p), |_, ()| Ok(()))
        }
        StepIn::Swap => push_res(o, s.swap_uv(k), |_, ()| Ok(())),
        StepIn::Transform {
            origin,
            x,
            y,
            scale,
        } => {
            let ax = Axis2Placement::new(v3(*origin), v3(*x), v3(*y));
            push_res(o, s.transform(k, &ax, scale.map(v3)), |_, ()| Ok(()))
        }
        StepIn::Trim { dom } => {
            let mut d = dom_of(*dom);
            let r = s.trim_with_domain(k, &mut d);
            push_res(o, r, |o, ()| {
                put_dom(o, &d);
                Ok(())
            })
        }
        StepIn::Split { t, param } => {
            let p = param_of(*param)?;
            push_res(o, s.split_at(k, *t, p), |o, (l, r)| {
                put_result(k, o, &l)?;
                put_result(k, o, &r)
            })
        }
        StepIn::CreateOffset { d } => {
            push_res(o, s.create_offset_surface(k, *d), |o, r| put_result(k, o, &r))
        }
        StepIn::Copy => push_res(o, s.copy(k), |o, c| put_result(k, o, &Surface::from(c))),
        StepIn::Eq { other } => match rust_off(k, other)? {
            Ok(other) => {
                o.push(0.0);
                let eq = s.approx_eq(k, &Surface::from(other));
                let w = Surface::from(s.clone());
                let self_eq = w.approx_eq(k, &w);
                for r in [eq, self_eq] {
                    match r {
                        Ok(b) => o.push(flag(b)),
                        Err(e) => {
                            return Err(defer(&e).unwrap_or_else(|| {
                                Fail::Harness(harness(format!("operator== failed: {e}")))
                            }))
                        }
                    }
                }
                Ok(())
            }
            Err(e) => {
                if let Some(d) = defer(&e) {
                    Err(d)
                } else {
                    o.push(status(&e));
                    Ok(())
                }
            }
        },
        StepIn::SetDist { d } => {
            s.set_offset_distance(*d);
            o.push(0.0);
            Ok(())
        }
        StepIn::SetAllow { b } => {
            s.set_allow_extension(*b);
            o.push(0.0);
            Ok(())
        }
        StepIn::IsPlanar { tol } => {
            let w = Surface::from(s.clone());
            match w.is_planar(k, *tol) {
                Ok(pl) => {
                    o.extend([0.0, flag(pl.is_planar)]);
                    pv(o, pl.point);
                    pv(o, pl.normal);
                    o.push(pl.deviation);
                    Ok(())
                }
                Err(e) => Err(defer(&e)
                    .unwrap_or_else(|| Fail::Harness(harness(format!("IsPlanar failed: {e}"))))),
            }
        }
        StepIn::IsKindOf { t } => {
            o.extend([0.0, flag(OffsetSurface::is_kind_of(*t))]);
            Ok(())
        }
        StepIn::BaseOffset { d, tol } => push_res(
            o,
            s.base_surface().create_offset_surface(k, *d, *tol),
            // (a NULL result writes only the status, as the shim)
            |o, r| r.as_ref().map_or(Ok(()), |r| put_result(k, o, r)),
        ),
        StepIn::AssertValid { level } => put_assert_valid(k, s, *level, o),
    }
}

fn av_text(o: &mut Vec<f64>, s: Option<&str>) {
    o.push(s.map_or(0, str::len) as f64);
    if let Some(s) = s {
        o.extend(s.bytes().map(f64::from));
    }
}

/// A report object as the shim writes it: `0` null, `1` the checked offset, `9` another.
fn av_id(key: Option<ObjectKey>) -> f64 {
    match key {
        None => 0.0,
        Some(ObjectKey(1)) => 1.0,
        Some(_) => 9.0,
    }
}

/// `AssertValid` through `SmSurface*` on the offset (key `1`): `0 ok n reports last_add`.
fn put_assert_valid(
    k: &Kernel,
    s: &OffsetSurface,
    level: u8,
    o: &mut Vec<f64>,
) -> Result<(), Fail> {
    let w = Surface::from(s.clone());
    let mut list = AssertArray::new();
    let ok = w
        .assert_valid(k, Some(&mut list), level, ObjectKey(1))
        .map_err(|e| {
            defer(&e).unwrap_or_else(|| Fail::Harness(harness(format!("AssertValid failed: {e}"))))
        })?;
    o.extend([0.0, flag(ok), list.len() as f64]);
    for r in list.reports() {
        o.extend([
            r.reporting_type as f64,
            r.list_index as f64,
            r.test_index as f64,
            f64::from(r.assert_type.code()),
            flag(r.ok),
            av_id(r.reporter),
            av_id(r.owner),
            av_id(r.other),
            r.owner_type as f64,
            r.other_type as f64,
            r.tol3d,
            r.value,
            r.dist3d,
        ]);
        av_text(o, Some(r.name));
        av_text(o, r.message.as_deref());
        av_text(o, r.message_for_file);
        av_text(o, r.owner_type_string);
        av_text(o, r.other_type_string);
    }
    o.push(flag(list.last_add()));
    Ok(())
}

fn rust_script(off: &OffIn, steps: &[StepIn]) -> Result<Vec<f64>, Fail> {
    let k = Kernel::default();
    let mut o = Vec::new();
    let mut s = match rust_off(&k, off)? {
        Ok(s) => s,
        Err(e) => {
            if let Some(d) = defer(&e) {
                return Err(d);
            }
            o.push(status(&e));
            return Ok(o);
        }
    };
    o.push(0.0);
    for step in steps {
        run_step(&k, &mut s, step, &mut o)?;
    }
    Ok(o)
}

fn rust_read(binary: bool, version: u32, bytes: &[u8]) -> Result<Vec<f64>, Fail> {
    let k = Kernel::default();
    let ft = if binary {
        FileType::Binary
    } else {
        FileType::Ascii
    };
    let mut db = DatabaseReader::with_stream(ft, InputStream::new(bytes.to_vec()));
    let mut o = Vec::new();
    push_res(
        &mut o,
        OffsetSurface::read_from_db(&k, &mut db, u64::from(version)),
        |o, s| put_state(&k, o, &s),
    )?;
    Ok(o)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let r = match i {
        Inputs::Script { off, steps } => rust_script(off, steps),
        Inputs::ReadDb {
            binary,
            version,
            bytes,
            ..
        } => {
            let bytes = bytes.as_ref().ok_or_else(|| harness("read_db: no bytes"))?;
            rust_read(*binary, *version, bytes)
        }
    };
    match r {
        Ok(rec) => Ok(Output::new().values("out", rec)),
        Err(Fail::Deferred(d)) => Ok(*d),
        Err(Fail::Harness(e)) => Err(e),
    }
}

// ---- generated cases --------------------------------------------------------------------------

fn gen_knots(rng: &mut Rng, n: usize, p: usize, a: f64, b: f64) -> Vec<f64> {
    let mut inner: Vec<f64> = (0..n - p).map(|_| rng.uniform(a, b)).collect();
    inner.sort_by(f64::total_cmp);
    // keep interior knots apart from the ends and from each other (or repeat one)
    let span = b - a;
    for k in &mut inner {
        *k = k.clamp(a + 1e-3 * span, b - 1e-3 * span);
    }
    if p >= 2 && inner.len() >= 2 && rng.chance(0.15) {
        let i = rng.below(inner.len() as u64 - 1) as usize;
        if let Some(&x) = inner.get(i) {
            if let Some(y) = inner.get_mut(i + 1) {
                *y = x;
            }
        }
    }
    let mut u = vec![a; p + 1];
    u.extend(inner);
    u.extend(vec![b; p + 1]);
    u
}

type Frame3 = [[f64; 3]; 3];

fn frame(rng: &mut Rng) -> Frame3 {
    let e1 = rev::unit_vec(rng);
    let mut t = rev::unit_vec(rng);
    while rev::cross3(e1, t).iter().map(|c| c * c).sum::<f64>() < 0.1 {
        t = rev::unit_vec(rng);
    }
    let e3 = rev::normalize(rev::cross3(e1, t));
    let e2 = rev::cross3(e3, e1);
    [e1, e2, e3]
}

fn at(c: [f64; 3], f: &Frame3, a: f64, b: f64, h: f64) -> [f64; 3] {
    let [e1, e2, e3] = f;
    rev::add(rev::add(rev::add(c, *e1, a), *e2, b), *e3, h)
}

/// The kinds of generated B-spline bases.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Smooth,
    /// Row `i = 0` collapsed to a point (a pole on the `u = UMin` side).
    PoleU,
    /// Column `j = m` collapsed (a pole on the `v = VMax` side).
    PoleV,
    /// Closed in u with matching end tangents (periodic upstream).
    ClosedU,
    Flat,
    /// All control points on a line: no normal anywhere.
    Collinear,
}

fn gen_bss(rng: &mut Rng, shape: Shape, scale: f64) -> BssIn {
    let flat = shape == Shape::Flat;
    let p = if flat { 1 } else { 1 + rng.below(3) as usize };
    let q = if flat { 1 } else { 1 + rng.below(3) as usize };
    let mut n = p + rng.below(4) as usize;
    let m = q + rng.below(4) as usize;
    if shape == Shape::ClosedU {
        n = n.max(p + 2).max(3);
    }
    let f = frame(rng);
    let c = rng.point3(-2.0 * scale, 2.0 * scale);
    let (amp, f1, f2, ph) = (
        rng.uniform(0.0, 0.6) * scale,
        rng.uniform(0.5, 3.0),
        rng.uniform(0.5, 3.0),
        rng.uniform(0.0, 3.0),
    );
    let rational = !flat && rng.chance(0.2);
    let mut net = Vec::new();
    for i in 0..=n {
        let mut row = Vec::new();
        for j in 0..=m {
            let (s, t) = (i as f64 / n as f64, j as f64 / m as f64);
            let pt = match shape {
                Shape::Smooth | Shape::Flat => {
                    let h = if flat {
                        0.0
                    } else {
                        amp * (f1 * s + ph).sin() * (f2 * t).cos()
                    };
                    at(c, &f, 2.0 * scale * s, 1.5 * scale * t, h)
                }
                Shape::PoleU => {
                    // a dome: radius growing from 0 at i = 0
                    let r = scale * s;
                    let th = std::f64::consts::PI * t;
                    at(c, &f, r * th.cos(), r * th.sin(), scale * (1.0 - s * s))
                }
                Shape::PoleV => {
                    let r = scale * (1.0 - t);
                    let th = 1.5 * s;
                    at(c, &f, r * th.cos(), r * th.sin(), 0.8 * scale * t)
                }
                Shape::ClosedU => {
                    // a polygon around a circle, the last row equal to the first, so
                    // the end tangents match
                    let k = if i == n { 0 } else { i };
                    let th = 2.0 * std::f64::consts::PI * k as f64 / n as f64;
                    at(c, &f, scale * th.cos(), scale * th.sin(), 1.5 * scale * t)
                }
                Shape::Collinear => at(c, &f, scale * (s + 0.5 * t), 0.0, 0.0),
            };
            let w = if rational { rng.uniform(0.5, 2.0) } else { NOW };
            row.push([pt[0], pt[1], pt[2], w]);
        }
        net.push(row);
    }
    if shape == Shape::PoleU {
        if let Some(first) = net.first().and_then(|r| r.first()).copied() {
            if let Some(row) = net.first_mut() {
                for pt in row.iter_mut() {
                    *pt = [first[0], first[1], first[2], pt[3]];
                }
            }
        }
    }
    if shape == Shape::PoleV {
        // every row ends at the apex
        let apex = at(c, &f, 0.0, 0.0, 0.8 * scale);
        for row in &mut net {
            if let Some(last) = row.last_mut() {
                *last = [apex[0], apex[1], apex[2], last[3]];
            }
        }
    }
    let (a, len) = if rng.chance(0.5) {
        (0.0, 1.0)
    } else {
        (rng.uniform(-3.0, 3.0), rng.uniform(0.5, 4.0))
    };
    let u = gen_knots(rng, n, p, a, a + len);
    let (a2, len2) = if rng.chance(0.5) {
        (0.0, 1.0)
    } else {
        (rng.uniform(-3.0, 3.0), rng.uniform(0.5, 4.0))
    };
    let v = gen_knots(rng, m, q, a2, a2 + len2);
    // uniform knots keep the closed polygon's end tangents equal
    let u = if shape == Shape::ClosedU {
        let mut u = vec![a; p + 1];
        for i in 1..=(n - p) {
            u.push(a + len * i as f64 / (n - p + 1) as f64);
        }
        u.extend(vec![a + len; p + 1]);
        u
    } else {
        u
    };
    BssIn {
        oob: rng.chance(0.1),
        form: default_form(),
        net,
        p,
        q,
        u,
        v,
    }
}

fn gen_shape(rng: &mut Rng) -> Shape {
    match rng.below(20) {
        0..=8 => Shape::Smooth,
        9..=11 => Shape::PoleU,
        12..=13 => Shape::PoleV,
        14..=15 => Shape::ClosedU,
        16..=18 => Shape::Flat,
        _ => Shape::Collinear,
    }
}

fn gen_bss_any(rng: &mut Rng, scale: f64) -> BssIn {
    let shape = gen_shape(rng);
    gen_bss(rng, shape, scale)
}

fn gen_scale(rng: &mut Rng) -> f64 {
    match rng.below(20) {
        0 => 1e-3,
        1 => 1e3,
        2 => 10.0,
        // extreme magnitudes (no coordinate cap on the C++ side)
        3 => 1e-9,
        4 => 1e9,
        5 => 1e150,
        _ => 1.0,
    }
}

/// A surface of revolution whose line or circle generator is shrunk to `1e-9` .. `1e-12`
/// of its size (no minimum size on the offset's C++ side).
fn tiny_revolution(rng: &mut Rng) -> rev::SurfIn {
    let mut s = rev::gen_surf(rng);
    let f = *rng.choose(&[1e-9, 1e-12]).unwrap_or(&1e-9);
    match &mut s.gen {
        rev::GenIn::Circle { r, .. } => *r *= f,
        rev::GenIn::Line { a, b } => {
            for (bi, ai) in b.iter_mut().zip(a.iter()) {
                *bi = *ai + (*bi - *ai) * f;
            }
        }
        rev::GenIn::BSpline(_) | rev::GenIn::Hyperbola { .. } => {}
    }
    s
}

fn gen_base(rng: &mut Rng, scale: f64, depth: usize) -> SrfIn {
    match rng.below(20) {
        0..=8 => SrfIn::Bspline(gen_bss_any(rng, scale)),
        9..=10 => SrfIn::Plane {
            o: rng.point3(-2.0 * scale, 2.0 * scale),
            n: rev::unit_vec(rng),
        },
        11..=13 => SrfIn::Revolution(rev::gen_surf(rng)),
        14 => SrfIn::Revolution(tiny_revolution(rng)),
        15 => SrfIn::Bounded {
            basis: Box::new(SrfIn::Bspline(gen_bss(rng, Shape::Smooth, scale))),
            implicit: rng.chance(0.5),
        },
        16 => SrfIn::Step {
            base: Box::new(SrfIn::Bspline(gen_bss_any(rng, scale))),
        },
        _ if depth < 2 => SrfIn::Offset(Box::new(gen_off(rng, scale, depth + 1))),
        _ => SrfIn::Bspline(gen_bss_any(rng, scale)),
    }
}

fn gen_dist(rng: &mut Rng, scale: f64) -> f64 {
    match rng.below(20) {
        0..=1 => 0.0,
        2 => rng.uniform(-1e-9, 1e-9),
        3..=4 => rng.uniform(2.0, 5.0) * scale * if rng.chance(0.5) { 1.0 } else { -1.0 },
        _ => rng.uniform(-1.0, 1.0) * scale,
    }
}

fn gen_off(rng: &mut Rng, scale: f64, depth: usize) -> OffIn {
    let base = gen_base(rng, scale, depth);
    let sing = match rng.below(10) {
        0..=1 => SS_UNKNOWN,
        2..=6 => 0,
        _ => rng.below(16) as u32,
    };
    OffIn {
        d: gen_dist(rng, scale),
        owns: rng.chance(0.85),
        sing,
        allow: rng.chance(0.3),
        base,
    }
}

/// The base's natural domain and size (the distance between its domain corners'
/// points, at least `1e-9`), built on the Rust side.
fn base_info(k: &Kernel, off: &OffIn) -> Option<([f64; 4], f64)> {
    let s = rust_srf(k, &off.base).ok()?.ok()?;
    // (runner helper that does not return Result)
    let d = s.natural_uv_domain_if_built();
    let r = [d.min().x, d.min().y, d.max().x, d.max().y];
    if !(r.iter().all(|c| c.is_finite() && c.abs() < 1e5) && r[0] < r[2] && r[1] < r[3]) {
        return None;
    }
    let size = match (s.evaluate_point(k, d.min()), s.evaluate_point(k, d.max())) {
        (Ok(a), Ok(b)) => a.distance(b),
        _ => 0.0,
    };
    Some((r, size.max(1e-9)))
}

fn gen_uv(rng: &mut Rng, d: [f64; 4]) -> [f64; 2] {
    let [u0, v0, u1, v1] = d;
    let (lu, lv) = (u1 - u0, v1 - v0);
    let pick = |rng: &mut Rng, a: f64, b: f64, len: f64| match rng.below(20) {
        0..=10 => rng.uniform(a, b),
        11..=12 => a,
        13..=14 => b,
        15..=17 => {
            if rng.chance(0.5) {
                a - rng.uniform(0.0, 1.5) * len
            } else {
                b + rng.uniform(0.0, 1.5) * len
            }
        }
        _ => a + len * f64::from(rng.below(5) as u32) / 4.0,
    };
    [pick(rng, u0, u1, lu), pick(rng, v0, v1, lv)]
}

fn gen_inside(rng: &mut Rng, d: [f64; 4]) -> [f64; 2] {
    [rng.uniform(d[0], d[2]), rng.uniform(d[1], d[3])]
}

fn gen_subdomain(rng: &mut Rng, d: [f64; 4]) -> [f64; 4] {
    let [a, b] = gen_inside(rng, d);
    let [c, e] = gen_inside(rng, d);
    [
        a.min(c),
        b.min(e),
        a.max(c).max(a.min(c) + 1e-3 * (d[2] - d[0])),
        b.max(e).max(b.min(e) + 1e-3 * (d[3] - d[1])),
    ]
}

fn gen_orders(rng: &mut Rng) -> (usize, usize, bool) {
    let upper = rng.chance(0.35);
    if rng.chance(0.05) {
        // more than MAX_DERIVS: SM_ERR before anything is written (orders up to 8)
        return (
            rng.below(3) as usize * 3 + 2,
            rng.below(3) as usize * 3 + 2,
            false,
        );
    }
    let pairs: &[(usize, usize)] = if upper {
        &[
            (0, 0),
            (1, 1),
            (2, 2),
            (3, 3),
            (1, 0),
            (2, 0),
            (3, 0),
            (2, 1),
            (3, 1),
            (3, 2),
            (2, 3),
            (1, 2),
        ]
    } else {
        &[
            (0, 0),
            (1, 0),
            (0, 1),
            (1, 1),
            (2, 0),
            (0, 2),
            (2, 1),
            (1, 2),
            (3, 0),
            (0, 3),
        ]
    };
    let &(hu, hv) = rng.choose(pairs).unwrap_or(&(1, 1));
    (hu, hv, upper)
}

/// One op; extensions are a fraction of the base `size` (tiny relative distances make
/// upstream's G1/G2 extension search loop forever, HANG-6).
fn gen_step(rng: &mut Rng, d: [f64; 4], scale: f64, size: f64, can_via: bool) -> StepIn {
    let via = can_via && rng.chance(0.3);
    match rng.below(100) {
        0..=24 => {
            let (hu, hv, upper) = gen_orders(rng);
            StepIn::Eval {
                uv: gen_uv(rng, d),
                hu,
                hv,
                ul: rng.chance(0.8),
                vl: rng.chance(0.8),
                upper,
                nzt: rng.chance(0.75),
                via,
            }
        }
        25..=34 => {
            let (hu, hv, upper) = gen_orders(rng);
            StepIn::EvalSimple {
                uv: gen_uv(rng, d),
                hu,
                hv,
                ul: rng.chance(0.8),
                vl: rng.chance(0.8),
                upper,
                via,
            }
        }
        35..=46 => StepIn::EvalPt {
            uv: gen_uv(rng, d),
            via,
        },
        47..=54 => StepIn::Normal {
            uv: gen_uv(rng, d),
            ul: rng.chance(0.8),
            vl: rng.chance(0.8),
            via,
        },
        55..=57 => StepIn::Knots {
            param: rng.below(2) as u8,
            ivl: rng.chance(0.3).then(|| {
                let [a, b] = gen_inside(rng, d);
                [a.min(b), a.max(b)]
            }),
        },
        58..=59 => StepIn::MaxAnal,
        60..=66 => StepIn::BBox {
            dom: if rng.chance(0.85) {
                d
            } else {
                gen_subdomain(rng, d)
            },
            want: [rng.chance(0.7), rng.chance(0.6), rng.chance(0.5)],
            expand: rng.chance(0.7),
        },
        67..=70 => StepIn::StepInv {
            dom: d,
            p: rng.point3(-3.0 * scale, 3.0 * scale),
            tol: if rng.chance(0.5) {
                1e-6
            } else {
                rng.uniform(1e-4, 0.5)
            },
            guess: rng.chance(0.4).then(|| gen_inside(rng, d)),
        },
        71..=72 => StepIn::Reverse {
            param: rng.below(2) as u8,
        },
        73..=74 => StepIn::Swap,
        75..=76 => {
            let x = rev::unit_vec(rng);
            let mut y = rev::unit_vec(rng);
            while rev::cross3(x, y).iter().map(|c| c * c).sum::<f64>() < 0.01 {
                y = rev::unit_vec(rng);
            }
            StepIn::Transform {
                origin: rng.point3(-2.0, 2.0),
                x,
                y,
                scale: match rng.below(4) {
                    0 => None,
                    1 => {
                        let s = rng.uniform(0.2, 3.0);
                        Some([s, s, s])
                    }
                    // unit, tiny, zero and negative uniform scales
                    2 => {
                        let s = *rng.choose(&[1.0, 1e-6, 0.0, -1e-3]).unwrap_or(&1.0);
                        Some([s, s, s])
                    }
                    _ => Some([
                        rng.uniform(0.2, 3.0),
                        rng.uniform(0.2, 3.0),
                        rng.uniform(0.2, 3.0),
                    ]),
                },
            }
        }
        77..=79 => StepIn::Trim {
            dom: if rng.chance(0.7) {
                gen_subdomain(rng, d)
            } else {
                let lu = d[2] - d[0];
                let lv = d[3] - d[1];
                [
                    d[0] - rng.uniform(0.0, 0.5) * lu,
                    d[1],
                    d[2] + rng.uniform(0.0, 0.5) * lu,
                    d[3] + rng.uniform(0.0, 0.2) * lv,
                ]
            },
        },
        80..=82 => {
            let param = rng.below(2) as u8;
            let (a, b) = if param == 0 {
                (d[0], d[2])
            } else {
                (d[1], d[3])
            };
            StepIn::Split {
                t: rng.uniform(a + 0.05 * (b - a), b - 0.05 * (b - a)),
                param,
            }
        }
        83..=85 => StepIn::CreateOffset {
            d: if rng.chance(0.2) {
                0.0
            } else {
                rng.uniform(-1.0, 1.0) * scale
            },
        },
        89..=91 => StepIn::Copy,
        92..=93 => StepIn::Eq {
            other: gen_off(rng, scale, 2),
        },
        94 => StepIn::SetDist {
            d: gen_dist(rng, scale),
        },
        95 => StepIn::SetAllow { b: rng.chance(0.5) },
        96..=97 => StepIn::IsPlanar {
            tol: *rng.choose(&[0.0, 1e-9, 1e-6, 1e-3, 0.1]).unwrap_or(&1e-6),
        },
        // (relative to the base's size, as in surface.surf_of_revolution_p2: an offset
        // far larger than a revolution's generator degenerates, and upstream's
        // `CreateSimpleOffset` then copies an uninitialised `dMaxGap3d`)
        98 => StepIn::BaseOffset {
            d: if rng.chance(0.1) {
                0.0
            } else {
                rng.uniform(-1.0, 1.0) * size
            },
            tol: *rng.choose(&[0.0, 1e-6, 1e-3]).unwrap_or(&1e-3) * scale,
        },
        _ => StepIn::IsKindOf {
            t: *rng
                .choose(&[OFFSET_SURFACE_TYPE, 15000, 0, 15001, 15400, 15100])
                .unwrap_or(&0),
        },
    }
}

/// A script on `off` whose `via` steps run only while extension is off (no extension
/// can then be stored).
fn gen_steps(rng: &mut Rng, off: &OffIn, d: [f64; 4], scale: f64, size: f64) -> Vec<StepIn> {
    let mut steps = Vec::new();
    if rng.chance(0.5) {
        steps.push(StepIn::State);
    }
    let mut allow = off.allow;
    let n = 1 + rng.below(5) as usize;
    for _ in 0..n {
        let step = gen_step(rng, d, scale, size, !allow);
        if let StepIn::SetAllow { b } = step {
            allow = b;
        }
        steps.push(step);
    }
    if rng.chance(0.4) {
        steps.push(StepIn::State);
    }
    steps
}

fn gen_case(rng: &mut Rng) -> Option<Inputs> {
    let k = Kernel::default();
    let scale = gen_scale(rng);
    let off = gen_off(rng, scale, 0);
    let (d, size) = base_info(&k, &off)?;
    if rng.chance(0.08) {
        let binary = rng.chance(0.5);
        let version = *rng.choose(&[30, 33])?;
        return Some(Inputs::ReadDb {
            off,
            binary,
            version,
            bytes: None,
        });
    }
    let steps = gen_steps(rng, &off, d, scale, size);
    Some(Inputs::Script { off, steps })
}

/// Generated `AssertValid` cases.
const ASSERT_VALID_GENERATED: usize = 100;

/// A base for an `AssertValid` case. The self-intersection test runs a full
/// surface/surface intersection of the offset with itself, which takes seconds to
/// minutes in C++ for the curved, tiny-scale and huge-scale bases of `gen_base`
/// (one 1e150-scale check took 148 s), so these bases are planes, flat and low-degree
/// curved B-splines (some with a pole) and offsets of them.
fn gen_av_base(rng: &mut Rng, scale: f64, depth: usize) -> SrfIn {
    match rng.below(10) {
        0..=2 => SrfIn::Plane {
            o: rng.point3(-2.0 * scale, 2.0 * scale),
            n: rev::unit_vec(rng),
        },
        3..=5 => SrfIn::Bspline(gen_bss(rng, Shape::Flat, scale)),
        6 => SrfIn::Bspline(gen_bss(rng, Shape::Smooth, scale)),
        7 => SrfIn::Bspline(gen_bss(rng, Shape::PoleU, scale)),
        _ if depth < 1 => SrfIn::Offset(Box::new(gen_av_off(rng, scale, depth + 1))),
        _ => SrfIn::Bspline(gen_bss(rng, Shape::Flat, scale)),
    }
}

/// An offset for an `AssertValid` case: owning or shared base, no extension.
fn gen_av_off(rng: &mut Rng, scale: f64, depth: usize) -> OffIn {
    let base = gen_av_base(rng, scale, depth);
    let d = match rng.below(10) {
        0 => 0.0,
        1 => rng.uniform(2.0, 5.0) * scale,
        _ => rng.uniform(-0.5, 0.5) * scale,
    };
    OffIn {
        d,
        owns: rng.chance(0.75),
        sing: if rng.chance(0.2) { SS_UNKNOWN } else { 0 },
        allow: false,
        base,
    }
}

/// An `AssertValid` script: an offset, an optional edit (distance, reversal, swap), then
/// the check at `SM_LEVEL_GIVEN` (mostly) or a level running the inherited base checks.
fn gen_assert_valid_case(rng: &mut Rng) -> Option<Inputs> {
    let scale = *rng.choose(&[1.0, 1.0, 1.0, 10.0, 1e-3, 1e3, 1e6])?;
    let off = gen_av_off(rng, scale, 0);
    let mut steps = Vec::new();
    match rng.below(8) {
        0 => steps.push(StepIn::SetDist {
            d: rng.uniform(-0.5, 0.5) * scale,
        }),
        1 => steps.push(StepIn::Reverse {
            param: *rng.choose(&[0, 1])?,
        }),
        2 => steps.push(StepIn::Swap),
        _ => {}
    }
    let level = if rng.chance(0.75) { 4 } else { rng.below(3) as u8 };
    steps.push(StepIn::AssertValid { level });
    Some(Inputs::Script { off, steps }).filter(admits)
}

/// Fills a fixture's placeholders from the base built on the Rust side: a `dom` of
/// `"NAT"` (the base's natural domain), `"SUB"` (its middle half) or `"OUT"` (a quarter
/// larger on every side); a `uv` of `"MID"`, `"VMIN"` or `"VMAX"` (the middle of the
/// domain, of its `v = VMin` side, of its `v = VMax` side); a `p` of `"ON"` (the offset's
/// point at `(0.4, 0.6)` of that domain, built without singularities).
fn fill_placeholders(case: &mut Case) -> Result<(), HarnessError> {
    let keys = ["dom", "uv", "p"];
    let Some(steps) = case.inputs.get("steps").and_then(Value::as_array) else {
        return Ok(());
    };
    let wanted = steps
        .iter()
        .any(|s| keys.iter().any(|k| s.get(k).is_some_and(Value::is_string)));
    if !wanted {
        return Ok(());
    }
    let bad = |what: &str| harness(format!("case {}: {what}", case.name));
    let off: OffIn = serde_json::from_value(
        case.inputs
            .get("off")
            .cloned()
            .ok_or_else(|| bad("no off"))?,
    )
    .map_err(|e| bad(&e.to_string()))?;
    let k = Kernel::default();
    let base = rust_srf(&k, &off.base)?.map_err(|e| bad(&format!("base: {e}")))?;
    let d = base.natural_uv_domain_with_nurb(&k).map_err(|e| HarnessError::new(e.to_string()))?;
    let (u0, v0, u1, v1) = (d.min().x, d.min().y, d.max().x, d.max().y);
    let (lu, lv) = (u1 - u0, v1 - v0);
    let on = || -> Result<[f64; 3], HarnessError> {
        let plain = OffIn {
            sing: 0,
            ..off.clone()
        };
        let s = rust_off(&k, &plain)?.map_err(|e| bad(&format!("offset: {e}")))?;
        let p = s
            .evaluate_point(&k, Point2::new(u0 + 0.4 * lu, v0 + 0.6 * lv))
            .map_err(|e| bad(&format!("point: {e}")))?;
        Ok([p.x, p.y, p.z])
    };
    let mut steps = steps.clone();
    for step in &mut steps {
        for key in keys {
            let Some(tag) = step.get(key).and_then(Value::as_str).map(str::to_owned) else {
                continue;
            };
            let v = match tag.as_str() {
                "NAT" => serde_json::json!([u0, v0, u1, v1]),
                "SUB" => serde_json::json!([
                    u0 + 0.25 * lu,
                    v0 + 0.25 * lv,
                    u1 - 0.25 * lu,
                    v1 - 0.25 * lv
                ]),
                "OUT" => serde_json::json!([
                    u0 - 0.25 * lu,
                    v0 - 0.25 * lv,
                    u1 + 0.25 * lu,
                    v1 + 0.25 * lv
                ]),
                "MID" => serde_json::json!([u0 + 0.5 * lu, v0 + 0.5 * lv]),
                "VMIN" => serde_json::json!([u0 + 0.5 * lu, v0]),
                "VMAX" => serde_json::json!([u0 + 0.5 * lu, v1]),
                "ON" => serde_json::json!(on()?),
                other => return Err(bad(&format!("unknown placeholder {other}"))),
            };
            if let Some(slot) = step.get_mut(key) {
                *slot = v;
            }
        }
    }
    if let Some(slot) = case.inputs.get_mut("steps") {
        *slot = Value::Array(steps);
    }
    Ok(())
}

/// A generated case's draw: the first [`gen_case`] inside the input contract.
fn draw(rng: &mut Rng) -> Option<Inputs> {
    (0..16).find_map(|_| gen_case(rng).filter(admits))
}

/// A generated `read_db` draw (no record yet). Its case reads the record upstream's
/// `WriteToDB` wrote of the drawn offset, which solidcore has no writer for (#2857): those
/// cases are frozen in `fixtures/surface.offset_surface/written_records.json` under their
/// generated names and the generated index is skipped.
fn is_read_draw(i: &Inputs) -> bool {
    matches!(i, Inputs::ReadDb { bytes: None, .. })
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.offset_surface"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        draw(rng)
            .filter(|i| !is_read_draw(i))
            .and_then(|i| serde_json::to_value(i).ok())
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let av = CaseContext {
            seeds: ctx.seeds.max(ASSERT_VALID_GENERATED),
            ..ctx.clone()
        };
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        let mut cases = ctx.fixture_cases(self.id())?;
        // A skipped index (a `read_db` draw) is a fixture case of the same name. So is an
        // index whose frozen `read_db` case the C++ shim's run-time refusal of an earlier
        // draw reached (that refusal checked the actual surfaces, so the contract above
        // cannot port it): the fixture keeps the index.
        let frozen: std::collections::BTreeSet<String> =
            cases.iter().map(|c| c.name.clone()).collect();
        let generated = ctx.generated_cases(self.id(), |rng| {
            let i = draw(rng)?;
            Some(if is_read_draw(&i) {
                Value::Null
            } else {
                serde_json::to_value(i).unwrap_or(Value::Null)
            })
        });
        cases.extend(
            generated
                .into_iter()
                .filter(|c| !c.inputs.is_null() && !frozen.contains(&c.name)),
        );
        for mut c in av.generated_cases("surface.offset_surface:assert-valid", |rng| {
            (0..16)
                .find_map(|_| gen_assert_valid_case(rng))
                .and_then(|i| serde_json::to_value(i).ok())
        }) {
            c.name = c.name.replacen("gen", "av", 1);
            c.unit = self.id().to_string();
            cases.push(c);
        }
        for case in &mut cases {
            fill_placeholders(case)?;
        }
        Ok(cases)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}
