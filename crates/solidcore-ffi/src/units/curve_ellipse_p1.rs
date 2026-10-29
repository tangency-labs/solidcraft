//! FFI wrappers for unit `curve.ellipse_p1`.
//!
//! Shim: `harness/cpp/shim/curve_ellipse_p1.cpp` (`sc_curve_ellipse_p1_run`).
//! Upstream: `source/SMLib/src/SmEllipse.cpp`, `source/SMLib/src/SmCircle.cpp`,
//! `source/SMLib/inc/SmCircle.h`.
//!
//! A program builds curve A ([`Build`]: the `SmEllipse` or `SmCircle` constructors,
//! `SmEllipse::CreateCanonical`, `SmCircle::CreateCanonical` or `SmCurve::ReadFromDB` of
//! an ellipse or circle record) and runs [`Op`]s on it. The record is the construction
//! status (`0` = `SM_SUCCESS`, else the `SmStatus`: `smgu_CircleFrom3Points`' for the
//! 3-point circle, the canonical creators', the reader's); when A was built and has a
//! NURB, per op `op status [outputs]` (outputs only for status `0`; `-2`: the other curve
//! of [`Op::Join`] has no NURB and is not called); then, when built, the state of A:
//!
//! | [`Op`]          | upstream                                                  | outputs |
//! |-----------------|-----------------------------------------------------------|---------|
//! | `State`         | queries                                                   | `type`, type string, class string (chars then `-1`), `GetRadius` (circles, else `0`), one `IsKindOf` flag per type |
//! | `Copy`          | `Copy`                                                    | `operator==` flag, state of the copy |
//! | `ToStep`        | `ConvertTFromNURBSToSTEP(t)`                              | `t` |
//! | `ToPolar`       | `m_vPolarConverter.ConvertToPolarParameter(t, ..)`        | `t exact` |
//! | `Adjust`        | `AdjustSTEPInterval(SmExtent1d(d0, d1))`                  | |
//! | `GpsStep`       | `GlobalPointSolveSTEP(GetSTEPInterval(), op, P, tol, target?, NULL, req)` | solutions |
//! | `Gps`           | `GlobalPointSolve(ivl, op, P, tol, target?, NULL, req)`   | solutions |
//! | `Join`          | `JoinWith(end, other, other_end, gap?)`                   | |
//! | `Reverse`       | `ReverseParameterization(ivl, ..)`                        | new interval |
//! | `Trim`          | `Trim(ivl, FALSE / TRUE / UNSURE, FALSE)`                 | trimmed interval |
//! | `Assert`        | `AssertValid(list, SM_LEVEL_0 or SM_LEVEL_GIVEN, SM_NO_WALK)` | flag, reports |
//! | `IsCurrent`     | `m_vPolarConverter.IsCurrent(list)`                       | flag, reports |
//! | `PolarAssert`   | `m_vPolarConverter.AssertValid(list)`                     | flag, reports |
//! | `Offset`        | `CreateSimpleOffset(ctx, tol, N, d, .., gap, match)`      | `gap kind` (`1` ellipse: its state; `0` B-spline: `type dim` NURB; `-1` none) |
//! | `SetRadius`     | `SmCircle::SetRadius(r)`, `MakeNurb()` (circles only)     | |
//! | `MakeNurb`      | `MakeNurb()`                                              | |
//! | `To2d`          | `ConvertTo2D()`                                           | |
//! | `EditParam`     | `EditParameterization(SmExtent1d(d0, d1), notify)`        | |
//! | `Notify`        | `Notify(SM_NO_PRE_EDIT / SM_NO_POST_EDIT, this, NULL, NULL)` | |
//! | `IsArc`         | `SmCircle::IsArc(5, 1e-6, ..)` (circles only)             | `flag O X Y r start end` |
//! | `IsNurbCircle`  | `SmCircle::IsNurbCurveCircle(ctx, curve, circle, scale?)` on [`NurbSource`] | `flag` (`TRUE`: `IsKindOf(SmCircle_TYPE)`, `GetRadius`, state of the circle) |
//!
//! Intervals are normalised parameters `[f0, f1]` of A's natural interval
//! (extrapolated outside `[0, 1]`); a NURB parameter is raw or normalised. Solutions are
//! `count`, then per solution `type num_vars value start[0]`. Reports are `count`, then
//! per report `reporting_type list_index test_index assert_type owner reporter
//! owner_type tol value` (owner, reporter: `0` A, `1` its polar converter, `-1` none). A
//! state is `type O[3] X[3] Y[3] dmin dmax rx ry inside_out dim form has_nurb [n p m
//! cpts(x y z w) knots] possible has_curve nk knots na angles ne exact`.

