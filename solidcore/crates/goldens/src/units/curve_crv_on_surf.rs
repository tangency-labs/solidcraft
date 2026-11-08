//! Runner for unit `curve.crv_on_surf`: `SmCrvOnSurf`, a 2D UV curve traced through a
//! surface.
//!
//! Ported from upstream `source/SMLib/src/SmCrvOnSurf.cpp`; checked against its golden.
//! Rust: `solidcore::curve::crv_on_surf` (the `&mut` methods, so the caches evolve as
//! upstream's; `Info` and `Equal` go through the [`Curve`] dispatch).
//!
//! A case is either an object (`cos`: `uv` (`bspline` or `line`), B-spline `srf` (inline or
//! an upstream `.sms` file), optional `offset`, `domain`, `owner`, `cache` `{need,
//! breaks}`) with an op list run in order on it, or a database record (`read_db`). Inline
//! UV B-splines and surfaces accept `borrow_nurb` to preserve raw knots. The Rust side
//! produces a flat record (every op's status, `0` = `SM_SUCCESS`, else the `SmStatus`;
//! `1007` for a Rust `InvalidInput`, `-1` for a Rust error upstream cannot produce, then
//! its outputs), compared as `out`.
//!
//! Knot and drop operations compare the break-cache state as well as their answers:
//! a local solve may leave breaks uncached, while a later global solve populates
//! them through the base curve's virtual knot queries. Every run has at least
//! [`MIN_GENERATED`] generated cases. Any genuinely unported downstream call still
//! defers the whole case.
//!
//! Deviations: the runner-local SMS reader validates counts below the exclusive
//! `2^usize::BITS` bound, checks dimension and payload arithmetic and the available
//! tokens before constructing the net, and grows storage incrementally. Valid point
//! order, weights, knots, lazy float parsing and path-bearing errors are unchanged.

#[path = "fixtures/crv_on_surf_sms.rs"]
mod sms;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::crv_on_surf::CrvOnSurf;
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::geom::assert_array::{AssertArray, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::global_solver::SolverOperation;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BoundingBoxRequest};
use solidcore::nurbs::b_spline_curve_n_lib::TrimMode;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::math::{NOW, NOZ};
use solidcore::nurbs::srf_basic::Surface;
use solidcore::surface::offset_surface::{OffsetSurface, SS_UNKNOWN};
use solidcore::surface::surface::{Surface as SmSurface, SurfaceKind};
use solidcore::topology::brep::validation::curve_assert_valid;
use solidcore::topology::database_io::{DatabaseReader, DatabaseWriter, FileType, InputStream};
use solidcore::topology::geometry_write::write_curve;
use solidcore::SolidError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::assert_reports::put_assert_reports;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `curve.crv_on_surf` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

