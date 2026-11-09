//! Runner for unit `curve.ellipse_p1`: `SmEllipse` / `SmCircle` / `SmPolarConversion`,
//! part 1.
//!
//! Ported from upstream `source/SMLib/src/SmEllipse.cpp` and `SmCircle.cpp`; checked
//! against its golden. Rust: `solidcore::curve::ellipse`.
//!
//! A case is a construction of curve A (`build`: a constructor, `CreateCanonical` or
//! `ReadFromDB` of a record written by [`record_bytes`]) and a program of operations
//! (`ops`) run on it in order; the Rust side produces a flat record (statuses `0` = `SM_SUCCESS`, else the `SmStatus`; `1007` for a Rust `InvalidInput`,
//! `-1` for a Rust error upstream cannot produce), reported as the values `rec`. A case
//! whose Rust side reaches an unported unit is deferred as a whole.
//!
//! Deferred cases: `CreateSimpleOffset` of a non-circular ellipse or of a circle with an
//! out-of-plane normal (`SmCurve::IsArc` / the offset approximation), and point solves
//! that reach unported solver pieces (see the report's `deferred_by_unit`).

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p1::GlobalPointSolveOptions;
use solidcore::curve::curve::p2::{CIRCLE_TYPE, ELLIPSE_TYPE};
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::geom::assert_array::{AssertArray, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::geom_utility::p1::circle_from_3_points;
use solidcore::geom::global_solver::{SolutionRequested, SolverOperation};
use solidcore::geom::object::NotifyOperation;
use solidcore::geom::solution_array::{ObjectKind, Solution, SolutionObject};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_curve_n_lib::TrimMode;
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::topology::database_io::{DatabaseReader, FileType};
use solidcore::SolidError;

use crate::case::{CaseContext, CaseSource};
use crate::cases::contract;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `curve.ellipse_p1` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// The key of curve A in assert reports.
const KEY_A: ObjectKey = ObjectKey(1);
/// The key of A's polar converter in assert reports.
const KEY_POLAR: ObjectKey = ObjectKey(2);

// ---- inputs -------------------------------------------------------------------------

fn three() -> u64 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EllipseJ {
    c: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    domain: [f64; 2],
    radii: [f64; 2],
    #[serde(default = "three")]
    dim: u64,
    #[serde(default)]
    inside_out: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    nurb_from: Option<Box<EllipseJ>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CircleJ {
    c: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    domain: [f64; 2],
    r: f64,
    #[serde(default = "three")]
    dim: u64,
    #[serde(default)]
    inside_out: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    nurb_from: Option<Box<EllipseJ>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RecordJ {
    head: [f64; 13],
    inside_out: i32,
    possible: i32,
    knots: Vec<f64>,
    angles: Vec<f64>,
    exact: Vec<i32>,
    n: i64,
    p: i16,
    rat: i16,
    dim: i16,
    kt: u64,
    form: u64,
    vals: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum BuildJ {
    Ellipse(EllipseJ),
    Circle(CircleJ),
    Circle3 {
        points: [[f64; 3]; 3],
        #[serde(default = "three")]
        dim: u64,
        #[serde(default)]
        closed: bool,
    },
    EllipseCanonical {
        frame: [[f64; 3]; 3],
        radii: [f64; 2],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        interval: Option<[f64; 2]>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        inside_out: Option<bool>,
    },
    CircleCanonical {
        frame: [[f64; 3]; 3],
        r: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        interval: Option<[f64; 2]>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        inside_out: Option<bool>,
    },
    ReadDb {
        circle: bool,
        #[serde(default = "three")]
        dim: u64,
        binary: bool,
        version: u64,
        record: RecordJ,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ParamJ {
    Raw(f64),
    Frac(f64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OtherJ {
    Ellipse(EllipseJ),
    Circle(CircleJ),
    BsplineOfA,
}

/// The curve tested by `SmCircle::IsNurbCurveCircle`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SourceJ {
    /// A itself.
    A,
    /// A plain B-spline with A's NURB.
    BsplineOfA,
    /// A raw 3D NURB: homogeneous `x y z w` points (`w` = `NOW` when non-rational).
    Raw {
        degree: usize,
        cpts: Vec<[f64; 4]>,
        knots: Vec<f64>,
    },
}

fn full() -> [f64; 2] {
    [0.0, 1.0]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "op")]
enum OpJ {
    State {
        #[serde(default)]
        types: Vec<i64>,
    },
    Copy,
    ToStep {
        t: ParamJ,
    },
    ToPolar {
        t: ParamJ,
    },
    Adjust {
        domain: [f64; 2],
    },
    Gps {
        #[serde(default)]
        step: bool,
        #[serde(default = "full")]
        ivl: [f64; 2],
        sop: i32,
        p: [f64; 3],
        tol: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<f64>,
        #[serde(default)]
        req: i32,
    },
    Join {
        end: u64,
        other: OtherJ,
        other_end: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gap: Option<f64>,
    },
    Reverse {
        #[serde(default = "full")]
        ivl: [f64; 2],
    },
    Trim {
        ivl: [f64; 2],
        mode: u64,
    },
    Assert {
        #[serde(default)]
        given: bool,
    },
    IsCurrent,
    PolarAssert,
    Offset {
        tol: f64,
        normal: [f64; 3],
        dist: f64,
        #[serde(default)]
        matched: bool,
    },
    SetRadius {
        r: f64,
    },
    MakeNurb,
    #[serde(rename = "to_2d")]
    To2d,
    EditParam {
        domain: [f64; 2],
        #[serde(default)]
        notify: bool,
    },
    Notify {
        post: bool,
    },
    IsArc,
    IsNurbCircle {
        source: SourceJ,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scale: Option<f64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    build: BuildJ,
    #[serde(default)]
    ops: Vec<OpJ>,
}

// ---- input contract ---------------------------------------------------------------

/// The largest magnitude of a coordinate, angle, radius or other scalar.
const MAX_COORD: f64 = 1e6;
/// The largest magnitude of a domain end, in degrees.
const MAX_ANGLE: f64 = 720.0;
/// The most ops of a case.
const MAX_OPS: usize = 64;
/// The most type codes of a `State` op.
const MAX_TYPES: usize = 64;
/// The most control points of a raw NURB.
const MAX_RAW_POINTS: usize = 65;
/// The largest degree of a raw NURB.
const MAX_RAW_DEGREE: usize = 8;
/// The weight range of a rational raw NURB.
const RAW_WEIGHTS: [f64; 2] = [1e-3, 1e3];
/// The largest control point index and polar table size of a database record.
const MAX_DB_INDEX: i64 = 64;

fn fin(x: f64) -> bool {
    contract::finite(x, MAX_COORD)
}

fn vec3(v: &[f64; 3]) -> bool {
    contract::vec3(v, MAX_COORD)
}

fn nonzero(v: [f64; 3]) -> bool {
    contract::len_sq(v) >= 1e-12
}

fn dim(d: u64) -> bool {
    d == 2 || d == 3
}

/// A domain whose ends are within [`MAX_ANGLE`] degrees and span `[1e-4, 360]` degrees.
fn domain_ok([d0, d1]: [f64; 2]) -> bool {
    !d0.is_nan()
        && !d1.is_nan()
        && d0.abs() <= MAX_ANGLE
        && d1.abs() <= MAX_ANGLE
        && (1e-4..=360.0).contains(&(d1 - d0).abs())
}

fn positive(r: f64) -> bool {
    !r.is_nan() && r > 0.0
}

/// An ellipse upstream builds: non-zero axes, a domain as [`domain_ok`] takes it, positive
/// radii, finite data, dimension 2 or 3, and a `pOptNurb` source only where one may be
/// given (never nested).
fn ellipse(e: &EllipseJ, with_nurb: bool) -> bool {
    nonzero(e.x)
        && nonzero(e.y)
        && domain_ok(e.domain)
        && e.radii.iter().all(|&r| positive(r))
        && [&e.c, &e.x, &e.y].iter().all(|v| vec3(v))
        && e.domain.iter().chain(&e.radii).all(|&x| fin(x))
        && dim(e.dim)
        && e.nurb_from
            .as_ref()
            .is_none_or(|src| with_nurb && ellipse(src, false))
}

/// A circle upstream builds, as [`ellipse`] with one radius.
fn circle(c: &CircleJ) -> bool {
    nonzero(c.x)
        && nonzero(c.y)
        && domain_ok(c.domain)
        && positive(c.r)
        && [&c.c, &c.x, &c.y].iter().all(|v| vec3(v))
        && c.domain.iter().all(|&x| fin(x))
        && fin(c.r)
        && dim(c.dim)
        && c.nurb_from.as_ref().is_none_or(|src| ellipse(src, false))
}

/// A canonical frame of non-zero axes, finite radii and, when given, a domain as
/// [`domain_ok`] takes it.
fn canonical(frame: &[[f64; 3]; 3], radii: &[f64], interval: Option<[f64; 2]>) -> bool {
    nonzero(frame[1])
        && nonzero(frame[2])
        && frame.iter().all(vec3)
        && radii.iter().all(|&r| fin(r))
        && interval.is_none_or(domain_ok)
        && interval.unwrap_or([0.0, 360.0]).iter().all(|&x| fin(x))
}

/// A database record upstream reads: finite values within range, flags in range, polar
/// tables of at most [`MAX_DB_INDEX`] values with as many angles as knots and, for a usable
/// converter, a non-empty angle table with an exactness flag per arc (upstream reads them
/// without bounds checks), `1 ≤ p ≤ n ≤ MAX_DB_INDEX`, the exact number of values, positive
/// weights and non-decreasing knots.
fn record(r: &RecordJ) -> bool {
    let max = MAX_DB_INDEX as usize;
    let flag = |x: i32| (0..=1).contains(&x);
    let head = r.head.iter().all(|&x| fin(x))
        && flag(r.inside_out)
        && flag(r.possible)
        && r.knots.len() <= max
        && r.angles.len() == r.knots.len()
        && r.exact.len() <= max
        && r.knots.iter().chain(&r.angles).all(|&x| fin(x))
        && r.exact.iter().all(|&x| flag(x))
        && !(r.possible != 0 && (r.angles.is_empty() || r.exact.len() + 1 < r.angles.len()))
        && (1..=MAX_DB_INDEX).contains(&r.n)
        && r.p >= 1
        && i64::from(r.p) <= r.n
        && (0..=1).contains(&r.rat)
        && (2..=3).contains(&r.dim)
        && r.kt <= 4
        && r.form <= 6;
    if !head {
        return false;
    }
    let per = 2 + usize::from(r.dim != 2) + usize::from(r.rat != 0);
    let (Ok(n_pts), Ok(n_knots)) = (
        usize::try_from(r.n + 1),
        usize::try_from(r.n + i64::from(r.p) + 2),
    ) else {
        return false;
    };
    r.vals.len() == n_pts * per + n_knots
        && r.vals.iter().all(|&x| fin(x))
        && (r.rat == 0
            || r.vals
                .chunks(per)
                .take(n_pts)
                .all(|c| c.last().is_some_and(|&w| w > 0.0)))
        && r.vals
            .get(n_pts * per..)
            .is_none_or(|k| k.windows(2).all(|w| w.first() <= w.last()))
}

fn build_ok(b: &BuildJ) -> bool {
    match b {
        BuildJ::Ellipse(e) => ellipse(e, true),
        BuildJ::Circle(c) => circle(c),
        BuildJ::Circle3 { points, dim: d, .. } => points.iter().all(vec3) && dim(*d),
        BuildJ::EllipseCanonical {
            frame,
            radii,
            interval,
            ..
        } => radii.iter().all(|&r| positive(r)) && canonical(frame, radii, *interval),
        BuildJ::CircleCanonical {
            frame, r, interval, ..
        } => canonical(frame, &[*r], *interval),
        BuildJ::ReadDb {
            dim: d,
            binary,
            version,
            record: rec,
            ..
        } => {
            *version <= 1000
                && record(rec)
                && record_bytes(*binary, *version, rec).len() <= 1 << 20
                && dim(*d)
        }
    }
}

fn param_ok(t: ParamJ) -> bool {
    match t {
        ParamJ::Raw(v) => fin(v),
        ParamJ::Frac(v) => (-2.0..=3.0).contains(&v) && fin(v),
    }
}

/// A normalised interval with both ends in `[-2, 3]`.
fn frac(f: [f64; 2]) -> bool {
    f.iter().all(|&x| (-2.0..=3.0).contains(&x) && fin(x))
}

/// A unit interval `0 ≤ a ≤ b ≤ 1`.
fn unit_interval([a, b]: [f64; 2]) -> bool {
    (0.0..=1.0).contains(&a) && (0.0..=1.0).contains(&b) && a <= b
}

/// A raw clamped 3D NURB: 2 to [`MAX_RAW_POINTS`] points, `1 ≤ p ≤ min(n, MAX_RAW_DEGREE)`,
/// `n + p + 2` clamped knots within range, one rationality with weights in [`RAW_WEIGHTS`],
/// and Euclidean points within range.
fn raw_nurb(degree: usize, cpts: &[[f64; 4]], knots: &[f64]) -> bool {
    if !(2..=MAX_RAW_POINTS).contains(&cpts.len()) {
        return false;
    }
    let n = cpts.len() - 1;
    let rational = cpts.first().is_some_and(|p| p[3] != NOW);
    (1..=MAX_RAW_DEGREE.min(n)).contains(&degree)
        && knots.len() == n + degree + 2
        && knots.iter().all(|&k| fin(k))
        && contract::clamped(knots, degree).is_some()
        && cpts.iter().all(|&[x, y, z, w]| {
            let w_e = if rational { w } else { 1.0 };
            let weight = if rational {
                (RAW_WEIGHTS[0]..=RAW_WEIGHTS[1]).contains(&w)
            } else {
                w == NOW
            };
            weight
                && [x, y, z]
                    .iter()
                    .all(|c| c.is_finite() && (c / w_e).abs() <= MAX_COORD)
        })
}

fn op_ok(op: &OpJ) -> bool {
    let tol = |x: f64| x >= 0.0 && fin(x);
    match op {
        OpJ::State { types } => {
            types.len() <= MAX_TYPES && types.iter().all(|t| (0..=100_000).contains(t))
        }
        OpJ::Copy
        | OpJ::Assert { .. }
        | OpJ::IsCurrent
        | OpJ::PolarAssert
        | OpJ::MakeNurb
        | OpJ::To2d
        | OpJ::Notify { .. }
        | OpJ::IsArc => true,
        OpJ::ToStep { t } | OpJ::ToPolar { t } => param_ok(*t),
        OpJ::Adjust { domain } => domain_ok(*domain) && domain.iter().all(|&x| fin(x)),
        OpJ::Gps {
            step,
            ivl,
            sop,
            p,
            tol: t,
            target,
            req,
        } => {
            [0, 3, 4, 5, 6].contains(sop)
                && (0..=1).contains(req)
                && !(*sop == 6 && target.is_none())
                && (*step || (unit_interval(*ivl) && frac(*ivl)))
                && vec3(p)
                && tol(*t)
                && fin(target.unwrap_or(0.0))
        }
        OpJ::Join {
            end,
            other,
            other_end,
            gap,
        } => {
            let other = match other {
                OtherJ::Ellipse(e) => ellipse(e, false),
                OtherJ::Circle(c) => c.nurb_from.is_none() && circle(c),
                OtherJ::BsplineOfA => true,
            };
            *end <= 1 && *other_end <= 1 && other && tol(gap.unwrap_or(0.0))
        }
        OpJ::Reverse { ivl } => unit_interval(*ivl) && frac(*ivl),
        OpJ::Trim { ivl, mode } => *mode <= 2 && frac(*ivl),
        OpJ::Offset {
            tol: t,
            normal,
            dist,
            ..
        } => positive(*t) && nonzero(*normal) && tol(*t) && vec3(normal) && fin(*dist),
        OpJ::SetRadius { r } => positive(*r) && fin(*r),
        OpJ::EditParam { domain, .. } => {
            !domain[0].is_nan()
                && !domain[1].is_nan()
                && domain[0] < domain[1]
                && domain.iter().all(|&x| fin(x))
        }
        OpJ::IsNurbCircle { source, scale } => {
            let source = match source {
                SourceJ::A | SourceJ::BsplineOfA => true,
                SourceJ::Raw {
                    degree,
                    cpts,
                    knots,
                } => raw_nurb(*degree, cpts, knots),
            };
            source && fin(scale.unwrap_or(0.0))
        }
    }
}

/// Whether a case keeps the input contract ported from the reference's wrapper: a curve A
/// upstream builds or reads ([`build_ok`]), at most [`MAX_OPS`] ops, the circle-only ops
/// only on a circle, and each op's arguments ([`op_ok`]).
fn keeps_contract(i: &Inputs) -> bool {
    let circle = match &i.build {
        BuildJ::Ellipse(_) | BuildJ::EllipseCanonical { .. } => false,
        BuildJ::Circle(_) | BuildJ::Circle3 { .. } | BuildJ::CircleCanonical { .. } => true,
        BuildJ::ReadDb { circle, .. } => *circle,
    };
    i.ops.len() <= MAX_OPS
        && (circle
            || !i
                .ops
                .iter()
                .any(|o| matches!(o, OpJ::SetRadius { .. } | OpJ::IsArc)))
        && build_ok(&i.build)
        && i.ops.iter().all(op_ok)
}

// ---- the database record -------------------------------------------------------------

/// The stream bytes of `r` as `SmEllipse::WriteToDB` writes them (ASCII: one group per
/// line with upstream's labels, values as `{:.17e}`, the polar arrays broken after the
/// elements with index 5, 10, ...; binary: native-endian values). The fields are written
/// as they are, so a record with inconsistent sizes reaches the reader as hostile input.
fn record_bytes(binary: bool, version: u64, r: &RecordJ) -> Vec<u8> {
    let per = 2 + usize::from(r.dim != 2) + usize::from(r.rat != 0);
    let n_pts = usize::try_from(r.n.saturating_add(1)).unwrap_or(0);
    let (pts, knots) = r.vals.split_at(n_pts.saturating_mul(per).min(r.vals.len()));
    let (nk, na, ne) = (
        r.knots.len() as u64,
        r.angles.len() as u64,
        r.exact.len() as u64,
    );
    if binary {
        let mut b = Vec::new();
        for x in &r.head {
            b.extend_from_slice(&x.to_ne_bytes());
        }
        b.extend_from_slice(&r.inside_out.to_ne_bytes());
        b.extend_from_slice(&r.possible.to_ne_bytes());
        b.extend_from_slice(&nk.to_ne_bytes());
        b.extend_from_slice(&na.to_ne_bytes());
        b.extend_from_slice(&ne.to_ne_bytes());
        b.extend_from_slice(&0i32.to_ne_bytes());
        for x in r.knots.iter().chain(&r.angles) {
            b.extend_from_slice(&x.to_ne_bytes());
        }
        for x in &r.exact {
            b.extend_from_slice(&x.to_ne_bytes());
        }
        b.extend_from_slice(&r.n.to_ne_bytes());
        b.extend_from_slice(&r.p.to_ne_bytes());
        b.extend_from_slice(&r.rat.to_ne_bytes());
        b.extend_from_slice(&r.dim.to_ne_bytes());
        if version > 32 {
            b.extend_from_slice(&r.kt.to_ne_bytes());
            b.extend_from_slice(&r.form.to_ne_bytes());
        }
        for x in &r.vals {
            b.extend_from_slice(&x.to_ne_bytes());
        }
        b
    } else {
        let f = |x: f64| format!("{x:.17e}");
        let [ox, oy, oz, xx, xy, xz, yx, yy, yz, d0, d1, rx, ry] = r.head;
        let mut t = String::new();
        let _ = writeln!(t, "{} {} {} SmEllipse Axis Origin ", f(ox), f(oy), f(oz));
        let _ = writeln!(t, "{} {} {} SmEllipse X Axis", f(xx), f(xy), f(xz));
        let _ = writeln!(t, "{} {} {} SmEllipse Y Axis", f(yx), f(yy), f(yz));
        let _ = writeln!(
            t,
            "{} {} SmEllipse angular arc domain in degrees ",
            f(d0),
            f(d1)
        );
        let _ = writeln!(t, "{} SmEllipse X_Axis radius ", f(rx));
        let _ = writeln!(t, "{} SmEllipse Y_Axis radius ", f(ry));
        let _ = writeln!(t, "{} SmEllipse InsideOut Flag ", r.inside_out);
        t.push_str(" SmEllipse->PolarConverter \n");
        let _ = writeln!(
            t,
            "{} SmPolarConversion PolarConversionPossible - TRUE=tables are initialized ",
            r.possible
        );
        let _ = writeln!(t, "{nk} SmPolarConversion Number of Natural Knots ");
        let _ = writeln!(t, "{na} SmPolarConversion Number of Angles ");
        let _ = writeln!(t, "{ne} SmPolarConversion Number of ExactConversion Flags ");
        t.push_str(
            "0 SmPolarConversion TRUE = Has Curve for stored conversions, FALSE = back pointer \n",
        );
        let mut put_list = |items: Vec<String>| {
            let n = items.len();
            for (ii, s) in items.into_iter().enumerate() {
                t.push_str(&s);
                t.push(' ');
                if ii > 0 && ii % 5 == 0 {
                    t.push('\n');
                }
            }
            if n > 0 && !n.is_multiple_of(5) {
                t.push('\n');
            }
        };
        put_list(r.knots.iter().map(|&x| f(x)).collect());
        put_list(r.angles.iter().map(|&x| f(x)).collect());
        put_list(r.exact.iter().map(ToString::to_string).collect());
        t.push_str("Has No Copy of SmPolarConversion->CurveBeingConverted \n");
        let _ = write!(t, "{}\n{}\n{}\n{}\n", r.n, r.p, r.rat, r.dim);
        if version > 32 {
            let _ = write!(t, "{}\n{}\n", r.kt, r.form);
        }
        for c in pts.chunks(per) {
            let s: Vec<String> = c.iter().map(|&x| f(x)).collect();
            t.push_str(&s.join(" "));
            t.push('\n');
        }
        for &x in knots {
            t.push_str(&f(x));
            t.push('\n');
        }
        t.into_bytes()
    }
}

// ---- Rust side --------------------------------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

fn b(x: bool) -> f64 {
    if x {
        1.0
    } else {
        0.0
    }
}

/// The `SmStatus` of a Rust error; `1007` for `InvalidInput`, `-1` for one upstream
/// cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
        e if solidcore::error::nurbs::upstream_status(e).is_some() => {
            f64::from(solidcore::error::nurbs::upstream_status(e).unwrap_or(-1))
        }
        _ => -1.0,
    }
}

/// A failed Rust operation: the whole case is deferred, or a harness error.
enum Fail {
    Deferred(Box<Output>),
    Harness(HarnessError),
}

impl From<HarnessError> for Fail {
    fn from(h: HarnessError) -> Self {
        Self::Harness(h)
    }
}

/// `Ok(v)`, an op status (pushed, `None`), or a deferral.
fn st<T>(o: &mut Vec<f64>, r: solidcore::Result<T>) -> Result<Option<T>, Fail> {
    match r {
        Ok(v) => {
            o.push(0.0);
            Ok(Some(v))
        }
        Err(e) => {
            if let Some(d) = Output::unported(&e) {
                return Err(Fail::Deferred(Box::new(d)));
            }
            o.push(status(&e));
            Ok(None)
        }
    }
}

/// A deferral for an unported error, else the value.
fn defer<T>(r: solidcore::Result<T>) -> Result<Result<T, SolidError>, Fail> {
    match r {
        Ok(v) => Ok(Ok(v)),
        Err(e) => match Output::unported(&e) {
            Some(d) => Err(Fail::Deferred(Box::new(d))),
            None => Ok(Err(e)),
        },
    }
}

fn put_vec(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn put_ivl(o: &mut Vec<f64>, e: &Extent1d) {
    o.extend([e.min(), e.max()]);
}

fn put_chars(o: &mut Vec<f64>, s: &str) {
    o.extend(s.bytes().map(f64::from));
    o.push(-1.0);
}

fn put_nurb(o: &mut Vec<f64>, c: &BSplineCurve) {
    match c.gw_nurb() {
        None => o.push(0.0),
        Some(cur) => {
            o.extend([1.0, cur.n() as f64, cur.degree() as f64, cur.m() as f64]);
            for p in cur.cpts() {
                o.extend([p.x, p.y, p.z, p.w]);
            }
            o.extend_from_slice(cur.knots());
        }
    }
}

fn put_state(o: &mut Vec<f64>, e: &Ellipse) {
    o.push(e.type_code() as f64);
    let pos = e.position();
    put_vec(o, pos.origin());
    put_vec(o, pos.x_axis());
    put_vec(o, pos.y_axis());
    put_ivl(o, &e.step_interval());
    o.extend([
        e.x_radius(),
        e.y_radius(),
        b(e.inside_out()),
        e.base().dim() as f64,
        f64::from(e.base().form() as i32),
    ]);
    put_nurb(o, e.base());
    let pc = e.polar_converter();
    o.extend([b(pc.is_polar_conversion_possible()), b(pc.has_curve())]);
    o.push(pc.natural_knots().len() as f64);
    o.extend_from_slice(pc.natural_knots());
    o.push(pc.angles().len() as f64);
    o.extend_from_slice(pc.angles());
    o.push(pc.exact_conversion().len() as f64);
    o.extend(pc.exact_conversion().iter().map(|&x| b(x)));
}

/// The handle of A in its global solve (a curve with A's natural interval, as
/// upstream's `this`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Handle(Extent1d);

impl SolutionObject for Handle {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Curve
    }

    fn natural_interval(&self) -> Option<Extent1d> {
        Some(self.0)
    }
}

fn put_sols<O>(o: &mut Vec<f64>, sols: &[Solution<O>]) {
    o.push(sols.len() as f64);
    for s in sols {
        o.extend([
            f64::from(s.solution_type as i32),
            s.num_variables as f64,
            s.start.solution_value,
            s.start.parameters[0],
        ]);
    }
}

fn put_reports(o: &mut Vec<f64>, list: &AssertArray) {
    o.push(list.len() as f64);
    let id = |k: Option<ObjectKey>| match k {
        None => -1.0,
        Some(KEY_A) => 0.0,
        Some(KEY_POLAR) => 1.0,
        Some(_) => 2.0,
    };
    for r in list.reports() {
        o.extend([
            r.reporting_type as f64,
            r.list_index as f64,
            r.test_index as f64,
            f64::from(r.assert_type.code()),
            id(r.owner),
            id(r.reporter),
            r.owner_type as f64,
            r.tol3d,
            r.value,
        ]);
    }
}

/// The ellipse of `e`; `Err` only for an unported callee.
fn build_ellipse(k: &Kernel, e: &EllipseJ) -> solidcore::Result<Ellipse> {
    let src = match &e.nurb_from {
        Some(s) => Some(build_ellipse(k, s)?),
        None => None,
    };
    let opts = EllipseOptions {
        dim: e.dim,
        nurb: src.as_ref().map(Ellipse::base),
        inside_out: e.inside_out,
    };
    Ellipse::new(
        k,
        v3(e.c),
        v3(e.x),
        v3(e.y),
        Extent1d::from_min_max(e.domain[0], e.domain[1]),
        e.radii[0],
        e.radii[1],
        &opts,
    )
}

/// The circle of `c`; `Err` only for an unported callee.
fn build_circle(k: &Kernel, c: &CircleJ) -> solidcore::Result<Ellipse> {
    let src = match &c.nurb_from {
        Some(s) => Some(build_ellipse(k, s)?),
        None => None,
    };
    let opts = EllipseOptions {
        dim: c.dim,
        nurb: src.as_ref().map(Ellipse::base),
        inside_out: c.inside_out,
    };
    Ellipse::new_circle(
        k,
        v3(c.c),
        v3(c.x),
        v3(c.y),
        Extent1d::from_min_max(c.domain[0], c.domain[1]),
        c.r,
        &opts,
    )
}

fn frame_of(f: &[[f64; 3]; 3]) -> Axis2Placement {
    let mut place = Axis2Placement::default();
    let _ = place.set_canonical(v3(f[0]), v3(f[1]), v3(f[2]));
    place
}

/// Builds A; pushes the construction status. `None` when not built.
fn build(k: &Kernel, bj: &BuildJ, o: &mut Vec<f64>) -> Result<Option<Ellipse>, Fail> {
    let built = match bj {
        BuildJ::Ellipse(e) => build_ellipse(k, e),
        BuildJ::Circle(c) => build_circle(k, c),
        BuildJ::Circle3 {
            points,
            dim,
            closed,
        } => {
            let [p0, p1, p2] = points.map(v3);
            // (the constructor ignores this status; the shim does not build then)
            if let Err(e) = defer(circle_from_3_points(p0, p1, p2, *dim, *closed))? {
                o.push(status(&e));
                return Ok(None);
            }
            Ellipse::circle_from_3_points(k, p0, p1, p2, *dim, *closed)
        }
        BuildJ::EllipseCanonical {
            frame,
            radii,
            interval,
            inside_out,
        } => {
            let ivl = interval.map(|d| Extent1d::from_min_max(d[0], d[1]));
            match defer(Ellipse::create_canonical(
                k,
                &frame_of(frame),
                radii[0],
                radii[1],
                ivl.as_ref(),
                *inside_out,
            ))? {
                Ok(e) => Ok(e),
                Err(e) => {
                    o.push(status(&e));
                    return Ok(None);
                }
            }
        }
        BuildJ::CircleCanonical {
            frame,
            r,
            interval,
            inside_out,
        } => {
            let ivl = interval.map(|d| Extent1d::from_min_max(d[0], d[1]));
            match defer(Ellipse::create_canonical_circle(
                k,
                &frame_of(frame),
                *r,
                ivl.as_ref(),
                *inside_out,
            ))? {
                Ok(e) => Ok(e),
                Err(e) => {
                    o.push(status(&e));
                    return Ok(None);
                }
            }
        }
        BuildJ::ReadDb {
            circle,
            dim,
            binary,
            version,
            record,
        } => {
            let bytes = record_bytes(*binary, *version, record);
            let mut db = DatabaseReader::new();
            db.open_for_read(
                bytes,
                if *binary {
                    FileType::Binary
                } else {
                    FileType::Ascii
                },
            );
            let t = if *circle { CIRCLE_TYPE } else { ELLIPSE_TYPE };
            match defer(Curve::read_from_db(k, t, &mut db, *dim, *version))? {
                Ok(c) => match c.into_kind() {
                    CurveKind::Ellipse(e) => Ok(e),
                    _ => return Err(harness("read curve is not an ellipse").into()),
                },
                Err(e) => {
                    o.push(status(&e));
                    return Ok(None);
                }
            }
        }
    };
    match defer(built)? {
        Ok(e) => {
            o.push(0.0);
            Ok(Some(e))
        }
        Err(e) => Err(harness(format!("construction failed: {e}")).into()),
    }
}

fn frac_ivl(c: &BSplineCurve, f: [f64; 2]) -> Extent1d {
    let nat = c.natural_interval();
    Extent1d::from_min_max(nat.evaluate(f[0]), nat.evaluate(f[1]))
}

fn param(c: &BSplineCurve, p: ParamJ) -> f64 {
    match p {
        ParamJ::Raw(v) => v,
        ParamJ::Frac(v) => c.natural_interval().evaluate(v),
    }
}

fn sop_of(code: i32) -> Result<SolverOperation, Fail> {
    SolverOperation::from_code(code).ok_or_else(|| harness("solver operation").into())
}

fn req_of(code: i32) -> SolutionRequested {
    if code == 0 {
        SolutionRequested::Single
    } else {
        SolutionRequested::All
    }
}

fn run_op(k: &Kernel, a: &mut Ellipse, op: &OpJ, o: &mut Vec<f64>) -> Result<(), Fail> {
    match op {
        OpJ::State { types } => {
            o.push(0.0);
            o.push(a.type_code() as f64);
            put_chars(o, a.type_string());
            put_chars(o, a.class_string());
            o.push(if a.is_circle_class() { a.radius() } else { 0.0 });
            for &t in types {
                o.push(b(a.kind_of(t)));
            }
        }
        OpJ::Copy => {
            if let Some(c) = st(o, a.copy(k))? {
                let eq = a.type_code() == c.type_code() && a.approx_eq(&c);
                o.push(b(eq));
                put_state(o, &c);
            }
        }
        OpJ::ToStep { t } => {
            let t = param(a.base(), *t);
            if let Some(v) = st(o, a.convert_t_from_nurbs_to_step(k, t))? {
                o.push(v);
            }
        }
        OpJ::ToPolar { t } => {
            let t = param(a.base(), *t);
            if let Some((v, exact)) =
                st(o, a.polar_converter().convert_to_polar_parameter(k, a, t))?
            {
                o.extend([v, b(exact)]);
            }
        }
        OpJ::Adjust { domain } => {
            let d = Extent1d::from_min_max(domain[0], domain[1]);
            st(o, a.adjust_step_interval(k, &d, None))?;
        }
        OpJ::Gps {
            step,
            ivl,
            sop,
            p,
            tol,
            target,
            req,
        } => {
            let op = sop_of(*sop)?;
            if *step {
                let r = a.global_point_solve_step(k, op, v3(*p), *tol, *target, req_of(*req));
                if let Some(sols) = st(o, r)? {
                    put_sols(o, &sols);
                }
            } else {
                let c = Curve::from(a.clone());
                let i = frac_ivl(a.base(), *ivl);
                let opts = GlobalPointSolveOptions {
                    target_distance: *target,
                    handle: Some(Handle(a.base().natural_interval())),
                    ..GlobalPointSolveOptions::default()
                };
                let r = c.global_point_solve(k, &i, op, v3(*p), *tol, req_of(*req), &opts);
                if let Some(sols) = st(o, r)? {
                    put_sols(o, sols.as_slice());
                }
            }
        }
        OpJ::Join {
            end,
            other,
            other_end,
            gap,
        } => {
            let other = match other {
                OtherJ::Ellipse(e) => match defer(build_ellipse(k, e))? {
                    Ok(e) => Curve::from(e),
                    Err(e) => return Err(harness(e.to_string()).into()),
                },
                OtherJ::Circle(c) => match defer(build_circle(k, c))? {
                    Ok(e) => Curve::from(e),
                    Err(e) => return Err(harness(e.to_string()).into()),
                },
                OtherJ::BsplineOfA => match a.base().gw_nurb() {
                    Some(n) => Curve::from(BSplineCurve::from_gw_nurb(3, n)),
                    None => return Err(harness("A without NURB").into()),
                },
            };
            let has_nurb = match other.kind() {
                CurveKind::Ellipse(e) => e.base().gw_nurb().is_some(),
                CurveKind::BSpline(c) => c.gw_nurb().is_some(),
                _ => false,
            };
            if !has_nurb {
                o.push(-2.0);
                return Ok(());
            }
            st(o, a.join_with(k, *end, &other, *other_end, *gap, None))?;
        }
        OpJ::Reverse { ivl } => {
            let old = frac_ivl(a.base(), *ivl);
            if let Some(n) = st(o, a.reverse_parameterization(k, &old, None))? {
                put_ivl(o, &n);
            }
        }
        OpJ::Trim { ivl, mode } => {
            let mut i = frac_ivl(a.base(), *ivl);
            let m = match mode {
                0 => TrimMode::Silent,
                1 => TrimMode::Notify,
                _ => TrimMode::IntervalOnly,
            };
            if st(o, a.trim(k, &mut i, m))?.is_some() {
                put_ivl(o, &i);
            }
        }
        OpJ::Assert { given } => {
            let mut list = AssertArray::new();
            let ok = match defer(a.assert_valid(k, Some(&mut list), *given, KEY_A, KEY_POLAR))? {
                Ok(v) => v,
                Err(e) => {
                    o.push(status(&e));
                    return Ok(());
                }
            };
            o.extend([0.0, b(ok)]);
            put_reports(o, &list);
        }
        OpJ::IsCurrent | OpJ::PolarAssert => {
            let mut list = AssertArray::new();
            let pc = a.polar_converter();
            let r = if matches!(op, OpJ::IsCurrent) {
                pc.is_current(k, a, Some(&mut list), KEY_POLAR)
            } else {
                pc.assert_valid(k, a, Some(&mut list), KEY_POLAR)
            };
            let ok = match defer(r)? {
                Ok(v) => v,
                Err(e) => {
                    o.push(status(&e));
                    return Ok(());
                }
            };
            o.extend([0.0, b(ok)]);
            put_reports(o, &list);
        }
        OpJ::Offset {
            tol,
            normal,
            dist,
            matched,
        } => {
            if let Some((c, gap)) = st(
                o,
                a.create_simple_offset(k, *tol, v3(*normal), *dist, *matched),
            )? {
                o.push(gap);
                match c.kind() {
                    CurveKind::Ellipse(e) => {
                        o.push(1.0);
                        put_state(o, e);
                    }
                    CurveKind::BSpline(bs) => {
                        o.extend([0.0, c.type_code() as f64, bs.dim() as f64]);
                        put_nurb(o, bs);
                    }
                    _ => return Err(harness("offset of unexpected kind").into()),
                }
            }
        }
        OpJ::SetRadius { r } => {
            a.set_radius(*r);
            st(o, a.make_nurb(k))?;
        }
        OpJ::MakeNurb => {
            st(o, a.make_nurb(k))?;
        }
        OpJ::To2d => {
            st(o, a.convert_to_2d(k))?;
        }
        OpJ::EditParam { domain, notify } => {
            let d = Extent1d::from_min_max(domain[0], domain[1]);
            st(o, a.edit_parameterization(k, &d, *notify, None))?;
        }
        OpJ::Notify { post } => {
            let n = if *post {
                NotifyOperation::PostEdit
            } else {
                NotifyOperation::PreEdit
            };
            st(o, a.notify(k, n))?;
        }
        OpJ::IsArc => {
            if let Some(arc) = st(o, a.is_arc(k, 5, 1e-6))? {
                let (f, r, s0, s1) = arc.ok_or_else(|| harness("circle without arc"))?;
                o.push(1.0);
                put_vec(o, f.origin());
                put_vec(o, f.x_axis());
                put_vec(o, f.y_axis());
                o.extend([r, s0, s1]);
            }
        }
        OpJ::IsNurbCircle { source, scale } => {
            let test = match source {
                SourceJ::A => Curve::from(a.clone()),
                SourceJ::BsplineOfA => match a.base().gw_nurb() {
                    Some(n) => Curve::from(BSplineCurve::from_gw_nurb(3, n)),
                    None => return Err(harness("A without NURB").into()),
                },
                SourceJ::Raw {
                    degree,
                    cpts,
                    knots,
                } => {
                    let pw = cpts
                        .iter()
                        .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
                        .collect();
                    let cur = NurbCurve::new(pw, *degree, knots.clone())
                        .map_err(|e| harness(format!("raw NURB: {e}")))?;
                    Curve::from(BSplineCurve::from_nurb(cur, 3, false))
                }
            };
            if let Some(got) = st(o, Ellipse::is_nurb_curve_circle(k, &test, *scale))? {
                o.push(b(got.is_some()));
                if let Some(c) = got {
                    o.push(b(c.kind_of(CIRCLE_TYPE)));
                    o.push(c.radius());
                    put_state(o, &c);
                }
            }
        }
    }
    Ok(())
}

fn op_code(op: &OpJ) -> f64 {
    match op {
        OpJ::State { .. } => 1.0,
        OpJ::Copy => 2.0,
        OpJ::ToStep { .. } => 3.0,
        OpJ::ToPolar { .. } => 4.0,
        OpJ::Adjust { .. } => 5.0,
        OpJ::Gps { step: true, .. } => 6.0,
        OpJ::Gps { step: false, .. } => 7.0,
        OpJ::Join { .. } => 8.0,
        OpJ::Reverse { .. } => 9.0,
        OpJ::Trim { .. } => 10.0,
        OpJ::Assert { .. } => 11.0,
        OpJ::IsCurrent => 12.0,
        OpJ::PolarAssert => 13.0,
        OpJ::Offset { .. } => 14.0,
        OpJ::SetRadius { .. } => 15.0,
        OpJ::MakeNurb => 16.0,
        OpJ::To2d => 17.0,
        OpJ::EditParam { .. } => 18.0,
        OpJ::Notify { .. } => 19.0,
        OpJ::IsArc => 20.0,
        OpJ::IsNurbCircle { .. } => 21.0,
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut o = Vec::new();
    let a = match build(&k, &i.build, &mut o) {
        Ok(a) => a,
        Err(Fail::Deferred(d)) => return Ok(*d),
        Err(Fail::Harness(h)) => return Err(h),
    };
    if let Some(mut a) = a {
        if a.base().gw_nurb().is_some() {
            for op in &i.ops {
                o.push(op_code(op));
                match run_op(&k, &mut a, op, &mut o) {
                    Ok(()) => {}
                    Err(Fail::Deferred(d)) => return Ok(*d),
                    Err(Fail::Harness(h)) => return Err(h),
                }
            }
        }
        put_state(&mut o, &a);
    }
    Ok(Output::new().values("rec", o))
}

// ---- generation -------------------------------------------------------------------

fn round(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

fn gen_dir(rng: &mut Rng) -> [f64; 3] {
    loop {
        let v = rng.point3(-1.0, 1.0);
        if v.iter().map(|x| x * x).sum::<f64>() > 0.05 {
            return v.map(round);
        }
    }
}

fn gen_domain(rng: &mut Rng) -> [f64; 2] {
    match rng.below(8) {
        0 => [0.0, 360.0],
        1 => [-180.0, 180.0],
        2 => [0.0, 90.0],
        3 => [
            *rng.choose(&[0.0, 1e-5, 2e-4, 179.9999, 180.0, 359.9999])
                .unwrap_or(&0.0),
            270.0,
        ],
        4 => {
            let a = round(rng.uniform(-360.0, 0.0));
            [a, a + 360.0]
        }
        5 => [
            0.0,
            *rng.choose(&[180.0, 180.0002, 359.9998, 1e-3, 45.0])
                .unwrap_or(&90.0),
        ],
        _ => {
            let a = round(rng.uniform(-360.0, 300.0));
            let l = round(rng.uniform(0.01, 360.0));
            [a, (a + l).min(360.0)]
        }
    }
}

/// A Y axis mostly perpendicular to `x` (sometimes skewed: `SetCanonical`
/// orthonormalises).
fn gen_y(rng: &mut Rng, x: [f64; 3]) -> [f64; 3] {
    if rng.chance(0.75) {
        let xv = v3(x);
        let mut t = v3(gen_dir(rng)).cross(xv);
        if t.length() < 0.1 {
            t = Vec3::new(-x[1], x[0], 0.3);
        }
        [round(t.x), round(t.y), round(t.z)]
    } else {
        gen_dir(rng)
    }
}

fn gen_ellipse(rng: &mut Rng, allow_nurb: bool) -> EllipseJ {
    let r1 = round(rng.uniform(0.05, 5.0));
    let r2 = if rng.chance(0.3) {
        r1
    } else {
        round(rng.uniform(0.05, 5.0))
    };
    let x = gen_dir(rng);
    let y = gen_y(rng, x);
    let mut e = EllipseJ {
        c: rng.point3(-5.0, 5.0).map(round),
        x,
        y,
        domain: gen_domain(rng),
        radii: [r1, r2],
        dim: if rng.chance(0.1) { 2 } else { 3 },
        inside_out: rng.chance(0.15),
        nurb_from: None,
    };
    if allow_nurb && rng.chance(0.08) {
        let mut src = e.clone();
        if rng.chance(0.5) {
            src.domain = gen_domain(rng);
        }
        e.nurb_from = Some(Box::new(src));
    }
    e
}

fn gen_circle(rng: &mut Rng, allow_nurb: bool) -> CircleJ {
    let e = gen_ellipse(rng, allow_nurb);
    CircleJ {
        c: e.c,
        x: e.x,
        y: e.y,
        domain: e.domain,
        r: e.radii[0],
        dim: e.dim,
        inside_out: rng.chance(0.3),
        nurb_from: e.nurb_from,
    }
}

fn gen_frame(rng: &mut Rng) -> [[f64; 3]; 3] {
    let x = gen_dir(rng);
    [rng.point3(-5.0, 5.0).map(round), x, gen_y(rng, x)]
}

/// The record `WriteToDB` would write for the Rust ellipse `e` (the tables and NURB
/// as built), sometimes with a perturbed head or flags.
fn gen_record(rng: &mut Rng, k: &Kernel, circle: bool) -> Option<RecordJ> {
    let e = if circle {
        build_circle(k, &gen_circle(rng, false)).ok()?
    } else {
        build_ellipse(k, &gen_ellipse(rng, false)).ok()?
    };
    let cur = e.base().gw_nurb()?;
    let pos = e.position();
    let (o, x, y) = (pos.origin(), pos.x_axis(), pos.y_axis());
    let d = e.step_interval();
    let mut head = [
        o.x,
        o.y,
        o.z,
        x.x,
        x.y,
        x.z,
        y.x,
        y.y,
        y.z,
        d.min(),
        d.max(),
        e.x_radius(),
        e.y_radius(),
    ];
    if rng.chance(0.15) {
        // a different placement or radius than the stored NURB
        let i = rng.below(13) as usize;
        if let Some(h) = head.get_mut(i) {
            *h += round(rng.uniform(-0.5, 0.5));
        }
    }
    let pc = e.polar_converter();
    let rat = i16::from(cur.cpts().iter().any(|c| c.w != -f64::MAX));
    let three_d = cur.cpts().iter().all(|c| c.z != f64::MAX);
    let mut vals = Vec::new();
    for c in cur.cpts() {
        vals.extend([c.x, c.y]);
        if three_d {
            vals.push(c.z);
        }
        if rat != 0 {
            vals.push(c.w);
        }
    }
    vals.extend_from_slice(cur.knots());
    let mut r = RecordJ {
        head,
        inside_out: i32::from(e.inside_out()),
        possible: i32::from(pc.is_polar_conversion_possible()),
        knots: pc.natural_knots().to_vec(),
        angles: pc.angles().to_vec(),
        exact: pc
            .exact_conversion()
            .iter()
            .map(|&b| i32::from(b))
            .collect(),
        n: cur.n() as i64,
        p: cur.degree() as i16,
        rat,
        dim: if three_d { 3 } else { 2 },
        kt: 1,
        form: if circle { 1 } else { 2 },
        vals,
    };
    match rng.below(10) {
        0 => r.possible = 0,
        1 => {
            // stored angles off by a little
            if let Some(a) = r.angles.last_mut() {
                *a += 1e-3;
            }
        }
        _ => {}
    }
    Some(r)
}

fn near_point(rng: &mut Rng, a: Option<&Ellipse>, c: [f64; 3]) -> [f64; 3] {
    let base = a
        .and_then(|el| {
            let d = el.step_interval();
            let t = rng.uniform(d.min() - 30.0, d.max() + 30.0);
            el.evaluate_step_point(t).ok()
        })
        .unwrap_or_else(|| v3(c));
    let s = *rng
        .choose(&[0.0, 1e-9, 1e-6, 1e-3, 0.5, 3.0])
        .unwrap_or(&0.0);
    let d = rng.point3(-1.0, 1.0);
    [base.x + s * d[0], base.y + s * d[1], base.z + s * d[2]]
}

fn gen_frac(rng: &mut Rng) -> [f64; 2] {
    if rng.chance(0.5) {
        return [0.0, 1.0];
    }
    let a = round(rng.unit());
    let b = round(rng.unit());
    if a <= b {
        [a, b]
    } else {
        [b, a]
    }
}

fn gen_param(rng: &mut Rng) -> ParamJ {
    match rng.below(6) {
        0 => ParamJ::Frac(0.0),
        1 => ParamJ::Frac(1.0),
        2 => ParamJ::Frac(
            *rng.choose(&[1e-12, 1.0 - 1e-12, -0.1, 1.1, 0.5])
                .unwrap_or(&0.5),
        ),
        3 => ParamJ::Raw(round(rng.uniform(-2.0, 3.0))),
        _ => ParamJ::Frac(round(rng.unit())),
    }
}

fn gen_op(rng: &mut Rng, a: Option<&Ellipse>, center: [f64; 3], circle: bool) -> OpJ {
    loop {
        let op = match rng.below(22) {
            0 => OpJ::State {
                types: vec![14022, 14021, 14033, 14001, 12060, 14010, 14023, 0],
            },
            1 => OpJ::Copy,
            2 | 3 => OpJ::ToStep { t: gen_param(rng) },
            4 => OpJ::ToPolar { t: gen_param(rng) },
            5 => OpJ::Adjust {
                domain: gen_domain(rng),
            },
            6 | 7 => OpJ::Gps {
                step: rng.chance(0.5),
                ivl: gen_frac(rng),
                sop: *rng.choose(&[0, 0, 3, 3, 4, 5, 6]).unwrap_or(&3),
                p: near_point(rng, a, center),
                tol: *rng.choose(&[1e-6, 1e-3, 0.1, 1.0]).unwrap_or(&1e-6),
                target: rng.chance(0.3).then(|| round(rng.uniform(0.0, 2.0))),
                req: rng.below(2) as i32,
            },
            8 | 9 => {
                // a neighbour arc of the same ellipse (often), or another curve
                let other = match (a, rng.below(5)) {
                    (Some(e), 0..=2) => {
                        let pos = e.position();
                        let d = e.step_interval();
                        let (o, x, y) = (pos.origin(), pos.x_axis(), pos.y_axis());
                        let s = round(rng.uniform(10.0, 120.0));
                        let dom = if rng.chance(0.5) {
                            [d.max(), d.max() + s]
                        } else {
                            [d.min() - s, d.min()]
                        };
                        let dom = [dom[0].clamp(-360.0, 360.0), dom[1].clamp(-360.0, 360.0)];
                        if circle {
                            OtherJ::Circle(CircleJ {
                                c: [o.x, o.y, o.z],
                                x: [x.x, x.y, x.z],
                                y: [y.x, y.y, y.z],
                                domain: dom,
                                r: e.x_radius(),
                                dim: 3,
                                inside_out: rng.chance(0.2),
                                nurb_from: None,
                            })
                        } else {
                            OtherJ::Ellipse(EllipseJ {
                                c: [o.x, o.y, o.z],
                                x: [x.x, x.y, x.z],
                                y: [y.x, y.y, y.z],
                                domain: dom,
                                radii: [e.x_radius(), e.y_radius()],
                                dim: 3,
                                inside_out: rng.chance(0.2),
                                nurb_from: None,
                            })
                        }
                    }
                    (_, 3) => OtherJ::BsplineOfA,
                    _ => OtherJ::Ellipse(gen_ellipse(rng, false)),
                };
                OpJ::Join {
                    end: rng.below(2),
                    other,
                    other_end: rng.below(2),
                    gap: rng
                        .chance(0.2)
                        .then(|| *rng.choose(&[1e-9, 1e-3, 1.0]).unwrap_or(&1e-3)),
                }
            }
            10 => OpJ::Reverse { ivl: gen_frac(rng) },
            11 | 12 => {
                let mut ivl = gen_frac(rng);
                if rng.chance(0.2) {
                    ivl = [round(rng.uniform(-0.2, 0.3)), round(rng.uniform(0.7, 1.2))];
                }
                if ivl[0] == ivl[1] {
                    ivl[1] = (ivl[0] + 0.1).min(1.0);
                }
                OpJ::Trim {
                    ivl,
                    mode: rng.below(3),
                }
            }
            13 => OpJ::Assert {
                given: rng.chance(0.3),
            },
            14 => OpJ::IsCurrent,
            15 => OpJ::PolarAssert,
            16 if circle => {
                let n = a.map_or(Vec3::new(0.0, 0.0, 1.0), |e| e.position().z_axis());
                let normal = match rng.below(4) {
                    0 => [-n.x, -n.y, -n.z],
                    1 => [n.x * 2.0, n.y * 2.0, n.z * 2.0],
                    _ => [n.x, n.y, n.z],
                };
                OpJ::Offset {
                    tol: *rng.choose(&[1e-6, 1e-3]).unwrap_or(&1e-6),
                    normal,
                    dist: round(rng.uniform(-3.0, 3.0)),
                    matched: rng.chance(0.5),
                }
            }
            17 if circle => OpJ::SetRadius {
                r: round(rng.uniform(0.05, 5.0)),
            },
            18 => {
                if rng.chance(0.5) {
                    OpJ::MakeNurb
                } else {
                    OpJ::To2d
                }
            }
            19 => {
                let a0 = round(rng.uniform(-3.0, 3.0));
                OpJ::EditParam {
                    domain: [a0, a0 + round(rng.uniform(0.1, 5.0))],
                    notify: rng.chance(0.7),
                }
            }
            20 => OpJ::Notify {
                post: rng.chance(0.5),
            },
            21 if circle => OpJ::IsArc,
            _ => continue,
        };
        return op;
    }
}

fn gen_case(rng: &mut Rng) -> Option<Inputs> {
    let k = Kernel::default();
    let (bj, circle) = match rng.below(12) {
        0..=3 => (BuildJ::Ellipse(gen_ellipse(rng, true)), false),
        4..=6 => (BuildJ::Circle(gen_circle(rng, true)), true),
        7 => {
            let c = rng.point3(-3.0, 3.0).map(round);
            let r = round(rng.uniform(0.1, 4.0));
            let mut pts = [[0.0; 3]; 3];
            for p in &mut pts {
                let t = rng.uniform(0.0, std::f64::consts::TAU);
                let z = if rng.chance(0.7) {
                    0.0
                } else {
                    round(rng.uniform(-1.0, 1.0))
                };
                *p = [c[0] + r * t.cos(), c[1] + r * t.sin(), c[2] + z];
            }
            if rng.chance(0.05) {
                pts[2] = pts[1]; // coincident points
            }
            (
                BuildJ::Circle3 {
                    points: pts,
                    dim: if rng.chance(0.1) { 2 } else { 3 },
                    closed: rng.chance(0.3),
                },
                true,
            )
        }
        8 => (
            BuildJ::EllipseCanonical {
                frame: gen_frame(rng),
                radii: [round(rng.uniform(0.05, 5.0)), round(rng.uniform(0.05, 5.0))],
                interval: rng.chance(0.5).then(|| gen_domain(rng)),
                inside_out: rng.chance(0.5).then(|| rng.chance(0.3)),
            },
            false,
        ),
        9 => (
            BuildJ::CircleCanonical {
                frame: gen_frame(rng),
                r: if rng.chance(0.1) {
                    *rng.choose(&[0.0, -1.0, 1e-13]).unwrap_or(&0.0)
                } else {
                    round(rng.uniform(0.05, 5.0))
                },
                interval: rng.chance(0.5).then(|| gen_domain(rng)),
                inside_out: rng.chance(0.5).then(|| rng.chance(0.3)),
            },
            true,
        ),
        _ => {
            let circle = rng.chance(0.5);
            let record = gen_record(rng, &k, circle)?;
            (
                BuildJ::ReadDb {
                    circle,
                    dim: if rng.chance(0.15) { 2 } else { 3 },
                    binary: rng.chance(0.5),
                    version: *rng.choose(&[32, 36, 40]).unwrap_or(&36),
                    record,
                },
                circle,
            )
        }
    };
    // the Rust A (when it builds) aims the generated points and joins
    let mut scratch = Vec::new();
    let a = build(&k, &bj, &mut scratch).ok().flatten();
    let center = a.as_ref().map_or([0.0; 3], |e| {
        let o = e.position().origin();
        [o.x, o.y, o.z]
    });
    let n = 1 + rng.below(3) as usize;
    let ops = (0..n)
        .map(|_| gen_op(rng, a.as_ref(), center, circle))
        .collect();
    Some(Inputs { build: bj, ops })
}

// ---- SmCircle::IsNurbCurveCircle cases ----------------------------------------------

/// Generated `IsNurbCurveCircle` cases (inline, fixed seed, independent of `--seeds`
/// so that the main generated set keeps its seeds).
const NURB_CIRCLE_GENERATED: usize = 400;
/// Seed of the generated `IsNurbCurveCircle` cases.
const NURB_CIRCLE_SEED: u64 = 0x1432_0c1c;

fn inc(source: SourceJ, scale: Option<f64>) -> OpJ {
    OpJ::IsNurbCircle { source, scale }
}

fn unit_frame() -> [[f64; 3]; 3] {
    [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
}

/// The unit canonical circle: the host of raw-NURB ops (and the issue's repro).
fn host() -> BuildJ {
    BuildJ::CircleCanonical {
        frame: unit_frame(),
        r: 1.0,
        interval: None,
        inside_out: None,
    }
}

fn circle_j(c: [f64; 3], x: [f64; 3], y: [f64; 3], domain: [f64; 2], r: f64) -> BuildJ {
    BuildJ::Circle(CircleJ {
        c,
        x,
        y,
        domain,
        r,
        dim: 3,
        inside_out: false,
        nurb_from: None,
    })
}

fn ellipse_j(c: [f64; 3], domain: [f64; 2], radii: [f64; 2]) -> BuildJ {
    BuildJ::Ellipse(EllipseJ {
        c,
        x: [1.0, 0.0, 0.0],
        y: [0.0, 1.0, 0.0],
        domain,
        radii,
        dim: 3,
        inside_out: false,
        nurb_from: None,
    })
}

fn add3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale3(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// An orthonormal pair spanning a plane (from two directions, Gram-Schmidt).
fn plane(a: [f64; 3], b: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let u = v3(a).unitize_or_z();
    let bv = v3(b);
    let mut w = Vec3::new(
        bv.x - u.x * u.dot(bv),
        bv.y - u.y * u.dot(bv),
        bv.z - u.z * u.dot(bv),
    );
    if w.length() < 1e-3 {
        w = u.cross(Vec3::new(0.3, -0.5, 0.8));
    }
    let w = w.unitize_or_z();
    ([u.x, u.y, u.z], [w.x, w.y, w.z])
}

/// The rational quadratic NURB of the arc of radius `r` about `c` in the plane of
/// the orthonormal `u`, `v` from angle `a0` to `a1` (radians) in `spans` equal spans
/// (each below half a turn), weights times `wscale`, over `[t0, t0 + len]`.
#[allow(clippy::too_many_arguments)]
fn raw_arc(
    c: [f64; 3],
    (u, v): ([f64; 3], [f64; 3]),
    r: f64,
    [a0, a1]: [f64; 2],
    spans: usize,
    wscale: f64,
    [t0, len]: [f64; 2],
) -> SourceJ {
    let d = (a1 - a0) / spans as f64;
    let pt = |a: f64, rr: f64| add3(c, add3(scale3(u, rr * a.cos()), scale3(v, rr * a.sin())));
    let hw = |p: [f64; 3], w: f64| {
        let w = w * wscale;
        [p[0] * w, p[1] * w, p[2] * w, w]
    };
    let w1 = (0.5 * d).cos();
    let mut cpts = vec![hw(pt(a0, r), 1.0)];
    let mut knots = vec![t0; 3];
    for i in 0..spans {
        let s0 = a0 + d * i as f64;
        cpts.push(hw(pt(s0 + 0.5 * d, r / w1), w1));
        cpts.push(hw(pt(s0 + d, r), 1.0));
        let t = t0 + len * (i + 1) as f64 / spans as f64;
        if i + 1 < spans {
            knots.extend([t, t]);
        }
    }
    knots.extend([t0 + len; 3]);
    SourceJ::Raw {
        degree: 2,
        cpts,
        knots,
    }
}

/// `raw_arc` in the XY plane about the origin over `[0, 1]`.
fn raw_xy(r: f64, angles: [f64; 2], spans: usize, wscale: f64) -> SourceJ {
    raw_arc(
        [0.0; 3],
        ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        r,
        angles,
        spans,
        wscale,
        [0.0, 1.0],
    )
}

/// `src` with control point `i` changed by `f` (homogeneous).
fn tweak(src: SourceJ, i: usize, f: impl Fn([f64; 4]) -> [f64; 4]) -> SourceJ {
    match src {
        SourceJ::Raw {
            degree,
            mut cpts,
            knots,
        } => {
            if let Some(p) = cpts.get_mut(i) {
                *p = f(*p);
            }
            SourceJ::Raw {
                degree,
                cpts,
                knots,
            }
        }
        other => other,
    }
}

/// The weight of control point `i` times `1 + e` (the Euclidean point kept).
fn tweak_weight(src: SourceJ, i: usize, e: f64) -> SourceJ {
    tweak(src, i, |[x, y, z, w]| {
        let s = 1.0 + e;
        [x * s, y * s, z * s, w * s]
    })
}

/// Control point `i` moved by `d` (Euclidean, the weight kept).
fn tweak_point(src: SourceJ, i: usize, d: [f64; 3]) -> SourceJ {
    tweak(src, i, |[x, y, z, w]| {
        [x + d[0] * w, y + d[1] * w, z + d[2] * w, w]
    })
}

fn nurb_circle_fixed() -> Vec<(String, Inputs)> {
    use std::f64::consts::{FRAC_PI_2, PI};
    let mut v: Vec<(String, Inputs)> = Vec::new();
    let mut push = |name: &str, build: BuildJ, ops: Vec<OpJ>| {
        v.push((format!("inc_{name}"), Inputs { build, ops }));
    };
    let both = |scale: Option<f64>| vec![inc(SourceJ::BsplineOfA, scale), inc(SourceJ::A, scale)];
    // #1432: the NURB form of the full unit canonical circle as a plain B-spline
    push("issue_unit_circle", host(), both(None));
    push(
        "unit_circle_scales",
        host(),
        [0.0, 1e-3, 10.0, 1e4, -1.0]
            .iter()
            .map(|&s| inc(SourceJ::BsplineOfA, Some(s)))
            .collect(),
    );
    // arcs of every span count
    let tilted = plane([0.3, -0.4, 0.8], [0.9, 0.1, -0.2]);
    for (i, dom) in [
        [0.0, 90.0],
        [30.0, 200.0],
        [-90.0, 45.0],
        [0.0, 359.9],
        [0.0, 1e-3],
        [0.0, 180.0],
        [-180.0, 180.0],
        [100.0, 370.0],
    ]
    .iter()
    .enumerate()
    {
        push(
            &format!("circle_arc_{i}"),
            circle_j([1.0, 2.0, 3.0], tilted.0, tilted.1, *dom, 2.5),
            both(None),
        );
    }
    // huge and tiny scales
    for (i, (c, r)) in [
        ([1e5, -1e5, 5e4], 1e5),
        ([4e5, 0.0, 0.0], 5e5),
        ([0.0; 3], 1e-5),
        ([1.0, 1.0, 1.0], 1e-7),
        ([1e3, 0.0, 0.0], 1e-6),
        ([0.0; 3], 1e-9),
    ]
    .iter()
    .enumerate()
    {
        push(
            &format!("circle_scale_{i}"),
            circle_j(*c, [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 360.0], *r),
            both(None),
        );
        push(
            &format!("circle_scale_arc_{i}"),
            circle_j(*c, [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [10.0, 75.0], *r),
            both(Some(1e-2)),
        );
    }
    // SmEllipse (not SmCircle): equal radii is an arc, unequal is not
    push(
        "ellipse_equal_radii",
        ellipse_j([1.0, 0.0, 0.0], [0.0, 360.0], [2.0, 2.0]),
        both(None),
    );
    push(
        "ellipse_equal_radii_arc",
        ellipse_j([1.0, 0.0, 0.0], [10.0, 100.0], [2.0, 2.0]),
        both(None),
    );
    push(
        "ellipse_near_equal_radii",
        ellipse_j([0.0; 3], [0.0, 360.0], [2.0, 2.0 * (1.0 + 1e-9)]),
        both(None),
    );
    push(
        "ellipse_unequal_radii",
        ellipse_j([0.0; 3], [0.0, 360.0], [2.0, 1.0]),
        both(None),
    );
    // raw NURBs
    let raw = |name: &str, src: SourceJ, scale: Option<f64>| -> (String, Inputs) {
        (
            format!("inc_raw_{name}"),
            Inputs {
                build: host(),
                ops: vec![inc(src, scale)],
            },
        )
    };
    let quarter = raw_xy(1.0, [0.0, FRAC_PI_2], 1, 1.0);
    v.push(raw("quarter", quarter.clone(), None));
    v.push(raw(
        "quarter_weights_x2",
        raw_xy(1.0, [0.0, FRAC_PI_2], 1, 2.0),
        None,
    ));
    v.push(raw(
        "quarter_weights_x0.01",
        raw_xy(1.0, [0.0, FRAC_PI_2], 1, 0.01),
        None,
    ));
    v.push(raw(
        "semicircle_2span",
        raw_xy(3.0, [0.0, PI], 2, 1.0),
        None,
    ));
    v.push(raw(
        "full_4span",
        raw_xy(3.0, [0.0, 2.0 * PI], 4, 1.0),
        None,
    ));
    v.push(raw("full_3span", raw_xy(0.5, [-PI, PI], 3, 1.0), None));
    v.push(raw(
        "tilted_120",
        raw_arc(
            [5.0, -2.0, 1.0],
            tilted,
            0.75,
            [0.2, 0.2 + 2.0 * PI / 3.0],
            1,
            1.0,
            [2.0, 3.5],
        ),
        None,
    ));
    v.push(raw(
        "wide_170",
        raw_xy(1.0, [0.0, 170f64.to_radians()], 1, 1.0),
        None,
    ));
    v.push(raw("tiny_angle", raw_xy(1.0, [0.0, 1e-6], 1, 1.0), None));
    for (i, e) in [1e-14, 1e-12, 1e-10, 1e-8, 1e-6, 1e-4].iter().enumerate() {
        v.push(raw(
            &format!("weight_perturbed_{i}"),
            tweak(quarter.clone(), 1, |[x, y, z, w]| [x, y, z, w * (1.0 + e)]),
            None,
        ));
        v.push(raw(
            &format!("point_perturbed_{i}"),
            tweak_point(quarter.clone(), 1, [*e, -*e, 0.0]),
            None,
        ));
        v.push(raw(
            &format!("point_out_of_plane_{i}"),
            tweak_point(quarter.clone(), 2, [0.0, 0.0, *e]),
            None,
        ));
        v.push(raw(
            &format!("end_weight_{i}"),
            tweak_weight(quarter.clone(), 0, *e),
            None,
        ));
    }
    // scaled tolerance at the limit: the same near-circle accepted or not by the scale
    for (i, s) in [1e-3, 1.0, 1e3, 1e6].iter().enumerate() {
        v.push(raw(
            &format!("tol_scale_{i}"),
            tweak_point(quarter.clone(), 1, [1e-9, 0.0, 0.0]),
            Some(*s),
        ));
    }
    // not circles
    v.push(raw(
        "hyperbola_weight",
        tweak(quarter.clone(), 1, |[x, y, z, w]| {
            [x / w * 2.0, y / w * 2.0, z / w * 2.0, 2.0]
        }),
        None,
    ));
    v.push(raw(
        "ellipse_weight",
        tweak(quarter.clone(), 1, |[x, y, z, w]| {
            [x / w * 0.3, y / w * 0.3, z / w * 0.3, 0.3]
        }),
        None,
    ));
    v.push(raw(
        "unit_weights",
        tweak(quarter.clone(), 1, |[x, y, z, w]| {
            [x / w, y / w, z / w, 1.0]
        }),
        None,
    ));
    v.push(raw(
        "non_rational",
        SourceJ::Raw {
            degree: 2,
            cpts: vec![
                [1.0, 0.0, 0.0, NOW],
                [1.0, 1.0, 0.0, NOW],
                [0.0, 1.0, 0.0, NOW],
            ],
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        },
        None,
    ));
    v.push(raw(
        "rational_cubic",
        SourceJ::Raw {
            degree: 3,
            cpts: vec![
                [1.0, 0.0, 0.0, 1.0],
                [0.5, 0.25, 0.0, 0.5],
                [0.25, 0.5, 0.0, 0.5],
                [0.0, 1.0, 0.0, 1.0],
            ],
            knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        },
        None,
    ));
    v.push(raw(
        "rational_linear",
        SourceJ::Raw {
            degree: 1,
            cpts: vec![[1.0, 0.0, 0.0, 1.0], [0.0, 0.5, 0.0, 0.5]],
            knots: vec![0.0, 0.0, 1.0, 1.0],
        },
        None,
    ));
    v.push(raw(
        "degenerate_point",
        SourceJ::Raw {
            degree: 2,
            cpts: vec![
                [1.0, 2.0, 3.0, 1.0],
                [0.5, 1.0, 1.5, 0.5],
                [1.0, 2.0, 3.0, 1.0],
            ],
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        },
        None,
    ));
    v.push(raw(
        "degenerate_tiny_radius",
        raw_xy(1e-12, [0.0, FRAC_PI_2], 1, 1.0),
        None,
    ));
    v.push(raw(
        "tiny_radius_1e-8",
        raw_xy(1e-8, [0.0, FRAC_PI_2], 1, 1.0),
        None,
    ));
    v.push(raw(
        "huge_radius",
        raw_xy(1e5, [0.0, FRAC_PI_2], 1, 1.0),
        None,
    ));
    v.push(raw(
        "huge_offset_small_radius",
        raw_arc(
            [5e5, 5e5, 0.0],
            ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            1e-2,
            [0.0, 1.0],
            1,
            1.0,
            [0.0, 1.0],
        ),
        None,
    ));
    v.push(raw(
        "collinear",
        SourceJ::Raw {
            degree: 2,
            cpts: vec![
                [0.0, 0.0, 0.0, 1.0],
                [0.5, 0.0, 0.0, 0.5],
                [2.0, 0.0, 0.0, 1.0],
            ],
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        },
        None,
    ));
    v.push(raw(
        "s_curve_2span",
        tweak_point(raw_xy(1.0, [0.0, PI], 2, 1.0), 3, [0.0, 0.0, 0.5]),
        None,
    ));
    v
}

/// A generated `IsNurbCurveCircle` case.
fn gen_nurb_circle(rng: &mut Rng) -> Inputs {
    use std::f64::consts::PI;
    let scale = if rng.chance(0.25) {
        Some(*rng.choose(&[0.0, 1e-3, 0.5, 2.0, 1e3, 1e6]).unwrap_or(&1.0))
    } else {
        None
    };
    let r = 10f64.powf(rng.uniform(-6.0, 5.0));
    let c = scale3(rng.point3(-1.0, 1.0), 10f64.powf(rng.uniform(-3.0, 5.0)));
    match rng.below(4) {
        0 => {
            let f = gen_frame(rng);
            Inputs {
                build: circle_j(c, f[1], f[2], gen_domain(rng), r),
                ops: vec![inc(SourceJ::BsplineOfA, scale), inc(SourceJ::A, scale)],
            }
        }
        1 => {
            let rr = 2.0
                * (1.0
                    + if rng.chance(0.5) {
                        10f64.powf(rng.uniform(-14.0, -2.0))
                    } else {
                        0.0
                    });
            Inputs {
                build: ellipse_j(c, gen_domain(rng), [2.0, rr]),
                ops: vec![inc(SourceJ::A, scale), inc(SourceJ::BsplineOfA, scale)],
            }
        }
        _ => {
            let total = rng.uniform(1e-3, 2.0 * PI);
            let min_spans = (total / (0.9 * PI)).ceil() as usize;
            let spans = min_spans.max(1) + rng.below(2) as usize;
            let a0 = rng.uniform(-PI, PI);
            let pl = plane(gen_dir(rng), gen_dir(rng));
            let wscale = 10f64.powf(rng.uniform(-1.0, 1.0));
            let t0 = rng.uniform(-10.0, 10.0);
            let len = 10f64.powf(rng.uniform(-2.0, 2.0));
            let mut src = raw_arc(c, pl, r, [a0, a0 + total], spans, wscale, [t0, len]);
            let n = 2 * spans + 1;
            let e = 10f64.powf(rng.uniform(-15.0, -2.0));
            let i = rng.below(n as u64) as usize;
            src = match rng.below(4) {
                0 => tweak_weight(src, i, e),
                1 => {
                    let d = scale3(gen_dir(rng), e * r);
                    tweak_point(src, i, d)
                }
                _ => src,
            };
            Inputs {
                build: host(),
                ops: vec![inc(src, scale)],
            }
        }
    }
}

/// The inline `IsNurbCurveCircle` cases: fixed, then generated (within the input contract).
fn nurb_circle_cases(unit: &str) -> Vec<Case> {
    let mut all = nurb_circle_fixed();
    let mut rng = Rng::new(NURB_CIRCLE_SEED);
    let mut made = 0;
    for _ in 0..8 * NURB_CIRCLE_GENERATED {
        if made == NURB_CIRCLE_GENERATED {
            break;
        }
        let i = gen_nurb_circle(&mut rng);
        if keeps_contract(&i) {
            all.push((format!("incgen{made:04}"), i));
            made += 1;
        }
    }
    all.into_iter()
        .filter_map(|(name, i)| {
            Some(Case {
                unit: unit.to_string(),
                name,
                source: CaseSource::Inline,
                inputs: serde_json::to_value(i).ok()?,
            })
        })
        .collect()
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "curve.ellipse_p1"
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
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        let mut v = default_cases(self, &ctx)?;
        v.extend(nurb_circle_cases(self.id()));
        Ok(v)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `IsNurbCurveCircle` flag of the first op of `case` (Rust side).
    fn first_flag(case: &Case) -> f64 {
        let i: Inputs = case.parse().unwrap();
        let out = rust(&i).unwrap();
        let rec = out.values.get("rec").unwrap();
        // construction status, op code 21, op status 0, flag
        assert_eq!(&rec[..3], &[0.0, 21.0, 0.0], "{}", case.name);
        rec[3]
    }

    /// The inline `IsNurbCurveCircle` cases reach both outcomes (the circle is built
    /// for the issue's repro and the clean raw arcs; not for non-circles).
    #[test]
    fn nurb_circle_cases_reach_both_outcomes() {
        let cases = nurb_circle_cases("curve.ellipse_p1");
        let flag = |name: &str| first_flag(cases.iter().find(|c| c.name == name).unwrap());
        for yes in [
            "inc_issue_unit_circle",
            "inc_raw_quarter",
            "inc_raw_quarter_weights_x2",
            "inc_raw_full_4span",
            "inc_raw_tilted_120",
            "inc_circle_arc_1",
            "inc_ellipse_equal_radii",
        ] {
            assert_eq!(flag(yes), 1.0, "{yes}");
        }
        for no in [
            "inc_raw_non_rational",
            "inc_raw_rational_cubic",
            "inc_raw_hyperbola_weight",
            "inc_raw_degenerate_point",
            "inc_raw_collinear",
            "inc_ellipse_unequal_radii",
        ] {
            assert_eq!(flag(no), 0.0, "{no}");
        }
        let gen: Vec<f64> = cases
            .iter()
            .filter(|c| c.name.starts_with("incgen"))
            .map(first_flag)
            .collect();
        let yes = gen.iter().filter(|&&f| f == 1.0).count();
        assert!(
            yes > gen.len() / 5 && gen.len() - yes > gen.len() / 10,
            "{yes} of {}",
            gen.len()
        );
    }
}