use std::fmt::Write as _;

use crate::validate::{bad, check_knots, NOW};
use crate::{call_with_buffer, FfiError};

/// Largest magnitude of coordinates, lengths, parameters and tolerances.
pub const MAX_COORD: f64 = 1e6;
/// Largest magnitude of a STEP domain end (degrees).
pub const MAX_ANGLE: f64 = 720.0;
/// Most ops per program.
pub const MAX_OPS: usize = 64;
/// Most types of [`Op::State`].
pub const MAX_TYPES: usize = 64;
/// Most control points of a raw NURB of [`NurbSource::Raw`].
pub const MAX_RAW_POINTS: usize = 65;
/// Largest degree of a raw NURB of [`NurbSource::Raw`].
pub const MAX_RAW_DEGREE: usize = 8;
/// Weight range of a rational raw NURB of [`NurbSource::Raw`].
pub const RAW_WEIGHTS: [f64; 2] = [1e-3, 1e3];
/// Largest last control point index and polar table length of a database record.
pub const MAX_DB_INDEX: i64 = 64;

mod sys {
    extern "C" {
        pub fn sc_curve_ellipse_p1_run(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Constructor arguments of an ellipse
/// (`SmEllipse(C, X, Y, SmExtent1d(d0, d1), rx, ry, dim, &ctx, pOptNurb, bInsideOut)`).
#[derive(Debug, Clone, PartialEq)]
pub struct EllipseIn {
    /// Center.
    pub center: [f64; 3],
    /// X axis (nonzero).
    pub x_axis: [f64; 3],
    /// Y axis (nonzero).
    pub y_axis: [f64; 3],
    /// STEP domain ends in degrees; its length is in `[1e-4, 360]`.
    pub domain: [f64; 2],
    /// Radii at the X and Y axes (positive).
    pub radii: [f64; 2],
    /// Image dimension (2 or 3).
    pub dim: u64,
    /// `bInsideOut`.
    pub inside_out: bool,
    /// An ellipse whose NURB is passed as `pOptNurb` (itself without one).
    pub nurb_from: Option<Box<EllipseIn>>,
}

/// Constructor arguments of a circle
/// (`SmCircle(C, X, Y, SmExtent1d(d0, d1), r, dim, &ctx, pOptNurb, bInsideOut)`).
#[derive(Debug, Clone, PartialEq)]
pub struct CircleIn {
    /// Center.
    pub center: [f64; 3],
    /// X axis (nonzero).
    pub x_axis: [f64; 3],
    /// Y axis (nonzero).
    pub y_axis: [f64; 3],
    /// STEP domain ends in degrees; its length is in `[1e-4, 360]`.
    pub domain: [f64; 2],
    /// Radius (positive).
    pub radius: f64,
    /// Image dimension (2 or 3).
    pub dim: u64,
    /// `bInsideOut`.
    pub inside_out: bool,
    /// An ellipse whose NURB is passed as `pOptNurb` (itself without one).
    pub nurb_from: Option<Box<EllipseIn>>,
}

/// An ellipse or circle database record as `WriteToDB` writes it.
#[derive(Debug, Clone, PartialEq)]
pub struct DbRecord {
    /// Ellipse data: `O[3] X[3] Y[3] dmin dmax rx ry`.
    pub head: [f64; 13],
    /// `m_bInsideOut` (0 or 1).
    pub inside_out: i32,
    /// `m_bPolarConversionPossible` (0 or 1).
    pub possible: i32,
    /// Natural knots of the polar table.
    pub knots: Vec<f64>,
    /// Angles of the polar table.
    pub angles: Vec<f64>,
    /// Exactness flags of the polar table (0 or 1).
    pub exact: Vec<i32>,
    /// B-spline base: last control point index.
    pub n: i64,
    /// Degree.
    pub p: i16,
    /// Rational flag.
    pub rat: i16,
    /// Stored dimension (2 or 3).
    pub dim: i16,
    /// Knot type code (written for versions above 32).
    pub kt: u64,
    /// Form code (written for versions above 32).
    pub form: u64,
    /// Control point values (2 to 4 each) then the `n + p + 2` knots.
    pub vals: Vec<f64>,
}

/// How curve A is made.
#[derive(Debug, Clone, PartialEq)]
pub enum Build {
    /// The ellipse constructor.
    Ellipse(EllipseIn),
    /// The circle constructor.
    Circle(CircleIn),
    /// `SmCircle(P0, P1, P2, dim, bClosedCircle, &ctx)` (built only when
    /// `smgu_CircleFrom3Points` succeeds).
    Circle3 {
        /// Start, mid and end points.
        points: [[f64; 3]; 3],
        /// Dimension (2 or 3).
        dim: u64,
        /// `bClosedCircle`.
        closed: bool,
    },
    /// `SmEllipse::CreateCanonical(ctx, placement, r1, r2, .., interval?, inside_out?)`.
    EllipseCanonical {
        /// Placement `SetCanonical(O, X, Y)`.
        frame: [[f64; 3]; 3],
        /// Radii (positive).
        radii: [f64; 2],
        /// Optional interval (length in `[1e-4, 360]`).
        interval: Option<[f64; 2]>,
        /// Optional inside-out flag.
        inside_out: Option<bool>,
    },
    /// `SmCircle::CreateCanonical(ctx, placement, r, .., interval?, inside_out?)`.
    CircleCanonical {
        /// Placement `SetCanonical(O, X, Y)`.
        frame: [[f64; 3]; 3],
        /// Radius (any finite value; not above `SM_EFF_ZERO` fails).
        radius: f64,
        /// Optional interval (length in `[1e-4, 360]`).
        interval: Option<[f64; 2]>,
        /// Optional inside-out flag.
        inside_out: Option<bool>,
    },
    /// `SmCurve::ReadFromDB(SmEllipse_TYPE or SmCircle_TYPE, db, dim, ctx, .., version)`.
    ReadDb {
        /// Read a circle.
        circle: bool,
        /// Requested dimension (2 or 3).
        dim: u64,
        /// Binary (else ASCII) stream.
        binary: bool,
        /// Database version.
        version: u64,
        /// The record.
        record: DbRecord,
    },
}

/// A NURB parameter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Param {
    /// The value itself.
    Raw(f64),
    /// A normalised parameter of the natural interval.
    Frac(f64),
}

/// The other curve of [`Op::Join`].
#[derive(Debug, Clone, PartialEq)]
pub enum JoinOther {
    /// An ellipse.
    Ellipse(EllipseIn),
    /// A circle.
    Circle(CircleIn),
    /// `SmBSplineCurve(3, A's NURB)` (not an ellipse).
    BSplineOfA,
}

/// The curve tested by [`Op::IsNurbCircle`].
#[derive(Debug, Clone, PartialEq)]
pub enum NurbSource {
    /// A itself (an `SmCircle` is copied; an `SmEllipse` dispatches to its overrides).
    A,
    /// `SmBSplineCurve(3, A's NURB)`.
    BSplineOfA,
    /// `SmBSplineCurve(gw_CURVE, 3, FALSE)` of a clamped 3D NURB.
    Raw {
        /// Degree (`1..=MAX_RAW_DEGREE`).
        degree: usize,
        /// Homogeneous control points `x y z w` (`w` positive in [`RAW_WEIGHTS`] for
        /// every point, or `NOW` for every point: non-rational).
        cpts: Vec<[f64; 4]>,
        /// Clamped knots (`cpts.len() + degree + 1`).
        knots: Vec<f64>,
    },
}

/// An operation on curve A (see the module docs).
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Queries; `IsKindOf` for each type.
    State {
        /// Types to test.
        types: Vec<i64>,
    },
    /// `Copy`.
    Copy,
    /// `ConvertTFromNURBSToSTEP`.
    ToStep {
        /// NURB parameter.
        t: Param,
    },
    /// `ConvertToPolarParameter`.
    ToPolar {
        /// NURB parameter.
        t: Param,
    },
    /// `AdjustSTEPInterval`.
    Adjust {
        /// New domain ends (length in `[1e-4, 360]`).
        domain: [f64; 2],
    },
    /// `GlobalPointSolveSTEP` (`ivl` unused) or `GlobalPointSolve` over `ivl`.
    Gps {
        /// `true`: `GlobalPointSolveSTEP`.
        step: bool,
        /// Normalised interval (global solve).
        ivl: [f64; 2],
        /// `SmSolverOperationType`: 0, 3, 4, 5 or 6.
        op: i32,
        /// Point.
        p: [f64; 3],
        /// Distance tolerance.
        tol: f64,
        /// Target distance (required for 6).
        target: Option<f64>,
        /// `SmSolutionRequestedType` (0 or 1).
        req: i32,
    },
    /// `JoinWith`.
    Join {
        /// End of A (0 or 1).
        end: u64,
        /// The other curve.
        other: JoinOther,
        /// End of the other curve (0 or 1).
        other_end: u64,
        /// Gap tolerance.
        gap: Option<f64>,
    },
    /// `ReverseParameterization`.
    Reverse {
        /// Normalised old interval.
        ivl: [f64; 2],
    },
    /// `Trim`.
    Trim {
        /// Normalised trim interval (may extend past `[0, 1]`).
        ivl: [f64; 2],
        /// `bNotify`: 0 `FALSE`, 1 `TRUE`, 2 `UNSURE`.
        mode: u64,
    },
    /// `AssertValid`.
    Assert {
        /// `SM_LEVEL_GIVEN`.
        given: bool,
    },
    /// `SmPolarConversion::IsCurrent`.
    IsCurrent,
    /// `SmPolarConversion::AssertValid`.
    PolarAssert,
    /// `CreateSimpleOffset`.
    Offset {
        /// Approximation tolerance (positive).
        tol: f64,
        /// Offset plane normal (nonzero).
        normal: [f64; 3],
        /// Offset distance.
        dist: f64,
        /// `bOptMatchParameterization`.
        matched: bool,
    },
    /// `SetRadius` and `MakeNurb` (circles only).
    SetRadius {
        /// New radius (positive).
        r: f64,
    },
    /// `MakeNurb`.
    MakeNurb,
    /// `ConvertTo2D`.
    To2d,
    /// `EditParameterization`.
    EditParam {
        /// New natural interval ends.
        domain: [f64; 2],
        /// `bNotify`.
        notify: bool,
    },
    /// `Notify(SM_NO_PRE_EDIT)` (`post == false`) or `Notify(SM_NO_POST_EDIT)`.
    Notify {
        /// Post-edit notification.
        post: bool,
    },
    /// `SmCircle::IsArc` (circles only).
    IsArc,
    /// `SmCircle::IsNurbCurveCircle` (static) of a curve.
    IsNurbCircle {
        /// The tested curve.
        source: NurbSource,
        /// `dToleranceScale` (`None`: the default `1.0`).
        scale: Option<f64>,
    },
}

fn finite(x: f64) -> Result<f64, FfiError> {
    if x.is_finite() && x.abs() <= MAX_COORD {
        Ok(x)
    } else {
        Err(bad("non-finite or huge value"))
    }
}

fn nonzero(v: [f64; 3]) -> Result<(), FfiError> {
    if v.iter().map(|x| x * x).sum::<f64>() < 1e-12 {
        return Err(bad("zero vector"));
    }
    Ok(())
}

fn domain_ok(d: [f64; 2]) -> Result<(), FfiError> {
    let [d0, d1] = d;
    if d0.is_nan() || d1.is_nan() || d0.abs() > MAX_ANGLE || d1.abs() > MAX_ANGLE {
        return Err(bad("domain end beyond 720 degrees"));
    }
    let len = (d1 - d0).abs();
    if !(1e-4..=360.0).contains(&len) {
        return Err(bad("domain length outside [1e-4, 360]"));
    }
    Ok(())
}

/// Values per control point of a record.
fn per_point(r: &DbRecord) -> usize {
    2 + usize::from(r.dim != 2) + usize::from(r.rat != 0)
}

/// Checks a record: `1 <= p <= n <= MAX_DB_INDEX`, flags in range, the exact number
/// of finite values, positive weights, polar tables of at most `MAX_DB_INDEX` finite
/// values with the angle table as long as the knot table (upstream reads them in step)
/// and, for a usable converter, a non-empty angle table with an exactness flag per arc.
fn check_record(r: &DbRecord) -> Result<(), FfiError> {
    for &x in &r.head {
        finite(x)?;
    }
    if !(0..=1).contains(&r.inside_out) || !(0..=1).contains(&r.possible) {
        return Err(bad("record flags"));
    }
    let max = MAX_DB_INDEX as usize;
    if r.knots.len() > max || r.angles.len() != r.knots.len() || r.exact.len() > max {
        return Err(bad("polar table sizes"));
    }
    for &x in r.knots.iter().chain(&r.angles) {
        finite(x)?;
    }
    if r.exact.iter().any(|&x| !(0..=1).contains(&x)) {
        return Err(bad("polar exactness flags"));
    }
    // `SmPolarConversion::ConvertToNURBSParameter` reads `m_vAngles[0]` and, per arc
    // interval `ii`, `m_vExactConversion[ii-1]` without bounds checks: a usable
    // converter read with no angles or fewer than `angles - 1` flags dereferences past
    // its arrays (SIGSEGV, `SmEllipse.cpp:555`).
    if r.possible != 0 && (r.angles.is_empty() || r.exact.len() + 1 < r.angles.len()) {
        return Err(bad("polar exactness table shorter than the angle table"));
    }
    if !(1..=MAX_DB_INDEX).contains(&r.n) || r.p < 1 || i64::from(r.p) > r.n {
        return Err(bad("record sizes"));
    }
    if !(0..=1).contains(&r.rat) || !(2..=3).contains(&r.dim) || r.kt > 4 || r.form > 6 {
        return Err(bad("record flags"));
    }
    let per = per_point(r);
    let n_pts = usize::try_from(r.n + 1).map_err(|_| bad("record sizes"))?;
    let n_knots = usize::try_from(r.n + i64::from(r.p) + 2).map_err(|_| bad("record sizes"))?;
    if r.vals.len() != n_pts * per + n_knots {
        return Err(bad("record value count"));
    }
    for &x in &r.vals {
        finite(x)?;
    }
    if r.rat != 0 {
        for c in r.vals.chunks(per).take(n_pts) {
            if c.last().is_none_or(|&w| w <= 0.0) {
                return Err(bad("record weight"));
            }
        }
    }
    // NLib assumes a non-decreasing knot vector (span search); a record with a
    // decreasing knot is not a B-spline (fuzz: `GlobalPointSolve` fails upstream).
    if r.vals
        .get(n_pts * per..)
        .is_some_and(|k| k.windows(2).any(|w| w.first() > w.last()))
    {
        return Err(bad("record knots not non-decreasing"));
    }
    Ok(())
}

/// The stream bytes of `r` as `SmEllipse::WriteToDB` writes them (ASCII: one group per
/// line with upstream's labels, values as `{:.17e}`, the polar arrays broken after the
/// elements with index 5, 10, ...; binary: native-endian values).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a record [`run`] would reject.
pub fn db_bytes(binary: bool, version: u64, r: &DbRecord) -> Result<Vec<u8>, FfiError> {
    check_record(r)?;
    let per = per_point(r);
    let n_pts = usize::try_from(r.n + 1).map_err(|_| bad("record sizes"))?;
    let (pts, knots) = r.vals.split_at(n_pts * per);
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
        Ok(b)
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
        Ok(t.into_bytes())
    }
}