/// A 2D B-spline UV curve: control points `[x, y, w]` (`w` = `NOW` when
/// non-rational; `x`, `y` homogeneous otherwise), degree, knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct UvBspline {
    cpts: Vec<[f64; 3]>,
    p: usize,
    u: Vec<f64>,
    /// Borrow the supplied NURB without constructor knot repair on both sides.
    #[serde(default)]
    borrow_nurb: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum UvIn {
    Bspline(UvBspline),
    /// The UV-line constructor `[x0, y0, x1, y1]`.
    Line([f64; 4]),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InlineSrf {
    /// Control net `[x, y, z, w]` (`w` = `NOW` when non-rational), `net[i][j]`, `i` in u.
    net: Vec<Vec<[f64; 4]>>,
    p: usize,
    q: usize,
    u: Vec<f64>,
    v: Vec<f64>,
    /// Borrow the supplied NURB without constructor knot repair on both sides.
    #[serde(default)]
    borrow_nurb: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum SrfIn {
    /// An upstream `.sms` surface file (path relative to the repository root).
    File {
        sms: String,
    },
    Inline(InlineSrf),
}

/// A real `SmOffsetSurface` wrapped around the B-spline `srf`; ownership controls
/// whether the wrapper deletes its base.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OffsetIn {
    distance: f64,
    #[serde(default)]
    owns_base: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheIn {
    need: bool,
    breaks: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CosIn {
    uv: UvIn,
    srf: SrfIn,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    offset: Option<OffsetIn>,
    domain: Option<[f64; 4]>,
    #[serde(default)]
    owner: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cache: Option<CacheIn>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Info {
        types: Vec<i64>,
    },
    State,
    /// `AssertValid` at `level` (0..2, 4 = `SM_LEVEL_GIVEN`), with or without a
    /// report list; the reports as `crate::util::assert_reports` writes them.
    AssertValid {
        level: u8,
        list: bool,
    },
    Eval {
        t: f64,
        n: usize,
        left: bool,
        nonzero: bool,
    },
    EvalPoint {
        t: f64,
    },
    EvalUv {
        t: f64,
    },
    EvalNormal {
        t: f64,
        left: bool,
    },
    Eval1st {
        t: f64,
        n: usize,
        left: bool,
    },
    Knots {
        ivl: Option<[f64; 2]>,
        want: bool,
    },
    Nnk,
    Conts {
        angle: f64,
    },
    Equal {
        other: Option<Box<CosIn>>,
    },
    Copy,
    CopyEval {
        t: f64,
        n: usize,
        left: bool,
        nonzero: bool,
    },
    EditParam {
        a: f64,
        b: f64,
    },
    Reverse {
        a: f64,
        b: f64,
    },
    Trim {
        a: f64,
        b: f64,
        mode: u64,
    },
    Transform {
        origin: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        scale: Option<[f64; 3]>,
    },
    Approx {
        breaks: Vec<f64>,
        tol: f64,
        #[serde(rename = "match")]
        match_param: bool,
    },
    GetOrCreate {
        tol: f64,
        breaks: Option<Vec<f64>>,
        #[serde(rename = "match")]
        match_param: Option<bool>,
    },
    Bbox {
        a: f64,
        b: f64,
    },
    Drop {
        a: f64,
        b: f64,
        point: [f64; 3],
        vec: Option<[f64; 3]>,
        tol: f64,
        guess: Option<f64>,
        solver_op: i32,
    },
    Gap {
        other: Option<Box<CosIn>>,
        a: f64,
        b: f64,
        s0: f64,
        s1: f64,
        samples: usize,
        tol: f64,
        max: Option<f64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReadDbIn {
    binary: bool,
    version: u64,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum Inputs {
    Object { cos: Box<CosIn>, ops: Vec<OpIn> },
    ReadDb { read_db: ReadDbIn },
}

// ---- surfaces ---------------------------------------------------------------------

/// Reads an upstream `.sms` text file: `n m`, `p q`, rational flag, `(n+1)(m+1)`
/// points (`x y z`, or `wx wy wz w` when rational, v index fastest), `n+p+2` u knots,
/// `m+q+2` v knots.
fn read_sms(path: &str) -> Result<InlineSrf, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    let s = sms::parse_sms(&text, path)?;
    Ok(InlineSrf {
        net: s.net,
        p: s.p,
        q: s.q,
        u: s.u,
        v: s.v,
        borrow_nurb: false,
    })
}

fn resolve(s: &SrfIn) -> Result<InlineSrf, HarnessError> {
    match s {
        SrfIn::File { sms } => read_sms(sms),
        SrfIn::Inline(i) => Ok(i.clone()),
    }
}

fn srf_domain(s: &InlineSrf) -> [f64; 4] {
    [
        s.u.first().copied().unwrap_or(0.0),
        s.v.first().copied().unwrap_or(0.0),
        s.u.last().copied().unwrap_or(1.0),
        s.v.last().copied().unwrap_or(1.0),
    ]
}

// ---- C++ input --------------------------------------------------------------------

fn b(x: bool) -> f64 {
    f64::from(u8::from(x))
}

fn to_output(flat: Vec<f64>) -> Output {
    Output::new().values("out", flat)
}

// ---- input contract ---------------------------------------------------------------

/// The largest last control point index of a UV curve.
const MAX_INDEX: usize = 64;
/// The most control points of a surface row or column.
const MAX_POINTS: usize = 32;
/// The largest degree of a UV curve or surface.
const MAX_DEGREE: usize = 7;
/// The largest magnitude of a coordinate, knot or scalar.
const MAX_COORD: f64 = 1e5;
/// How far outside its domain, in domain lengths, a curve is evaluated.
const MAX_REL_DIST: f64 = 2.0;
/// The longest type or break array.
const MAX_ARRAY: usize = 256;
/// The longest database record.
const MAX_DB_BYTES: usize = 1 << 16;
/// The highest derivative order evaluated.
const MAX_DERIVS: usize = 6;
/// The most ops of an object case.
const MAX_OPS: usize = 16;

fn fin(v: f64) -> bool {
    v.is_finite() && v.abs() <= MAX_COORD
}

fn vec3(v: &[f64; 3]) -> bool {
    v.iter().all(|&c| fin(c))
}

/// The range of clamped knots of degree `p` within range, spanning at least `1e-3`
/// relative, with end multiplicity `p + 1` and interior multiplicity at most `p`.
fn knots_ok(u: &[f64], p: usize) -> Option<(f64, f64)> {
    if !u.iter().all(|&k| fin(k)) || u.windows(2).any(|w| w.first() > w.last()) {
        return None;
    }
    let (&a, &b) = (u.first()?, u.last()?);
    let m = u.len() - 1;
    let head = u.get(..=p)?;
    let tail = u.get(m.checked_sub(p)?..)?;
    let inner = u.get(p + 1..m.checked_sub(p)?)?;
    if a >= b || head.iter().any(|&k| k != a) || tail.iter().any(|&k| k != b) {
        return None;
    }
    if b - a < 1e-3 * a.abs().max(b.abs()).max(1.0) || inner.iter().any(|&k| k == a || k == b) {
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

/// A weight: `NOW` for a polynomial point, else in `[1e-3, 1e3]`.
fn weight_ok(w: f64, w0: f64) -> bool {
    (w == NOW) == (w0 == NOW) && (w == NOW || (1e-3..=1e3).contains(&w))
}

/// A UV curve's parameter range and Euclidean control points.
type UvCurve = ((f64, f64), Vec<[f64; 2]>);

/// The range and Euclidean control points of a UV B-spline within the size bounds, of
/// one rationality, with Euclidean points within range and knots as [`knots_ok`] takes them.
fn uv_bspline(bs: &UvBspline) -> Option<UvCurve> {
    let n = bs.cpts.len().checked_sub(1)?;
    let m = bs.u.len().checked_sub(1)?;
    if n > MAX_INDEX
        || bs.p > MAX_DEGREE
        || m > MAX_INDEX + MAX_DEGREE + 1
        || bs.p == 0
        || bs.p > n
        || m != n + bs.p + 1
    {
        return None;
    }
    let w0 = bs.cpts.first()?[2];
    let mut pts = Vec::with_capacity(bs.cpts.len());
    for &[x, y, w] in &bs.cpts {
        if !x.is_finite() || !y.is_finite() || !weight_ok(w, w0) {
            return None;
        }
        let (ex, ey) = if w == NOW { (x, y) } else { (x / w, y / w) };
        if ex.abs() > MAX_COORD || ey.abs() > MAX_COORD {
            return None;
        }
        pts.push([ex, ey]);
    }
    Some((knots_ok(&bs.u, bs.p)?, pts))
}

/// The domain `[u0, v0, u1, v1]` of a surface within the size bounds, with a full control
/// net of one rationality, Euclidean points within range and knots as [`knots_ok`] takes them.
fn surface(s: &InlineSrf) -> Option<[f64; 4]> {
    let n = s.net.len().checked_sub(1)?;
    let m = s.net.first()?.len().checked_sub(1)?;
    let (r, q_) = (s.u.len().checked_sub(1)?, s.v.len().checked_sub(1)?);
    let sizes = n < MAX_POINTS
        && m < MAX_POINTS
        && s.p <= MAX_DEGREE
        && s.q <= MAX_DEGREE
        && r <= MAX_POINTS + MAX_DEGREE
        && q_ <= MAX_POINTS + MAX_DEGREE
        && s.p >= 1
        && s.q >= 1
        && s.p <= n
        && s.q <= m
        && r == n + s.p + 1
        && q_ == m + s.q + 1
        && s.net.iter().all(|row| row.len() == m + 1);
    if !sizes {
        return None;
    }
    let w0 = s.net.first()?.first()?[3];
    let points = s.net.iter().flatten().all(|&[x, y, z, w]| {
        let d = if w == NOW { 1.0 } else { w };
        x.is_finite()
            && y.is_finite()
            && z.is_finite()
            && weight_ok(w, w0)
            && [x / d, y / d, z / d].iter().all(|c| c.abs() <= MAX_COORD)
    });
    let (u0, u1) = knots_ok(&s.u, s.p)?;
    let (v0, v1) = knots_ok(&s.v, s.q)?;
    points.then_some([u0, v0, u1, v1])
}

/// A curve on surface that keeps the contract, as the ops before the current one left it:
/// its parameter range, owner flag, whether its UV curve stays inside the surface domain,
/// and whether a reparameterisation, reversal or trim has run.
#[derive(Debug, Clone, Copy)]
struct Object {
    a: f64,
    b: f64,
    owner: u64,
    inside: bool,
    edited: bool,
}

fn in_domain(p: [f64; 2], d: [f64; 4]) -> bool {
    let [u0, v0, u1, v1] = d;
    let tu = 1e-9 * (u1 - u0);
    let tv = 1e-9 * (v1 - v0);
    p[0] >= u0 - tu && p[0] <= u1 + tu && p[1] >= v0 - tv && p[1] <= v1 + tv
}

/// Whether `t` is finite and at most [`MAX_REL_DIST`] range lengths outside `(a, b)`.
fn near(t: f64, a: f64, b: f64) -> bool {
    let far = MAX_REL_DIST * (b - a);
    fin(t) && t >= a - far && t <= b + far
}

/// The object of a curve on surface: a UV B-spline or a UV line of distinct ends, a surface
/// as [`surface`] takes it, an offset distance within range, a finite domain, an owner flag
/// up to 7, and a cache of strictly increasing breaks near the range.
fn object(c: &CosIn) -> Option<Object> {
    let ((a, b), pts) = match &c.uv {
        UvIn::Bspline(bs) => uv_bspline(bs)?,
        UvIn::Line([x0, y0, x1, y1]) => {
            let ok = [x0, y0, x1, y1].iter().all(|&&v| fin(v))
                && ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt() >= 1e-6;
            ok.then(|| ((0.0, 1.0), vec![[*x0, *y0], [*x1, *y1]]))?
        }
    };
    let dom = surface(&resolve(&c.srf).ok()?)?;
    let offset = c.offset.as_ref().is_none_or(|o| fin(o.distance));
    let domain = c.domain.unwrap_or([0.0; 4]).iter().all(|&v| fin(v));
    let cache = c.cache.as_ref().is_none_or(|k| {
        k.breaks.len() <= MAX_ARRAY
            && k.breaks.iter().all(|&t| near(t, a, b))
            && k.breaks.windows(2).all(|w| w.first() < w.last())
    });
    (offset && domain && c.owner <= 7 && cache).then(|| Object {
        a,
        b,
        owner: c.owner,
        inside: pts.iter().all(|&p| in_domain(p, dom)),
        edited: false,
    })
}

/// An interval inside the range, longer than `1e-4` of it.
fn inner_interval(c: &Object, x: f64, y: f64) -> bool {
    let eps = 1e-12 * (c.b - c.a);
    fin(x) && fin(y) && x >= c.a - eps && y <= c.b + eps && y - x > 1e-4 * (c.b - c.a)
}

/// At most [`MAX_ARRAY`] strictly increasing breaks inside the range.
fn breaks_ok(c: &Object, br: &[f64]) -> bool {
    let eps = 1e-12 * (c.b - c.a);
    br.len() <= MAX_ARRAY
        && br
            .iter()
            .all(|&t| fin(t) && t >= c.a - eps && t <= c.b + eps)
        && br.windows(2).all(|w| w.first() < w.last())
}

fn tolerance(t: f64) -> bool {
    fin(t) && (1e-7..=1.0).contains(&t)
}

/// A placement of finite axes of length `[0.5, 2]`, far from parallel.
fn placement(o: &[f64; 3], x: &[f64; 3], y: &[f64; 3]) -> bool {
    let len = |v: &[f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let dot = x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
    vec3(o)
        && vec3(x)
        && vec3(y)
        && (0.5..=2.0).contains(&len(x))
        && (0.5..=2.0).contains(&len(y))
        && dot.abs() <= 0.9 * len(x) * len(y)
}

/// Whether `op` keeps the contract on `c` as the ops before it left it, updating `c` as the
/// op does: a UV curve outside the surface domain takes only the ops that stay on the UV
/// curve's own data.
fn object_op(op: &OpIn, c: &mut Object) -> bool {
    let safe_outside = matches!(
        op,
        OpIn::Info { .. }
            | OpIn::State
            | OpIn::Eval { .. }
            | OpIn::EvalPoint { .. }
            | OpIn::EvalUv { .. }
            | OpIn::Equal { .. }
            | OpIn::Copy
            | OpIn::CopyEval { .. }
            | OpIn::AssertValid { .. }
    );
    if !c.inside && !safe_outside {
        return false;
    }
    let param = |c: &Object, t: f64| near(t, c.a, c.b);
    match op {
        OpIn::Info { types } => {
            types.len() <= MAX_ARRAY && types.iter().all(|t| t.abs() <= 1 << 30)
        }
        OpIn::State | OpIn::Nnk | OpIn::Copy => true,
        OpIn::AssertValid { level, .. } => *level <= 4 && *level != 3,
        OpIn::Eval { t, n, .. } | OpIn::CopyEval { t, n, .. } => param(c, *t) && *n <= MAX_DERIVS,
        OpIn::EvalPoint { t } | OpIn::EvalUv { t } | OpIn::EvalNormal { t, .. } => param(c, *t),
        OpIn::Eval1st { t, n, .. } => param(c, *t) && *n <= 4,
        OpIn::Knots { ivl, .. } => match ivl {
            // the interval must hold no UV knot
            Some([x, y]) => inner_interval(c, *x, *y) && !c.edited && (*x <= c.a || *y >= c.b),
            None => true,
        },
        OpIn::Conts { angle } => fin(*angle) && (0.0..=90.0).contains(angle),
        OpIn::Equal { other } => other.as_deref().is_none_or(|o| object(o).is_some()),
        OpIn::EditParam { a, b } => {
            c.edited = true;
            let ok =
                fin(*a) && fin(*b) && (b - a).abs() >= 1e-3 && a.abs() <= 1e3 && b.abs() <= 1e3;
            c.a = a.min(*b);
            c.b = a.max(*b);
            ok
        }
        OpIn::Reverse { a, b } => {
            c.edited = true;
            inner_interval(c, *a, *b)
        }
        OpIn::Trim { a, b, mode } => {
            c.edited = true;
            let ok = inner_interval(c, *a, *b) && *mode <= 2;
            if c.owner != 0 && *mode != 2 {
                c.a = *a;
                c.b = *b;
            }
            ok
        }
        OpIn::Transform {
            origin,
            x,
            y,
            scale,
        } => {
            let s = scale.unwrap_or([1.0; 3]);
            placement(origin, x, y) && vec3(&s) && s.iter().all(|v| (0.1..=10.0).contains(v))
        }
        OpIn::Approx { breaks, tol, .. } => breaks_ok(c, breaks) && tolerance(*tol),
        OpIn::GetOrCreate { tol, breaks, .. } => {
            tolerance(*tol) && breaks_ok(c, breaks.as_deref().unwrap_or_default())
        }
        OpIn::Bbox { a, b } => inner_interval(c, *a, *b),
        OpIn::Drop {
            a,
            b,
            point,
            vec,
            tol,
            guess,
            solver_op,
        } => {
            inner_interval(c, *a, *b)
                && vec3(point)
                && vec3(&vec.unwrap_or([0.0; 3]))
                && fin(*tol)
                && (0.0..=1.0).contains(tol)
                && param(c, guess.unwrap_or(*a))
                && [0, 3, 4, 5, 6].contains(solver_op)
        }
        OpIn::Gap {
            other,
            a,
            b,
            s0,
            s1,
            samples,
            tol,
            max,
        } => {
            other
                .as_deref()
                .is_none_or(|o| object(o).is_some_and(|o| o.inside))
                && inner_interval(c, *a, *b)
                && fin(*s0)
                && fin(*s1)
                && *samples <= 8
                && *samples != 1
                && fin(*tol)
                && (1e-9..=1.0).contains(tol)
                && fin(max.unwrap_or(0.0))
        }
    }
}

/// Whether a case keeps the input contract ported from the reference's wrapper: an object
/// as [`object`] takes it with at most [`MAX_OPS`] ops, each as [`object_op`] takes it on
/// the state the ops before it left; a record of at most [`MAX_DB_BYTES`] bytes, complete
/// (ending in a newline) when it is ASCII.
fn keeps_contract(i: &Inputs) -> bool {
    match i {
        Inputs::Object { cos, ops } => {
            ops.len() <= MAX_OPS
                && object(cos).is_some_and(|mut c| ops.iter().all(|op| object_op(op, &mut c)))
        }
        Inputs::ReadDb { read_db: r } => {
            r.version <= 1 << 20
                && r.bytes.len() <= MAX_DB_BYTES
                && (r.binary || r.bytes.ends_with(b"\n"))
        }
    }
}

// ---- Rust side --------------------------------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

fn build_uv(bs: &UvBspline) -> Result<Curve, HarnessError> {
    let pw = bs
        .cpts
        .iter()
        .map(|&[x, y, w]| CPoint::new(x, y, NOZ, w))
        .collect();
    let cur =
        NurbCurve::new(pw, bs.p, bs.u.clone()).map_err(|e| harness(format!("uv curve: {e}")))?;
    Ok(Curve::from(BSplineCurve::from_nurb(cur, 2, bs.borrow_nurb)))
}

fn build_surface(
    k: &Kernel,
    s: &SrfIn,
    offset: Option<&OffsetIn>,
) -> Result<SmSurface, HarnessError> {
    let i = resolve(s)?;
    let net = i
        .net
        .iter()
        .map(|row| {
            row.iter()
                .map(|c| CPoint::new(c[0], c[1], c[2], c[3]))
                .collect()
        })
        .collect();
    let sur =
        Surface::new(net, i.p, i.q, i.u, i.v).map_err(|e| harness(format!("surface: {e}")))?;
    let base = SmSurface::from(BSplineSurface::from_nurb(sur, i.borrow_nurb));
    match offset {
        None => Ok(base),
        Some(offset) => OffsetSurface::new(k, offset.distance, base, offset.owns_base, SS_UNKNOWN)
            .map(SmSurface::from)
            .map_err(|e| harness(format!("offset surface: {e}"))),
    }
}

/// The object, or the error (status) its construction gives.
fn build(c: &CosIn) -> Result<solidcore::Result<CrvOnSurf>, HarnessError> {
    let k = Kernel::default();
    let srf = build_surface(&k, &c.srf, c.offset.as_ref())?;
    let domain = c
        .domain
        .map(|[a, bb, cc, d]| Extent2d::from_bounds(a, bb, cc, d));
    let r = match &c.uv {
        UvIn::Bspline(bs) => CrvOnSurf::new(&k, build_uv(bs)?, srf, domain, c.owner),
        UvIn::Line([x0, y0, x1, y1]) => CrvOnSurf::from_uv_line(
            &k,
            Point2::new(*x0, *y0),
            Point2::new(*x1, *y1),
            srf,
            domain,
            c.owner,
        ),
    };
    Ok(r.map(|mut cos| {
        if let Some(k) = &c.cache {
            cos.set_breaks_cache(k.need, k.breaks.clone());
        }
        cos
    }))
}

fn pv(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn put_nurb(o: &mut Vec<f64>, bs: &BSplineCurve) {
    match bs.gw_nurb() {
        None => o.push(-1.0),
        Some(cur) => {
            o.extend([cur.n() as f64, cur.degree() as f64, cur.m() as f64]);
            for p in cur.cpts() {
                o.extend([p.x, p.y, p.z, p.w]);
            }
            o.extend_from_slice(cur.knots());
        }
    }
}

/// `type [dim nurb]` (B-splines and lines).
fn put_curve(o: &mut Vec<f64>, c: &Curve) {
    o.push(c.type_code() as f64);
    match c.kind() {
        CurveKind::BSpline(bs) => {
            o.push(bs.dim() as f64);
            put_nurb(o, bs);
        }
        CurveKind::Line(l) => {
            o.push(l.dim() as f64);
            put_nurb(o, l.base());
        }
        _ => {}
    }
}

fn put_surface(o: &mut Vec<f64>, s: &SmSurface) {
    o.push(s.type_code() as f64);
    match s.kind() {
        SurfaceKind::BSpline(s) => match s.gw_nurb() {
            None => o.push(-1.0),
            Some(sur) => {
                o.extend([sur.n() as f64, sur.m() as f64]);
                for row in sur.cpts() {
                    for p in row {
                        o.extend([p.x, p.y, p.z, p.w]);
                    }
                }
                o.extend_from_slice(sur.u_knots());
                o.extend_from_slice(sur.v_knots());
            }
        },
        SurfaceKind::OffsetSurface(s) => {
            o.extend([s.offset_distance(), b(s.owns_surface())]);
            put_surface(o, s.base_surface());
        }
        _ => {}
    }
}

fn put_state(o: &mut Vec<f64>, c: &CrvOnSurf) {
    o.push(c.owner_flag() as f64);
    let d = c.uv_domain();
    o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
    o.push(b(c.need_breaks()));
    let breaks = c.breaks();
    o.push(breaks.len() as f64);
    o.extend_from_slice(&breaks);
    o.push(c.bs_approx_max_gap_3d());
    o.push(b(c.parametrization_matches()));
    match c.bs_approx() {
        None => o.push(0.0),
        Some(a) => {
            o.push(1.0);
            put_nurb(o, &a);
        }
    }
    put_curve(o, c.uv_curve());
    put_surface(o, c.surface());
}

/// The `SmStatus` of a Rust error; `1007` for `InvalidInput`, `-1` for one upstream
/// cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// A deferral or a harness failure that ends the Rust run.
enum Stop {
    Deferred(Box<Output>),
    Harness(HarnessError),
}

impl From<HarnessError> for Stop {
    fn from(e: HarnessError) -> Self {
        Stop::Harness(e)
    }
}

/// The status of `r` (deferring on `Unported`).
fn st<T>(r: &solidcore::Result<T>) -> Result<f64, Stop> {
    match r {
        Ok(_) => Ok(0.0),
        Err(e) => match Output::unported(e) {
            Some(d) => Err(Stop::Deferred(Box::new(d))),
            None => Ok(status(e)),
        },
    }
}

fn wrap(c: &CrvOnSurf) -> Curve {
    Curve::from(c.clone())
}

fn cos_of(c: Curve) -> Result<CrvOnSurf, HarnessError> {
    match c.into_kind() {
        CurveKind::CrvOnSurf(x) => Ok(*x),
        _ => Err(harness("not a curve on surface")),
    }
}

fn solver_op(code: i32) -> Result<SolverOperation, HarnessError> {
    SolverOperation::from_code(code).ok_or_else(|| harness("bad solver op"))
}

fn rust_op(o: &mut Vec<f64>, c: &mut CrvOnSurf, op: &OpIn, k: &Kernel) -> Result<(), Stop> {
    match op {
        OpIn::Info { types } => {
            let w = wrap(c);
            let deg = w.degree();
            o.push(st(&deg)?);
            let ivl = w.natural_interval();
            o.extend([
                w.type_code() as f64,
                w.dim() as f64,
                deg.unwrap_or(0) as f64,
                ivl.min(),
                ivl.max(),
                b(w.is_analytic()),
                b(w.is_bounded()),
            ]);
            o.extend(types.iter().map(|&t| b(w.is_kind_of(t))));
            put_state(o, c);
        }
        OpIn::State => {
            o.push(0.0);
            put_state(o, c);
        }
        OpIn::AssertValid { level, list } => {
            // the object itself, not a copy (`Clone` is the copy constructor, which
            // links the parts it copies to the copy)
            let stand_in = CrvOnSurf::empty(c.uv_curve().clone(), c.surface().clone())
                .map_err(|e| Stop::Harness(harness(e.to_string())))?;
            let mut w = Curve::from(std::mem::replace(c, stand_in));
            let mut reports = AssertArray::new();
            let key = ObjectKey(1);
            let r = curve_assert_valid(k, &mut w, list.then_some(&mut reports), *level, key);
            *c = cos_of(w).map_err(Stop::Harness)?;
            o.push(st(&r)?);
            if let Ok(ok) = r {
                put_assert_reports(o, ok, &reports, key);
            }
        }
        OpIn::Eval {
            t,
            n,
            left,
            nonzero,
        } => {
            let r = c.evaluate(k, *t, *n, *left, *nonzero);
            o.push(st(&r)?);
            if let Ok(v) = r {
                for p in v {
                    pv(o, p);
                }
            }
        }
        OpIn::EvalPoint { t } => {
            let r = c.evaluate_point(k, *t);
            o.push(st(&r)?);
            if let Ok(p) = r {
                pv(o, p);
            }
        }
        OpIn::EvalUv { t } => {
            let r = c.evaluate_uv_point(k, *t);
            o.push(st(&r)?);
            if let Ok(p) = r {
                o.extend([p.x, p.y]);
            }
        }
        OpIn::EvalNormal { t, left } => {
            let r = c.evaluate_surface_normal(k, *t, *left);
            o.push(st(&r)?);
            if let Ok(s) = r {
                o.extend([s.uv.x, s.uv.y, s.uv_tangent.x, s.uv_tangent.y]);
                pv(o, s.normal);
            }
        }
        OpIn::Eval1st { t, left, .. } => {
            let r = c.evaluate_surface_1st_deriv_derivs(k, *t, *left);
            o.push(st(&r)?);
            if let Ok((du, dv)) = r {
                for (u, v) in du.iter().zip(&dv) {
                    pv(o, *u);
                    pv(o, *v);
                }
            }
        }
        OpIn::Knots { ivl, want } => {
            let iv = ivl.map(|[x, y]| Extent1d::from_min_max(x, y));
            // the status, then what GetKnots wrote (also on a failing status)
            let (kn, m) = match c.knots_with_status(k, iv.as_ref()) {
                Ok(w) => {
                    o.push(st(&w.status)?);
                    (w.knots, w.mults)
                }
                Err(e) => {
                    o.push(st(&Err::<(), _>(e))?);
                    (Vec::new(), Vec::new())
                }
            };
            o.push(kn.len() as f64);
            o.extend_from_slice(&kn);
            if *want {
                o.push(m.len() as f64);
                o.extend(m.iter().map(|&x| x as f64));
            }
            put_state(o, c);
        }
        OpIn::Nnk => {
            let r = c.number_natural_knots(k);
            st(&r)?;
            o.extend([0.0, r.unwrap_or(0) as f64]);
            put_state(o, c);
        }
        OpIn::Conts { angle } => {
            let r = c.calculate_continuities(k, *angle);
            o.push(st(&r)?);
            let (mn, cs) = r.unwrap_or((
                solidcore::geom::geom_utility::p2::ContinuityType::Undefined,
                Vec::new(),
            ));
            o.extend([f64::from(mn as i32), cs.len() as f64]);
            o.extend(cs.iter().map(|&x| f64::from(x as i32)));
            put_state(o, c);
        }
        OpIn::Equal { other } => match other {
            None => {
                let w = wrap(c);
                let r = w.approx_eq(k, &w);
                o.push(st(&r)?);
                o.push(b(r.unwrap_or(false)));
            }
            Some(oc) => {
                let d = match build(oc)? {
                    Ok(d) => d,
                    Err(e) => return Err(Stop::Harness(harness(format!("other: {e}")))),
                };
                let (wa, wb) = (wrap(c), Curve::from(d));
                let r1 = wa.approx_eq(k, &wb);
                let r2 = wb.approx_eq(k, &wa);
                st(&r1)?;
                st(&r2)?;
                o.extend([0.0, b(r1.unwrap_or(false)), b(r2.unwrap_or(false))]);
            }
        },
        OpIn::Copy => {
            let r = wrap(c).copy(k);
            o.push(st(&r)?);
            if let Ok(cp) = r {
                let eq = cp.approx_eq(k, &wrap(c));
                st(&eq)?;
                let cp = cos_of(cp)?;
                put_state(o, &cp);
                o.push(b(eq.unwrap_or(false)));
            }
        }
        OpIn::CopyEval {
            t,
            n,
            left,
            nonzero,
        } => {
            let r = wrap(c).copy(k);
            o.push(st(&r)?);
            if let Ok(cp) = r {
                let cp = cos_of(cp)?;
                put_state(o, &cp);
                let r = cp.evaluate(k, *t, *n, *left, *nonzero);
                o.push(st(&r)?);
                if let Ok(v) = r {
                    for p in v {
                        pv(o, p);
                    }
                }
            }
        }
        OpIn::EditParam { a, b: bb } => {
            let r = c.edit_parameterization(&Kernel::default(), &Extent1d::from_min_max(*a, *bb));
            o.push(st(&r)?);
            put_state(o, c);
        }
        OpIn::Reverse { a, b: bb } => {
            let r =
                c.reverse_parameterization(&Kernel::default(), &Extent1d::from_min_max(*a, *bb));
            o.push(st(&r)?);
            if let Ok(nv) = r {
                o.extend([nv.min(), nv.max()]);
            }
            put_state(o, c);
        }
        OpIn::Trim { a, b: bb, mode } => {
            let mode = match mode {
                0 => TrimMode::Notify,
                1 => TrimMode::Silent,
                _ => TrimMode::IntervalOnly,
            };
            let mut ivl = Extent1d::from_min_max(*a, *bb);
            let r = c.trim(k, &mut ivl, mode);
            o.push(st(&r)?);
            o.extend([ivl.min(), ivl.max()]);
            put_state(o, c);
        }
        OpIn::Transform {
            origin,
            x,
            y,
            scale,
        } => {
            let ax = Axis2Placement::new(v3(*origin), v3(*x), v3(*y));
            let r = c.transform(k, &ax, scale.map(v3));
            o.push(st(&r)?);
            put_state(o, c);
        }
        OpIn::Approx {
            breaks,
            tol,
            match_param,
        } => {
            let r = c.approximate_curve(&Kernel::default(), breaks, *tol, *match_param);
            o.push(st(&r)?);
            match r {
                Ok((bs, gap)) => {
                    o.extend([gap, 1.0]);
                    put_curve(o, &Curve::from(bs));
                }
                Err(_) => o.extend([c.bs_approx_max_gap_3d(), 0.0]),
            }
            put_state(o, c);
        }
        OpIn::GetOrCreate {
            tol,
            breaks,
            match_param,
        } => {
            let r = c.get_or_create_bs_approx(
                &Kernel::default(),
                *tol,
                breaks.as_deref(),
                *match_param,
            );
            o.push(st(&r)?);
            match r {
                Ok(bs) => {
                    o.push(1.0);
                    put_curve(o, &Curve::from(bs));
                }
                Err(_) => o.push(0.0),
            }
            put_state(o, c);
        }
        OpIn::Bbox { a, b: bb } => {
            let req = BoundingBoxRequest {
                normal: true,
                pseudo: false,
                polar: false,
            };
            let r = c.calculate_bounding_box(
                &Kernel::default(),
                &Extent1d::from_min_max(*a, *bb),
                req,
                true,
            );
            o.push(st(&r)?);
            if let Ok(bx) = r {
                let n = bx.normal.ok_or_else(|| harness("no normal box"))?;
                pv(o, n.min());
                pv(o, n.max());
            }
            put_state(o, c);
        }
        OpIn::Drop {
            a,
            b: bb,
            point,
            vec,
            tol,
            guess,
            solver_op: sop,
        } => {
            let r = c.drop_point(
                k,
                &Extent1d::from_min_max(*a, *bb),
                v3(*point),
                vec.map(v3),
                *tol,
                *guess,
                solver_op(*sop)?,
            );
            o.push(st(&r)?);
            let d = r.unwrap_or_default();
            o.extend([b(d.success), d.parameter, d.distance]);
            put_state(o, c);
        }
        OpIn::Gap {
            other,
            a,
            b: bb,
            s0,
            s1,
            samples,
            tol,
            max,
        } => {
            // the FFI rejects more than 8 samples; upstream (and the port) would loop that
            // many times, so a larger count is not a case for Rust either
            if *samples > 8 {
                return Err(HarnessError::new(format!("{samples} samples")).into());
            }
            let ivl = Extent1d::from_min_max(*a, *bb);
            let run = |this: &CrvOnSurf, oc: &Curve| {
                this.surface_curve_max_distance_between(k, &ivl, oc, *s0, *s1, *samples, *tol, *max)
            };
            let r = match other {
                None => {
                    // the same object as the other curve
                    let w = Curve::from(c.clone());
                    match w.kind() {
                        CurveKind::CrvOnSurf(x) => run(x, &w),
                        _ => return Err(Stop::Harness(harness("not a curve on surface"))),
                    }
                }
                Some(oc) => {
                    let d = match build(oc)? {
                        Ok(d) => d,
                        Err(e) => return Err(Stop::Harness(harness(format!("other: {e}")))),
                    };
                    run(c, &Curve::from(d))
                }
            };
            o.push(st(&r)?);
            if let Ok(g) = r {
                o.extend([
                    g.max_dist,
                    g.max_dev,
                    g.curve_t_dist,
                    g.other_t_dist,
                    g.curve_t_dev,
                    g.other_t_dev,
                ]);
            }
        }
    }
    Ok(())
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let mut o: Vec<f64> = Vec::new();
    match i {
        Inputs::ReadDb { read_db } => {
            let ft = if read_db.binary {
                FileType::Binary
            } else {
                FileType::Ascii
            };
            let mut db = DatabaseReader::with_stream(ft, InputStream::new(read_db.bytes.clone()));
            match CrvOnSurf::read_from_db(&Kernel::default(), &mut db, read_db.version) {
                Ok(c) => {
                    o.extend([0.0, 1.0]);
                    put_state(&mut o, &c);
                }
                Err(e) => {
                    if let Some(d) = Output::unported(&e) {
                        return Ok(d);
                    }
                    o.extend([status(&e), 0.0]);
                }
            }
            Ok(to_output(o))
        }
        Inputs::Object { cos, ops } => {
            let mut c = match build(cos)? {
                Ok(c) => c,
                Err(e) => {
                    if let Some(d) = Output::unported(&e) {
                        return Ok(d);
                    }
                    return Err(harness(format!("construction failed: {e}")));
                }
            };
            let k = Kernel::default();
            for op in ops {
                match rust_op(&mut o, &mut c, op, &k) {
                    Ok(()) => {}
                    Err(Stop::Deferred(d)) => return Ok(*d),
                    Err(Stop::Harness(e)) => return Err(e),
                }
            }
            Ok(to_output(o))
        }
    }
}

// ---- generation -------------------------------------------------------------------

/// A random clamped knot vector of degree `p` on `[a, a + len]` with `extra` interior
/// knots (multiplicities up to `p`).
fn gen_knots(rng: &mut Rng, p: usize, extra: usize, a: f64, len: f64) -> Vec<f64> {
    let bnd = a + len;
    let mut inner: Vec<f64> = Vec::new();
    let mut remaining = extra;
    while remaining > 0 {
        let v = if rng.chance(0.4) {
            a + len * (1.0 + rng.below(15) as f64) / 16.0
        } else {
            a + len * rng.uniform(0.02, 0.98)
        };
        if v <= a || v >= bnd || inner.contains(&v) {
            remaining -= 1;
            continue;
        }
        let mult = if p >= 2 && rng.chance(0.2) {
            1 + rng.below(p.min(remaining) as u64) as usize
        } else {
            1
        };
        inner.extend(std::iter::repeat_n(v, mult));
        remaining -= mult;
    }
    inner.sort_by(f64::total_cmp);
    let mut u = vec![a; p + 1];
    u.extend(inner);
    u.extend(std::iter::repeat_n(bnd, p + 1));
    u
}

fn gen_range(rng: &mut Rng) -> (f64, f64) {
    let unit_interval = rng.chance(0.5);
    if unit_interval {
        (0.0, 1.0)
    } else if rng.chance(0.5) {
        (rng.below(5) as f64 - 2.0, 1.0 + rng.below(6) as f64)
    } else {
        (rng.uniform(-5.0, 5.0), rng.uniform(0.25, 10.0))
    }
}

/// A random clamped 3D surface, rational or not.
fn gen_srf(rng: &mut Rng) -> InlineSrf {
    if rng.chance(0.15) {
        // a bilinear patch (upstream's planar and bilinear test surfaces)
        let (a, la) = gen_range(rng);
        let (c, lc) = gen_range(rng);
        let z = |rng: &mut Rng| {
            if rng.chance(0.5) {
                0.0
            } else {
                rng.uniform(-2.0, 2.0)
            }
        };
        let s = rng.uniform(1.0, 10.0);
        return InlineSrf {
            net: vec![
                vec![[0.0, 0.0, z(rng), NOW], [0.0, s, z(rng), NOW]],
                vec![[s, 0.0, z(rng), NOW], [s, s, z(rng), NOW]],
            ],
            p: 1,
            q: 1,
            u: vec![a, a, a + la, a + la],
            v: vec![c, c, c + lc, c + lc],
            borrow_nurb: false,
        };
    }
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let (a, la) = gen_range(rng);
    let eu = rng.below(4) as usize;
    let u = gen_knots(rng, p, eu, a, la);
    let (c, lc) = gen_range(rng);
    let ev = rng.below(4) as usize;
    let v = gen_knots(rng, q, ev, c, lc);
    let n = u.len() - p - 2;
    let m = v.len() - q - 2;
    let rational = rng.chance(0.3);
    let net = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    let [x, y, z] = if rng.chance(0.6) {
                        [
                            i as f64 + rng.uniform(-0.3, 0.3),
                            j as f64 + rng.uniform(-0.3, 0.3),
                            rng.uniform(-1.0, 1.0),
                        ]
                    } else {
                        rng.point3(-10.0, 10.0)
                    };
                    if rational {
                        let w = rng.uniform(0.3, 3.0);
                        [x * w, y * w, z * w, w]
                    } else {
                        [x, y, z, NOW]
                    }
                })
                .collect()
        })
        .collect();
    InlineSrf {
        net,
        p,
        q,
        u,
        v,
        borrow_nurb: false,
    }
}

/// The upstream test surfaces.
const SMS_FILES: [&str; 2] = [
    "upstream/TestFiles/pt_TestFiles/Surface.sms",
    "upstream/TestFiles/pt_TestFiles/DropCurveSurface.sms",
];

/// A point of the domain `d` (shrunk by `margin` of its size), or outside it.
fn gen_uv(rng: &mut Rng, d: [f64; 4], margin: f64, outside: bool) -> [f64; 2] {
    let [u0, v0, u1, v1] = d;
    let (lu, lv) = (u1 - u0, v1 - v0);
    if outside {
        return [
            u0 + lu * rng.uniform(-0.5, 1.5),
            v0 + lv * rng.uniform(-0.5, 1.5),
        ];
    }
    let pick = |rng: &mut Rng, a: f64, l: f64| match rng.below(12) {
        0 => a + l * margin,
        1 => a + l * (1.0 - margin),
        _ => a + l * rng.uniform(margin, 1.0 - margin),
    };
    [pick(rng, u0, lu), pick(rng, v0, lv)]
}

fn gen_uv_curve(rng: &mut Rng, d: [f64; 4], outside: bool) -> UvIn {
    let margin = if rng.chance(0.2) { 0.0 } else { 0.02 };
    if rng.chance(0.2) {
        let a = gen_uv(rng, d, margin, outside);
        let mut bb = gen_uv(rng, d, margin, outside);
        if (a[0] - bb[0]).abs() + (a[1] - bb[1]).abs() < 1e-3 {
            bb[0] = d[0] + 0.5 * (d[2] - d[0]);
            bb[1] = d[1] + 0.5 * (d[3] - d[1]);
        }
        return UvIn::Line([a[0], a[1], bb[0], bb[1]]);
    }
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(5) as usize;
    let (a, len) = gen_range(rng);
    let u = gen_knots(rng, p, n - p, a, len);
    let rational = rng.chance(0.2);
    let cpts = (0..=n)
        .map(|_| {
            let [x, y] = gen_uv(rng, d, margin, outside);
            if rational {
                let w = rng.uniform(0.5, 2.0);
                [x * w, y * w, w]
            } else {
                [x, y, NOW]
            }
        })
        .collect();
    UvIn::Bspline(UvBspline {
        cpts,
        p,
        u,
        borrow_nurb: false,
    })
}

fn range_of(c: &CosIn) -> (f64, f64) {
    match &c.uv {
        UvIn::Bspline(bs) => (
            bs.u.first().copied().unwrap_or(0.0),
            bs.u.last().copied().unwrap_or(1.0),
        ),
        UvIn::Line(_) => (0.0, 1.0),
    }
}

fn uv_knots(c: &CosIn) -> Vec<f64> {
    match &c.uv {
        UvIn::Bspline(bs) => {
            let mut k = bs.u.clone();
            k.dedup();
            k
        }
        UvIn::Line(_) => vec![0.0, 1.0],
    }
}

/// Increasing breaks in `[a, b]`: the UV knots plus a few interior values.
fn gen_breaks(rng: &mut Rng, c: &CosIn, a: f64, bb: f64) -> Vec<f64> {
    let mut v: Vec<f64> = uv_knots(c)
        .into_iter()
        .filter(|&t| t >= a && t <= bb)
        .collect();
    for _ in 0..rng.below(3) {
        v.push(a + (bb - a) * rng.uniform(0.05, 0.95));
    }
    if rng.chance(0.2) {
        v.push(a);
        v.push(bb);
    }
    v.sort_by(f64::total_cmp);
    v.dedup();
    if rng.chance(0.05) {
        v.truncate(rng.below(2) as usize);
    }
    v
}

fn gen_cos(rng: &mut Rng, outside: bool) -> (CosIn, [f64; 4]) {
    let srf = if rng.chance(0.08) {
        SrfIn::File {
            sms: (*rng.choose(&SMS_FILES).unwrap_or(&"")).to_string(),
        }
    } else {
        SrfIn::Inline(gen_srf(rng))
    };
    let d = resolve(&srf).map_or([0.0, 0.0, 1.0, 1.0], |s| srf_domain(&s));
    let uv = gen_uv_curve(rng, d, outside);
    let domain = match rng.below(10) {
        0 => Some(d),
        1 => {
            let [u0, v0, u1, v1] = d;
            Some([
                u0 + 0.1 * (u1 - u0),
                v0 + 0.1 * (v1 - v0),
                u1 - 0.2 * (u1 - u0),
                v1,
            ])
        }
        _ => None,
    };
    let owner = if rng.chance(0.9) {
        rng.below(4)
    } else {
        rng.below(8)
    };
    let mut c = CosIn {
        uv,
        srf,
        offset: None,
        domain,
        owner,
        cache: None,
    };
    if rng.chance(0.6) {
        let (a, bb) = range_of(&c);
        c.cache = Some(CacheIn {
            need: rng.chance(0.1),
            breaks: gen_breaks(rng, &c, a, bb),
        });
    }
    (c, d)
}

/// A parameter of the range `[a, b]`: interior, at an end, or slightly outside.
fn gen_t(rng: &mut Rng, a: f64, bb: f64, knots: &[f64]) -> f64 {
    match rng.below(10) {
        0 => a,
        1 => bb,
        2 => *rng.choose(knots).unwrap_or(&a),
        3 => a + (bb - a) * rng.uniform(-0.2, 1.2),
        _ => a + (bb - a) * rng.unit(),
    }
}

fn gen_inner(rng: &mut Rng, a: f64, bb: f64) -> (f64, f64) {
    if rng.chance(0.3) {
        return (a, bb);
    }
    let x = a + (bb - a) * rng.uniform(0.0, 0.45);
    let y = a + (bb - a) * rng.uniform(0.55, 1.0);
    (x, y)
}

fn rot_frame(rng: &mut Rng) -> ([f64; 3], [f64; 3]) {
    let t = rng.uniform(0.0, std::f64::consts::TAU);
    let (s, c) = t.sin_cos();
    match rng.below(3) {
        0 => ([c, s, 0.0], [-s, c, 0.0]),
        1 => ([c, 0.0, -s], [0.0, 1.0, 0.0]),
        _ => ([1.0, 0.0, 0.0], [0.0, c, s]),
    }
}

/// An op on the object with current range `[a, b]` (updated by the editing ops).
fn gen_op(rng: &mut Rng, c: &CosIn, d: [f64; 4], range: &mut (f64, f64), outside: bool) -> OpIn {
    let (a, bb) = *range;
    let knots = uv_knots(c);
    let pick = if outside {
        // only the ops safe on a UV curve outside the domain
        *rng.choose(&[0, 2, 3, 4, 10, 11]).unwrap_or(&0)
    } else {
        rng.below(40)
    };
    match pick {
        0 => OpIn::Info {
            types: vec![
                CrvOnSurf::TYPE,
                14001,
                14033,
                14053,
                10001,
                rng.below(30000) as i64,
            ],
        },
        1 => OpIn::State,
        2 | 12 | 13 => OpIn::Eval {
            t: gen_t(rng, a, bb, &knots),
            n: rng.below(7) as usize,
            left: rng.chance(0.5),
            nonzero: rng.chance(0.5),
        },
        3 | 14 => OpIn::EvalPoint {
            t: gen_t(rng, a, bb, &knots),
        },
        4 => OpIn::EvalUv {
            t: gen_t(rng, a, bb, &knots),
        },
        5 => OpIn::EvalNormal {
            t: gen_t(rng, a, bb, &knots),
            left: rng.chance(0.5),
        },
        6 => OpIn::Eval1st {
            t: gen_t(rng, a, bb, &knots),
            n: rng.below(4) as usize,
            left: rng.chance(0.5),
        },
        7 | 15 => OpIn::Knots {
            ivl: if rng.chance(0.4) {
                let (x, y) = gen_inner(rng, a, bb);
                Some([x, y])
            } else {
                None
            },
            want: rng.chance(0.7),
        },
        8 => OpIn::Nnk,
        9 | 16 => OpIn::Conts {
            angle: if rng.chance(0.5) {
                0.5
            } else {
                rng.uniform(0.0, 10.0)
            },
        },
        10 => OpIn::Equal {
            other: match rng.below(10) {
                0..=1 => None,
                2..=4 => Some(Box::new(c.clone())),
                5..=6 => {
                    let mut o = c.clone();
                    o.domain = Some([d[0], d[1], d[2] * 0.5 + d[0] * 0.5, d[3]]);
                    Some(Box::new(o))
                }
                _ => Some(Box::new(gen_cos(rng, false).0)),
            },
        },
        11 | 17 => OpIn::Copy,
        18 => {
            let x = rng.uniform(-10.0, 10.0);
            let y = x + rng.uniform(0.01, 20.0) * if rng.chance(0.1) { -1.0 } else { 1.0 };
            *range = (x.min(y), x.max(y));
            OpIn::EditParam { a: x, b: y }
        }
        19 => {
            let (x, y) = gen_inner(rng, a, bb);
            OpIn::Reverse { a: x, b: y }
        }
        20 | 21 => {
            let (x, y) = gen_inner(rng, a, bb);
            let mode = rng.below(3);
            if c.owner != 0 && mode != 2 {
                *range = (x, y);
            }
            OpIn::Trim { a: x, b: y, mode }
        }
        22 | 23 => {
            let identity = rng.chance(0.3);
            let (x, y) = if identity {
                ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
            } else {
                rot_frame(rng)
            };
            OpIn::Transform {
                origin: if identity && rng.chance(0.7) {
                    [0.0; 3]
                } else {
                    rng.point3(-5.0, 5.0)
                },
                x,
                y,
                scale: match rng.below(4) {
                    0 => None,
                    1 => Some([1.0, 1.0, 1.0]),
                    2 => {
                        let s = rng.uniform(0.5, 3.0);
                        Some([s, s, s])
                    }
                    _ => Some([
                        rng.uniform(0.5, 2.0),
                        rng.uniform(0.5, 2.0),
                        rng.uniform(0.5, 2.0),
                    ]),
                },
            }
        }
        24..=28 => OpIn::Approx {
            breaks: gen_breaks(rng, c, a, bb),
            tol: *rng.choose(&[1e-3, 1e-4, 5e-6, 1e-2]).unwrap_or(&1e-3),
            match_param: rng.chance(0.5),
        },
        29..=31 => OpIn::GetOrCreate {
            tol: *rng.choose(&[1e-3, 1e-4, 5e-6, 1e-2]).unwrap_or(&1e-3),
            breaks: if rng.chance(0.6) {
                Some(gen_breaks(rng, c, a, bb))
            } else {
                None
            },
            match_param: if rng.chance(0.5) {
                Some(rng.chance(0.5))
            } else {
                None
            },
        },
        32 | 33 => {
            let (x, y) = gen_inner(rng, a, bb);
            OpIn::Bbox { a: x, b: y }
        }
        34..=37 => {
            let (x, y) = gen_inner(rng, a, bb);
            let [u, v] = gen_uv(rng, d, 0.0, false);
            let s = resolve(&c.srf).ok();
            // a point near the surface: a corner-ish control point plus noise
            let base = s
                .as_ref()
                .and_then(|s| s.net.first().and_then(|r| r.first()).copied())
                .map_or([u, v, 0.0], |p| {
                    let w = if p[3] == NOW { 1.0 } else { p[3] };
                    [p[0] / w, p[1] / w, p[2] / w]
                });
            let noise = rng.point3(-3.0, 3.0);
            OpIn::Drop {
                a: x,
                b: y,
                point: [base[0] + noise[0], base[1] + noise[1], base[2] + noise[2]],
                vec: if rng.chance(0.2) {
                    Some(rng.point3(-1.0, 1.0))
                } else {
                    None
                },
                tol: *rng.choose(&[1e-5, 1e-3, 0.0, 0.1]).unwrap_or(&1e-5),
                guess: if rng.chance(0.5) {
                    Some(x + (y - x) * rng.unit())
                } else {
                    None
                },
                solver_op: *rng.choose(&[3, 3, 3, 0, 5, 4]).unwrap_or(&3),
            }
        }
        _ => {
            let (x, y) = if rng.chance(0.6) {
                (a, bb)
            } else {
                gen_inner(rng, a, bb)
            };
            let same = rng.chance(0.7);
            let (s0, s1) = if same {
                if rng.chance(0.5) {
                    (x, y)
                } else {
                    (y, x)
                }
            } else {
                gen_inner(rng, a, bb)
            };
            OpIn::Gap {
                other: if same && rng.chance(0.8) {
                    None
                } else {
                    Some(Box::new(c.clone()))
                },
                a: x,
                b: y,
                s0,
                s1,
                samples: *rng.choose(&[0, 2, 3, 5]).unwrap_or(&2),
                tol: 1e-5,
                max: if rng.chance(0.3) { Some(1e-3) } else { None },
            }
        }
    }
}

/// A database record written by `WriteToDB` (the port's `write_curve`) for a random
/// object, sometimes with the UV curve type replaced by one that is not a curve.
fn gen_read_db(rng: &mut Rng) -> Option<Inputs> {
    let (c, _) = gen_cos(rng, false);
    let binary = rng.chance(0.5);
    let version = *rng.choose(&[30u64, 33, 40])?;
    object(&c)?;
    let cos = build(&c).ok()?.ok()?;
    let mut w = DatabaseWriter::open_for_write(if binary {
        FileType::Binary
    } else {
        FileType::Ascii
    });
    write_curve(&wrap(&cos), &mut w, version).ok()?;
    let mut bytes = w.into_bytes();
    if binary {
        // the UV curve type follows the domain (4 doubles) and the breaks flag (an int)
        if rng.chance(0.5) {
            let to: i64 = *rng.choose(&[14099, 1])?;
            if let Some(t) = bytes.get_mut(36..44) {
                t.copy_from_slice(&to.to_le_bytes());
            }
        }
    } else if rng.chance(0.4) {
        let text = String::from_utf8(bytes.clone()).ok()?;
        let from = ["Object Type 14033", "Object Type 14010"];
        let to = *rng.choose(&["Object Type 14099", "Object Type 1"])?;
        let mut t = text.clone();
        for f in from {
            if let Some(pos) = t.find(f) {
                t.replace_range(pos..pos + f.len(), to);
                break;
            }
        }
        bytes = t.into_bytes();
    }
    Some(Inputs::ReadDb {
        read_db: ReadDbIn {
            binary,
            version,
            bytes,
        },
    })
}

fn gen_case(rng: &mut Rng) -> Option<Inputs> {
    if rng.chance(0.05) {
        return gen_read_db(rng);
    }
    let outside = rng.chance(0.05);
    let (c, d) = gen_cos(rng, outside);
    let mut range = range_of(&c);
    let nops = if rng.chance(0.7) {
        1
    } else {
        2 + rng.below(3) as usize
    };
    let ops = (0..nops)
        .map(|_| gen_op(rng, &c, d, &mut range, outside))
        .collect();
    Some(Inputs::Object {
        cos: Box::new(c),
        ops,
    })
}

/// Generated `assert_valid` cases (their own seed stream, `<unit>:assert_valid`).
pub const ASSERT_VALID_CASES: usize = 300;

/// An `assert_valid` case: a generated curve on a B-spline surface (any owner flag,
/// domain and cache; a fifth with the UV curve leaving the domain), any test level,
/// with or without a report list.
fn gen_assert_valid(rng: &mut Rng) -> Inputs {
    let outside = rng.chance(0.2);
    let (c, _) = gen_cos(rng, outside);
    Inputs::Object {
        cos: Box::new(c),
        ops: vec![OpIn::AssertValid {
            level: *rng.choose(&[0u8, 1, 2, 4]).unwrap_or(&0),
            list: rng.chance(0.8),
        }],
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "curve.crv_on_surf"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            if let Some(i) = gen_case(rng).filter(keeps_contract) {
                return serde_json::to_value(i).ok();
            }
        }
        None
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let main = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        let mut v = default_cases(self, &main)?;
        // AssertValid report records: their own seed stream (the other
        // generated cases are unchanged); named `avNNNN`
        let av = CaseContext {
            seeds: ctx.seeds.max(ASSERT_VALID_CASES),
            ..ctx.clone()
        };
        for mut c in av.generated_cases("curve.crv_on_surf:assert_valid", |rng| {
            let i = (0..64)
                .map(|_| gen_assert_valid(rng))
                .find(keeps_contract)?;
            serde_json::to_value(i).ok()
        }) {
            c.name = c.name.replacen("gen", "av", 1);
            c.unit = self.id().to_string();
            v.push(c);
        }
        Ok(v)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i = case.parse()?;
        rust(&i)
    }
}