struct Enc(Vec<f64>);

impl Enc {
    fn num(&mut self, x: f64) -> Result<(), FfiError> {
        self.0.push(finite(x)?);
        Ok(())
    }
    fn tol(&mut self, x: f64) -> Result<(), FfiError> {
        if x < 0.0 {
            return Err(bad("negative tolerance"));
        }
        self.num(x)
    }
    /// `n p m cpts knots` of a clamped 3D NURB.
    fn raw_nurb(
        &mut self,
        degree: usize,
        cpts: &[[f64; 4]],
        knots: &[f64],
    ) -> Result<(), FfiError> {
        if cpts.len() < 2 || cpts.len() > MAX_RAW_POINTS {
            return Err(bad("raw NURB: control point count"));
        }
        let n = cpts.len() - 1;
        if degree == 0 || degree > MAX_RAW_DEGREE || degree > n {
            return Err(bad("raw NURB: need 1 <= p <= min(n, MAX_RAW_DEGREE)"));
        }
        if knots.len() != n + degree + 2 {
            return Err(bad("raw NURB: need m = n + p + 1"));
        }
        if knots.iter().any(|k| !k.is_finite() || k.abs() > MAX_COORD) {
            return Err(bad("raw NURB: non-finite or huge knot"));
        }
        check_knots(knots, degree, "raw NURB")?;
        let rational = cpts.first().is_some_and(|p| p[3] != NOW);
        for &[x, y, z, w] in cpts {
            let w_e = if rational {
                if !(RAW_WEIGHTS[0]..=RAW_WEIGHTS[1]).contains(&w) {
                    return Err(bad("raw NURB: weight outside RAW_WEIGHTS or mixed rationality"));
                }
                w
            } else {
                if w != NOW {
                    return Err(bad("raw NURB: mixed rationality"));
                }
                1.0
            };
            for c in [x, y, z] {
                if !c.is_finite() || (c / w_e).abs() > MAX_COORD {
                    return Err(bad("raw NURB: non-finite or huge point"));
                }
            }
        }
        self.int(n);
        self.int(degree);
        self.int(n + degree + 1);
        for p in cpts {
            self.0.extend_from_slice(p);
        }
        self.0.extend_from_slice(knots);
        Ok(())
    }
    fn int(&mut self, x: usize) {
        self.0.push(x as f64);
    }
    fn flag(&mut self, b: bool) {
        self.0.push(if b { 1.0 } else { 0.0 });
    }
    fn vec(&mut self, v: [f64; 3]) -> Result<(), FfiError> {
        v.iter().try_for_each(|&x| self.num(x))
    }
    fn frac(&mut self, f: [f64; 2]) -> Result<(), FfiError> {
        let [a, b] = f;
        if !(-2.0..=3.0).contains(&a) || !(-2.0..=3.0).contains(&b) {
            return Err(bad("normalised interval outside [-2, 3]"));
        }
        self.num(a)?;
        self.num(b)
    }
    fn dim(&mut self, d: u64) -> Result<(), FfiError> {
        if d != 2 && d != 3 {
            return Err(bad("dimension not 2 or 3"));
        }
        self.int(d as usize);
        Ok(())
    }
    fn ellipse(&mut self, e: &EllipseIn, with_nurb: bool) -> Result<(), FfiError> {
        nonzero(e.x_axis)?;
        nonzero(e.y_axis)?;
        domain_ok(e.domain)?;
        if e.radii.iter().any(|r| r.is_nan() || *r <= 0.0) {
            return Err(bad("radius not positive"));
        }
        self.vec(e.center)?;
        self.vec(e.x_axis)?;
        self.vec(e.y_axis)?;
        self.num(e.domain[0])?;
        self.num(e.domain[1])?;
        self.num(e.radii[0])?;
        self.num(e.radii[1])?;
        self.dim(e.dim)?;
        self.flag(e.inside_out);
        if with_nurb {
            self.flag(e.nurb_from.is_some());
            if let Some(src) = &e.nurb_from {
                self.ellipse(src, false)?;
            }
        } else if e.nurb_from.is_some() {
            return Err(bad("nested pOptNurb"));
        }
        Ok(())
    }
    fn circle(&mut self, c: &CircleIn) -> Result<(), FfiError> {
        nonzero(c.x_axis)?;
        nonzero(c.y_axis)?;
        domain_ok(c.domain)?;
        if c.radius.is_nan() || c.radius <= 0.0 {
            return Err(bad("radius not positive"));
        }
        self.vec(c.center)?;
        self.vec(c.x_axis)?;
        self.vec(c.y_axis)?;
        self.num(c.domain[0])?;
        self.num(c.domain[1])?;
        self.num(c.radius)?;
        self.dim(c.dim)?;
        self.flag(c.inside_out);
        self.flag(c.nurb_from.is_some());
        if let Some(src) = &c.nurb_from {
            self.ellipse(src, false)?;
        }
        Ok(())
    }
    fn param(&mut self, t: Param) -> Result<(), FfiError> {
        match t {
            Param::Raw(v) => {
                self.flag(true);
                self.num(v)
            }
            Param::Frac(v) => {
                if !(-2.0..=3.0).contains(&v) {
                    return Err(bad("normalised parameter outside [-2, 3]"));
                }
                self.flag(false);
                self.num(v)
            }
        }
    }
    fn build(&mut self, b: &Build) -> Result<(), FfiError> {
        match b {
            Build::Ellipse(e) => {
                self.int(0);
                self.ellipse(e, true)
            }
            Build::Circle(c) => {
                self.int(1);
                self.circle(c)
            }
            Build::Circle3 {
                points,
                dim,
                closed,
            } => {
                self.int(2);
                points.iter().try_for_each(|p| self.vec(*p))?;
                self.dim(*dim)?;
                self.flag(*closed);
                Ok(())
            }
            Build::EllipseCanonical {
                frame,
                radii,
                interval,
                inside_out,
            } => {
                if radii.iter().any(|r| r.is_nan() || *r <= 0.0) {
                    return Err(bad("radius not positive"));
                }
                self.int(3);
                self.canonical(frame, radii, *interval, *inside_out)
            }
            Build::CircleCanonical {
                frame,
                radius,
                interval,
                inside_out,
            } => {
                self.int(4);
                self.canonical(frame, &[*radius], *interval, *inside_out)
            }
            Build::ReadDb {
                circle,
                dim,
                binary,
                version,
                record,
            } => {
                if *version > 1000 {
                    return Err(bad("version"));
                }
                let bytes = db_bytes(*binary, *version, record)?;
                if bytes.len() > 1 << 20 {
                    return Err(bad("record too long"));
                }
                self.int(5);
                self.flag(*circle);
                self.dim(*dim)?;
                self.flag(*binary);
                self.int(*version as usize);
                self.int(bytes.len());
                for b in bytes {
                    self.int(usize::from(b));
                }
                Ok(())
            }
        }
    }
    fn canonical(
        &mut self,
        frame: &[[f64; 3]; 3],
        radii: &[f64],
        interval: Option<[f64; 2]>,
        inside_out: Option<bool>,
    ) -> Result<(), FfiError> {
        nonzero(frame[1])?;
        nonzero(frame[2])?;
        frame.iter().try_for_each(|v| self.vec(*v))?;
        radii.iter().try_for_each(|&r| self.num(r))?;
        if let Some(d) = interval {
            domain_ok(d)?;
        }
        self.flag(interval.is_some());
        let d = interval.unwrap_or([0.0, 360.0]);
        self.num(d[0])?;
        self.num(d[1])?;
        self.flag(inside_out.is_some());
        self.flag(inside_out.unwrap_or(false));
        Ok(())
    }
    fn op(&mut self, op: &Op) -> Result<(), FfiError> {
        match op {
            Op::State { types } => {
                if types.len() > MAX_TYPES {
                    return Err(bad("too many types"));
                }
                self.int(1);
                self.int(types.len());
                for &t in types {
                    if !(0..=100_000).contains(&t) {
                        return Err(bad("type out of range"));
                    }
                    self.0.push(t as f64);
                }
            }
            Op::Copy => self.int(2),
            Op::ToStep { t } => {
                self.int(3);
                self.param(*t)?;
            }
            Op::ToPolar { t } => {
                self.int(4);
                self.param(*t)?;
            }
            Op::Adjust { domain } => {
                domain_ok(*domain)?;
                self.int(5);
                self.num(domain[0])?;
                self.num(domain[1])?;
            }
            Op::Gps {
                step,
                ivl,
                op,
                p,
                tol,
                target,
                req,
            } => {
                if ![0, 3, 4, 5, 6].contains(op) || !(0..=1).contains(req) {
                    return Err(bad("solver operation or request out of range"));
                }
                if *op == 6 && target.is_none() {
                    return Err(bad("at-distance solve without target"));
                }
                if *step {
                    self.int(6);
                } else {
                    self.int(7);
                    let [a, b] = *ivl;
                    if !(0.0..=1.0).contains(&a) || !(0.0..=1.0).contains(&b) || a > b {
                        return Err(bad("solve interval outside [0, 1] or reversed"));
                    }
                    self.frac(*ivl)?;
                }
                self.int(*op as usize);
                self.vec(*p)?;
                self.tol(*tol)?;
                self.flag(target.is_some());
                self.num(target.unwrap_or(0.0))?;
                self.int(*req as usize);
            }
            Op::Join {
                end,
                other,
                other_end,
                gap,
            } => {
                if *end > 1 || *other_end > 1 {
                    return Err(bad("join end not 0 or 1"));
                }
                self.int(8);
                self.int(*end as usize);
                match other {
                    JoinOther::Ellipse(e) => {
                        self.int(0);
                        self.ellipse(e, false)?;
                    }
                    JoinOther::Circle(c) => {
                        if c.nurb_from.is_some() {
                            return Err(bad("nested pOptNurb"));
                        }
                        self.int(1);
                        self.circle(c)?;
                    }
                    JoinOther::BSplineOfA => self.int(2),
                }
                self.int(*other_end as usize);
                self.flag(gap.is_some());
                self.tol(gap.unwrap_or(0.0))?;
            }
            Op::Reverse { ivl } => {
                let [a, b] = *ivl;
                if !(0.0..=1.0).contains(&a) || !(0.0..=1.0).contains(&b) || a > b {
                    return Err(bad("reverse interval outside [0, 1] or reversed"));
                }
                self.int(9);
                self.frac(*ivl)?;
            }
            Op::Trim { ivl, mode } => {
                if *mode > 2 {
                    return Err(bad("trim mode"));
                }
                self.int(10);
                self.frac(*ivl)?;
                self.int(*mode as usize);
            }
            Op::Assert { given } => {
                self.int(11);
                self.flag(*given);
            }
            Op::IsCurrent => self.int(12),
            Op::PolarAssert => self.int(13),
            Op::Offset {
                tol,
                normal,
                dist,
                matched,
            } => {
                if tol.is_nan() || *tol <= 0.0 {
                    return Err(bad("offset tolerance not positive"));
                }
                nonzero(*normal)?;
                self.int(14);
                self.tol(*tol)?;
                self.vec(*normal)?;
                self.num(*dist)?;
                self.flag(*matched);
            }
            Op::SetRadius { r } => {
                if r.is_nan() || *r <= 0.0 {
                    return Err(bad("radius not positive"));
                }
                self.int(15);
                self.num(*r)?;
            }
            Op::MakeNurb => self.int(16),
            Op::To2d => self.int(17),
            Op::EditParam { domain, notify } => {
                if domain[0].is_nan() || domain[1].is_nan() || domain[0] >= domain[1] {
                    return Err(bad("reparameterisation interval empty"));
                }
                self.int(18);
                self.num(domain[0])?;
                self.num(domain[1])?;
                self.flag(*notify);
            }
            Op::Notify { post } => {
                self.int(19);
                self.flag(*post);
            }
            Op::IsArc => self.int(20),
            Op::IsNurbCircle { source, scale } => {
                self.int(21);
                match source {
                    NurbSource::A => self.int(0),
                    NurbSource::BSplineOfA => self.int(1),
                    NurbSource::Raw {
                        degree,
                        cpts,
                        knots,
                    } => {
                        self.int(2);
                        self.raw_nurb(*degree, cpts, knots)?;
                    }
                }
                self.flag(scale.is_some());
                self.num(scale.unwrap_or(0.0))?;
            }
        }
        Ok(())
    }
}

/// Whether `b` builds a circle (the circle-only ops need one).
const fn builds_circle(b: &Build) -> bool {
    match b {
        Build::Ellipse(_) | Build::EllipseCanonical { .. } => false,
        Build::Circle(_) | Build::Circle3 { .. } | Build::CircleCanonical { .. } => true,
        Build::ReadDb { circle, .. } => *circle,
    }
}

/// The flat program of `build` and `ops`.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for inputs the shim must not pass upstream (including
/// the circle-only ops on an ellipse).
pub fn encode(build: &Build, ops: &[Op]) -> Result<Vec<f64>, FfiError> {
    if ops.len() > MAX_OPS {
        return Err(bad("too many ops"));
    }
    let circle = builds_circle(build);
    if !circle
        && ops
            .iter()
            .any(|o| matches!(o, Op::SetRadius { .. } | Op::IsArc))
    {
        return Err(bad("circle-only op on an ellipse"));
    }
    let mut e = Enc(Vec::new());
    e.build(build)?;
    e.int(ops.len());
    for op in ops {
        e.op(op)?;
    }
    Ok(e.0)
}

/// Builds curve A, runs `ops` on it and returns the record (see the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for rejected inputs; [`FfiError::Status`] for a shim
/// failure.
pub fn run(build: &Build, ops: &[Op]) -> Result<Vec<f64>, FfiError> {
    let prog = encode(build, ops)?;
    let n_in = i64::try_from(prog.len()).map_err(|_| bad("program too long"))?;
    call_with_buffer(256, |buf: &mut [f64], len| {
        // SAFETY: `prog` is valid for `n_in` reads; `buf` for `buf.len()` writes; `len`
        // is a valid i64; nothing is retained.
        unsafe {
            sys::sc_curve_ellipse_p1_run(
                prog.as_ptr(),
                n_in,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
