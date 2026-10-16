//! FFI wrappers for unit `curve.line`.
//!
//! Shim: `harness/cpp/shim/curve_line.cpp` (`sc_curve_line_run`). Upstream:
//! `source/SMLib/src/SmLine.cpp`, `source/SMLib/inc/SmLine.h` (`SmLine`).
//!
//! [`run`] builds line A from a [`Make`], runs the [`Op`]s on it in order and returns
//! the record `make_status`, then per op `op status [values]` (values only for status
//! 0), then the state of A (nothing after a failed construction). Statuses are `0`
//! (`SM_SUCCESS`) or the `SmStatus`.
//!
//! A line state is `type dim P[3] V[3] dmin dmax scale inside_out` then the B-spline
//! part `knot_type form has_nurb [n p m cpts(x y z w) knots]` then the natural
//! interval. Solutions are `count` then per solution `type nvars nobjs obj0 obj1
//! start_value start0 start1 end_value end0 end1` (objects: `0` = A, `1` = the other
//! line or ellipse, `-1` = `NULL`).
//!
//! | [`Op`]             | upstream                                  | values |
//! |--------------------|-------------------------------------------|--------|
//! | `State`            | getters                                   | state |
//! | `Evaluate`         | `Evaluate`                                | `n+1` vectors |
//! | `EvaluateStep`     | `EvaluateSTEP`                            | `n+1` vectors |
//! | `EvaluatePoint`    | `EvaluatePoint`                           | point |
//! | `ToStep`/`ToNurbs` | `ConvertTFromNURBSToSTEP` / `ConvertTFromSTEPToNURBS` | value |
//! | `IsLine`           | `IsLine`                                  | `flag P V` |
//! | `Queries`          | `IsBounded`, `IsAnalytic`, `IsClosed`, `IsArc`, `GetDegree`, `GetNumberNaturalKnots`, `GetMaxAnalyticDomain`, `GetSTEPInterval`, `GetClassType`, `GetTypeString`, `GetClassString` | values, type chars, `-1`, class chars |
//! | `IsDegenerate`     | `IsDegenerate`                            | flag |
//! | `DomainBoundary`   | `IsDomainBoundary`                        | `SmSurfParamType` |
//! | `ApproxLength`     | `ApproximateLength`                       | `len turn` (`-7` when not asked) |
//! | `Length`           | `Length`                                  | length |
//! | `Ends`             | `GetEnds` (inherited)                     | `P0 P1 w0 w1` |
//! | `BBox`/`TightBBox` | `CalculateBoundingBox` / `CalculateTightBoundingBox` | normal `min max`, pseudo `basis[3] ivl[3]`, polar `state basis[3] domain` |
//! | `Tessellate`       | `Tessellate`                              | per requested array `count values` |
//! | `Drop`/`Gps`       | `DropPointFast` / `GlobalPointSolveSTEP`  | solutions |
//! | `StepInversion`    | `STEPInversion`                           | `t loc` |
//! | `IntersectLine`    | `IntersectWithLine`                       | `more` solutions |
//! | `IntersectEllipse` | `IntersectWithEllipse` (an `SmEllipse`)   | `more` solutions |
//! | `Join`             | `JoinWith`                                | |
//! | `Adjust`, `To2d`, `To3d`, `Refresh`, `Transform`, `Trim`, `EditParam` | `AdjustSTEPInterval`, `ConvertTo2D`, `ConvertTo3D`, `RefreshAnalytics`, `Transform`, `Trim`, `EditParameterization` | |
//! | `Offset`           | `CreateSimpleOffset`                      | `gap` state |
//! | `Reverse`          | `ReverseParameterization`                 | new interval |
//! | `Deform`           | `DeformToEndPointTargets`                 | `has [type dim kt form has_nurb ...]` |
//! | `Eq`               | `operator==`                              | flag |
//! | `Copy`             | `Copy`                                    | state |
//! | `Analytic`         | `CreateAnalyticCurve` (inherited)         | `has [state]` |
//! | `Assert`           | `AssertValid`                             | `ok n (index tol value)*` |
//! | `KindOf`           | `IsKindOf`                                | flag |
//! | `Canonical`        | both `GetCanonical`                       | `P V dom io P U scale dom io` |
//! | `SetScale`, `SetDomain` | `SetLineScale`, `SetAnalyticDomain`  | |
//!
//! [`run`] accepts only finite inputs of magnitude at most [`MAX_COORD`], well-formed
//! clamped B-splines (degree at most [`MAX_DEGREE`], at most [`MAX_INDEX`] + 1
//! control points), derivative counts up to 8, at most 64 segments or ops, and
//! complete database streams built by [`line_db_bytes`] (upstream reads past the end
//! of a short stream: uninitialised values or a stream exception).

use std::fmt::Write as _;

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_curve_line_run(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Largest control point index `n`.
pub const MAX_INDEX: usize = 64;
/// Largest degree.
pub const MAX_DEGREE: usize = 9;
/// Largest input magnitude.
pub const MAX_COORD: f64 = 1e9;
/// Most ops per program.
pub const MAX_OPS: usize = 64;

/// A clamped B-spline: `SmBSplineCurve(dim, n, m, p, Pw, U)`.
#[derive(Debug, Clone, PartialEq)]
pub struct BSplineData {
    /// Image dimension, 2 or 3.
    pub dim: u32,
    /// Degree.
    pub p: usize,
    /// Control points as stored (`x y z w`, `w` = `NL_NOW` = `-f64::MAX` when
    /// non-rational).
    pub cpts: Vec<[f64; 4]>,
    /// Knots (`n + p + 2` of them).
    pub knots: Vec<f64>,
}

/// The `SmBSplineCurve` part of a database line ([`LineDb`]): `n p rat dim [kt form]`
/// then the control point values and knots in reading order.
#[derive(Debug, Clone, PartialEq)]
pub struct BaseDb {
    /// Highest control point index.
    pub n: i64,
    /// Degree.
    pub p: i16,
    /// Rational flag.
    pub rat: i16,
    /// Dimension written (2 or 3).
    pub dim: i16,
    /// Knot type (version > 32).
    pub kt: u64,
    /// Form (version > 32).
    pub form: u64,
    /// Control point values (2 to 4 each) then knots.
    pub vals: Vec<f64>,
}

/// A line as written by `SmLine::WriteToDB`.
#[derive(Debug, Clone, PartialEq)]
pub struct LineDb {
    /// `m_vLinePoint`.
    pub point: [f64; 3],
    /// `m_vLineVector`.
    pub vector: [f64; 3],
    /// Analytic domain ends as written.
    pub domain: [f64; 2],
    /// `m_dScale`.
    pub scale: f64,
    /// `m_bInsideOut`.
    pub inside_out: i32,
    /// The B-spline part.
    pub base: BaseDb,
}

/// How line A is built.
#[derive(Debug, Clone, PartialEq)]
pub enum Make {
    /// `new SmLine()`.
    Default,
    /// `SmLine(a, b, dim, ctx)`.
    Points {
        /// Start.
        a: [f64; 3],
        /// End.
        b: [f64; 3],
        /// Dimension.
        dim: u32,
    },
    /// `SmLine(p, v, dim, TRUE, ctx, inside_out)`.
    Infinite {
        /// Point.
        p: [f64; 3],
        /// Non-unit vector.
        v: [f64; 3],
        /// Dimension.
        dim: u32,
        /// Inside-out flag.
        inside_out: bool,
    },
    /// `SmLine(p, v, SmExtent1d(d0, d1), scale, dim, ctx, nurb, inside_out)`.
    Bounded {
        /// Point.
        p: [f64; 3],
        /// Vector (unitised by upstream).
        v: [f64; 3],
        /// Domain ends.
        domain: [f64; 2],
        /// Scale.
        scale: f64,
        /// Dimension.
        dim: u32,
        /// Inside-out flag.
        inside_out: bool,
        /// Optional NURB to copy.
        nurb: Option<BSplineData>,
    },
    /// `SmLine::CreateLineSegment(ctx, dim, a, b, l, interval)`.
    Segment {
        /// Dimension.
        dim: u32,
        /// Start.
        a: [f64; 3],
        /// End.
        b: [f64; 3],
        /// Optional parameter range.
        interval: Option<[f64; 2]>,
    },
    /// `SmLine::CreateCanonical(ctx, p, v, l)`.
    Canonical {
        /// Point.
        p: [f64; 3],
        /// Vector.
        v: [f64; 3],
    },
    /// `SmLine::ReadFromDB(SmLine_TYPE, db, dim, ctx, NULL, version)`.
    Read {
        /// Requested dimension.
        dim: u32,
        /// Database version.
        version: u32,
        /// Binary (else ASCII) stream.
        binary: bool,
        /// Stream content.
        db: LineDb,
    },
}

/// An interval operand.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ivl {
    /// `GetNaturalInterval()` at op time.
    Natural,
    /// `SmExtent1d(a, b)`.
    Explicit(f64, f64),
    /// `GetSTEPInterval()` at op time.
    Step,
}

/// One operation on line A.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// The state of A.
    State,
    /// `Evaluate(t, n, TRUE, ...)`.
    Evaluate(f64, usize),
    /// `EvaluateSTEP(t, n, TRUE, ...)`.
    EvaluateStep(f64, usize),
    /// `EvaluatePoint(t)`.
    EvaluatePoint(f64),
    /// `ConvertTFromNURBSToSTEP(t)`.
    ToStep(f64),
    /// `ConvertTFromSTEPToNURBS(t)`.
    ToNurbs(f64),
    /// `IsLine(5, 0, ...)`.
    IsLine,
    /// The simple queries.
    Queries,
    /// `IsDegenerate(tol, interval)`.
    IsDegenerate(f64, Option<Ivl>),
    /// `IsDomainBoundary(SmExtent2d((u0, v0), (u1, v1)), tol)`.
    DomainBoundary([f64; 4], f64),
    /// `ApproximateLength(ivl, 5, want_turn ? &ang : NULL)`.
    ApproxLength(Ivl, bool),
    /// `Length(ivl, 0)`.
    Length(Ivl),
    /// `GetEnds`.
    Ends,
    /// `CalculateBoundingBox(ivl, ...)` with flags `1` normal, `2` pseudo, `4` polar.
    BBox(Ivl, u8),
    /// `CalculateTightBoundingBox(ivl, ...)`.
    TightBBox(Ivl),
    /// `Tessellate(ivl, .., nseg, ...)` with flags `1` params, `2` points, `4` tangents.
    Tessellate(Ivl, u32, u8),
    /// `DropPointFast(ivl, op, p, NULL, tol, target, SM_SR_SINGLE)`.
    Drop {
        /// Interval.
        ivl: Ivl,
        /// `SmSolverOperationType` value.
        op: u8,
        /// Test point.
        p: [f64; 3],
        /// Tolerance.
        tol: f64,
        /// Optional target distance.
        target: Option<f64>,
    },
    /// `GlobalPointSolveSTEP(ivl, op, p, tol, NULL, NULL, SM_SR_ALL)`.
    Gps {
        /// Interval.
        ivl: Ivl,
        /// `SmSolverOperationType` value.
        op: u8,
        /// Test point.
        p: [f64; 3],
        /// Tolerance.
        tol: f64,
    },
    /// `STEPInversion(p, t, &loc)`.
    StepInversion([f64; 3]),
    /// `IntersectWithLine(ivl, B, other_ivl, tol)`; `None`: B is A itself.
    IntersectLine {
        /// Interval on A.
        ivl: Ivl,
        /// Line B.
        other: Option<Box<Make>>,
        /// Interval on B.
        other_ivl: Ivl,
        /// Tolerance.
        tol: f64,
    },
    /// `IntersectWithEllipse(ivl, SmEllipse(c, x, y, dom, r0, r1), its natural interval,
    /// tol)`.
    IntersectEllipse {
        /// Interval on A.
        ivl: Ivl,
        /// Center, X axis, Y axis.
        frame: [[f64; 3]; 3],
        /// Analytic domain (degrees).
        domain: [f64; 2],
        /// Radii.
        radii: [f64; 2],
        /// Tolerance.
        tol: f64,
    },
    /// `JoinWith(end_this, other, end_other, tol)`; the other curve is a line (`Ok`)
    /// or a B-spline (`Err`).
    Join {
        /// End of A.
        end_this: u8,
        /// Other curve.
        other: Result<Box<Make>, BSplineData>,
        /// End of the other curve.
        end_other: u8,
        /// Optional gap tolerance.
        tol: Option<f64>,
    },
    /// `AdjustSTEPInterval(ivl)`.
    Adjust(Ivl),
    /// `ConvertTo2D()`.
    To2d,
    /// `ConvertTo3D()`.
    To3d,
    /// `CreateSimpleOffset(ctx, 0.01, n, d, ...)`.
    Offset([f64; 3], f64),
    /// `RefreshAnalytics()`.
    Refresh,
    /// `ReverseParameterization(ivl, ...)`.
    Reverse(Ivl),
    /// `Transform(SmAxis2Placement(o, x, y), scale)`.
    Transform([[f64; 3]; 3], Option<[f64; 3]>),
    /// `DeformToEndPointTargets`.
    Deform {
        /// Target start point.
        start: Option<[f64; 3]>,
        /// Target end point.
        end: Option<[f64; 3]>,
        /// Pass an output curve pointer.
        want_new: bool,
        /// Start tangent and its `SmInValueType`.
        start_tan: (Option<[f64; 3]>, u8),
        /// End tangent and its `SmInValueType`.
        end_tan: (Option<[f64; 3]>, u8),
    },
    /// `Trim(ivl, mode)` (`0` TRUE, `1` FALSE, `2` UNSURE).
    Trim(Ivl, u8),
    /// `EditParameterization(ivl)`.
    EditParam(Ivl),
    /// `A == B`; `None`: `A == A`.
    Eq(Option<Box<Make>>),
    /// `Copy`.
    Copy,
    /// `CreateAnalyticCurve(ctx, ivl, ...)`.
    Analytic(Ivl),
    /// `AssertValid(list?, given ? SM_LEVEL_GIVEN : SM_LEVEL_0, SM_NO_WALK)`.
    Assert {
        /// `SM_LEVEL_GIVEN`.
        given: bool,
        /// Pass a report list.
        with_list: bool,
    },
    /// `IsKindOf(t)`.
    KindOf(u32),
    /// Both `GetCanonical` overloads.
    Canonical,
    /// `SetLineScale(s)`.
    SetScale(f64),
    /// `SetAnalyticDomain(SmExtent1d(a, b))`.
    SetDomain(f64, f64),
}

fn bad(msg: &str) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

fn finite(x: f64) -> Result<f64, FfiError> {
    if x.is_finite() && x.abs() <= MAX_COORD {
        Ok(x)
    } else {
        Err(bad("non-finite or huge value"))
    }
}

fn flag(b: bool) -> f64 {
    if b {
        1.0
    } else {
        0.0
    }
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
        self.0.push(flag(b));
    }
    fn vec(&mut self, v: [f64; 3]) -> Result<(), FfiError> {
        v.iter().try_for_each(|&x| self.num(x))
    }
    fn dim(&mut self, d: u32) -> Result<(), FfiError> {
        if d != 2 && d != 3 {
            return Err(bad("dimension must be 2 or 3"));
        }
        self.int(d as usize);
        Ok(())
    }
    fn ivl(&mut self, i: Ivl) -> Result<(), FfiError> {
        match i {
            Ivl::Natural => self.0.extend([0.0, 0.0, 0.0]),
            Ivl::Explicit(a, b) => {
                self.0.push(1.0);
                self.num(a)?;
                self.num(b)?;
            }
            Ivl::Step => self.0.extend([2.0, 0.0, 0.0]),
        }
        Ok(())
    }

    fn bspline(&mut self, c: &BSplineData) -> Result<(), FfiError> {
        validate_bspline(c)?;
        self.dim(c.dim)?;
        let n = c.cpts.len() - 1;
        self.int(n);
        self.int(c.p);
        self.int(c.knots.len() - 1);
        for cp in &c.cpts {
            self.num(cp[0])?;
            self.num(cp[1])?;
            self.num(cp[2])?;
            self.0.push(cp[3]);
        }
        for &k in &c.knots {
            self.num(k)?;
        }
        Ok(())
    }

    fn make(&mut self, m: &Make) -> Result<(), FfiError> {
        match m {
            Make::Default => self.int(0),
            Make::Points { a, b, dim } => {
                self.int(1);
                self.vec(*a)?;
                self.vec(*b)?;
                self.dim(*dim)?;
            }
            Make::Infinite {
                p,
                v,
                dim,
                inside_out,
            } => {
                self.int(2);
                self.vec(*p)?;
                self.vec(*v)?;
                self.dim(*dim)?;
                self.flag(*inside_out);
            }
            Make::Bounded {
                p,
                v,
                domain,
                scale,
                dim,
                inside_out,
                nurb,
            } => {
                self.int(3);
                self.vec(*p)?;
                self.vec(*v)?;
                self.num(domain[0])?;
                self.num(domain[1])?;
                self.num(*scale)?;
                self.dim(*dim)?;
                self.flag(*inside_out);
                self.flag(nurb.is_some());
                if let Some(c) = nurb {
                    self.bspline(c)?;
                }
            }
            Make::Segment {
                dim,
                a,
                b,
                interval,
            } => {
                self.int(4);
                self.dim(*dim)?;
                self.vec(*a)?;
                self.vec(*b)?;
                self.flag(interval.is_some());
                let [x0, x1] = interval.unwrap_or([0.0, 1.0]);
                self.num(x0)?;
                self.num(x1)?;
            }
            Make::Canonical { p, v } => {
                self.int(5);
                self.vec(*p)?;
                self.vec(*v)?;
            }
            Make::Read {
                dim,
                version,
                binary,
                db,
            } => {
                if *version > 100 {
                    return Err(bad("version"));
                }
                let bytes = line_db_bytes(db, *binary, u64::from(*version))?;
                self.int(6);
                self.dim(*dim)?;
                self.int(*version as usize);
                self.flag(*binary);
                self.int(bytes.len());
                self.0.extend(bytes.iter().map(|&b| f64::from(b)));
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn op(&mut self, op: &Op) -> Result<(), FfiError> {
        match op {
            Op::State => self.int(1),
            Op::Evaluate(t, n) | Op::EvaluateStep(t, n) => {
                self.int(if matches!(op, Op::Evaluate(..)) { 2 } else { 3 });
                self.num(*t)?;
                if *n > 8 {
                    return Err(bad("derivative count"));
                }
                self.int(*n);
            }
            Op::EvaluatePoint(t) => {
                self.int(4);
                self.num(*t)?;
            }
            Op::ToStep(t) => {
                self.int(5);
                self.num(*t)?;
            }
            Op::ToNurbs(t) => {
                self.int(6);
                self.num(*t)?;
            }
            Op::IsLine => self.int(7),
            Op::Queries => self.int(8),
            Op::IsDegenerate(tol, ivl) => {
                self.int(9);
                self.num(*tol)?;
                self.flag(ivl.is_some());
                self.ivl(ivl.unwrap_or(Ivl::Natural))?;
            }
            Op::DomainBoundary(d, tol) => {
                self.int(10);
                for &x in d {
                    self.num(x)?;
                }
                self.num(*tol)?;
            }
            Op::ApproxLength(i, turn) => {
                self.int(11);
                self.ivl(*i)?;
                self.flag(*turn);
            }
            Op::Length(i) => {
                self.int(12);
                self.ivl(*i)?;
            }
            Op::Ends => self.int(13),
            Op::BBox(i, f) => {
                if !(1..=7).contains(f) {
                    return Err(bad("box flags"));
                }
                self.int(14);
                self.ivl(*i)?;
                self.int(usize::from(*f));
            }
            Op::TightBBox(i) => {
                self.int(15);
                self.ivl(*i)?;
            }
            Op::Tessellate(i, n, f) => {
                if *n > 64 || *f > 7 {
                    return Err(bad("tessellation"));
                }
                self.int(16);
                self.ivl(*i)?;
                self.int(*n as usize);
                self.int(usize::from(*f));
            }
            Op::Drop {
                ivl,
                op,
                p,
                tol,
                target,
            } => {
                if *op > 6 {
                    return Err(bad("solver operation"));
                }
                self.int(17);
                self.ivl(*ivl)?;
                self.int(usize::from(*op));
                self.vec(*p)?;
                self.num(*tol)?;
                self.flag(target.is_some());
                self.num(target.unwrap_or(0.0))?;
            }
            Op::Gps { ivl, op, p, tol } => {
                if *op > 6 {
                    return Err(bad("solver operation"));
                }
                self.int(18);
                self.ivl(*ivl)?;
                self.int(usize::from(*op));
                self.vec(*p)?;
                self.num(*tol)?;
            }
            Op::StepInversion(p) => {
                self.int(19);
                self.vec(*p)?;
            }
            Op::IntersectLine {
                ivl,
                other,
                other_ivl,
                tol,
            } => {
                self.int(20);
                self.ivl(*ivl)?;
                self.flag(other.is_none());
                if let Some(m) = other {
                    self.make(m)?;
                }
                self.ivl(*other_ivl)?;
                self.num(*tol)?;
            }
            Op::IntersectEllipse {
                ivl,
                frame,
                domain,
                radii,
                tol,
            } => {
                self.int(21);
                self.ivl(*ivl)?;
                for v in frame {
                    self.vec(*v)?;
                }
                self.num(domain[0])?;
                self.num(domain[1])?;
                self.num(radii[0])?;
                self.num(radii[1])?;
                self.num(*tol)?;
            }
            Op::Join {
                end_this,
                other,
                end_other,
                tol,
            } => {
                if *end_this > 2 || *end_other > 2 {
                    return Err(bad("join end"));
                }
                self.int(22);
                self.int(usize::from(*end_this));
                self.flag(other.is_ok());
                match other {
                    Ok(m) => self.make(m)?,
                    Err(c) => self.bspline(c)?,
                }
                self.int(usize::from(*end_other));
                self.flag(tol.is_some());
                self.num(tol.unwrap_or(0.0))?;
            }
            Op::Adjust(i) => {
                self.int(23);
                self.ivl(*i)?;
            }
            Op::To2d => self.int(24),
            Op::To3d => self.int(25),
            Op::Offset(n, d) => {
                self.int(26);
                self.vec(*n)?;
                self.num(*d)?;
            }
            Op::Refresh => self.int(27),
            Op::Reverse(i) => {
                self.int(28);
                self.ivl(*i)?;
            }
            Op::Transform(f, s) => {
                self.int(29);
                for v in f {
                    self.vec(*v)?;
                }
                self.flag(s.is_some());
                self.vec(s.unwrap_or([1.0, 1.0, 1.0]))?;
            }
            Op::Deform {
                start,
                end,
                want_new,
                start_tan,
                end_tan,
            } => {
                if start_tan.1 > 2 || end_tan.1 > 2 {
                    return Err(bad("tangent type"));
                }
                self.int(30);
                self.flag(start.is_some());
                self.vec(start.unwrap_or([0.0; 3]))?;
                self.flag(end.is_some());
                self.vec(end.unwrap_or([0.0; 3]))?;
                self.flag(*want_new);
                self.flag(start_tan.0.is_some());
                self.vec(start_tan.0.unwrap_or([0.0; 3]))?;
                self.int(usize::from(start_tan.1));
                self.flag(end_tan.0.is_some());
                self.vec(end_tan.0.unwrap_or([0.0; 3]))?;
                self.int(usize::from(end_tan.1));
            }
            Op::Trim(i, mode) => {
                if *mode > 2 {
                    return Err(bad("trim mode"));
                }
                self.int(31);
                self.ivl(*i)?;
                self.int(usize::from(*mode));
            }
            Op::EditParam(i) => {
                self.int(32);
                self.ivl(*i)?;
            }
            Op::Eq(other) => {
                self.int(33);
                self.flag(other.is_none());
                if let Some(m) = other {
                    self.make(m)?;
                }
            }
            Op::Copy => self.int(34),
            Op::Analytic(i) => {
                self.int(35);
                self.ivl(*i)?;
            }
            Op::Assert { given, with_list } => {
                self.int(36);
                self.flag(*given);
                self.flag(*with_list);
            }
            Op::KindOf(t) => {
                if *t > 100_000 {
                    return Err(bad("type code"));
                }
                self.int(37);
                self.int(*t as usize);
            }
            Op::Canonical => self.int(38),
            Op::SetScale(s) => {
                self.int(39);
                self.num(*s)?;
            }
            Op::SetDomain(a, b) => {
                self.int(40);
                self.num(*a)?;
                self.num(*b)?;
            }
        }
        Ok(())
    }
}

/// `NL_NOW`: the weight of a non-rational control point.
const NOW: f64 = -f64::MAX;

/// Checks a B-spline is clamped and well formed (what upstream assumes).
fn validate_bspline(c: &BSplineData) -> Result<(), FfiError> {
    let n1 = c.cpts.len();
    if !(2..=MAX_INDEX + 1).contains(&n1) || !(1..=MAX_DEGREE).contains(&c.p) || c.p >= n1 {
        return Err(bad("B-spline size or degree"));
    }
    if c.knots.len() != n1 + c.p + 1 {
        return Err(bad("knot count"));
    }
    if c.knots.windows(2).any(|w| w.first() > w.last()) {
        return Err(bad("knots decreasing"));
    }
    let (Some(&k0), Some(&km)) = (c.knots.first(), c.knots.last()) else {
        return Err(bad("knot count"));
    };
    let head = c.knots.iter().take(c.p + 1);
    let tail = c.knots.iter().rev().take(c.p + 1);
    if km <= k0 || km.is_nan() || head.clone().any(|&k| k != k0) || tail.clone().any(|&k| k != km) {
        return Err(bad("knots not clamped"));
    }
    // interior multiplicity at most p
    let interior: Vec<f64> = c
        .knots
        .iter()
        .copied()
        .filter(|&k| k != k0 && k != km)
        .collect();
    let mut run = 0usize;
    let mut last = f64::NAN;
    for k in interior {
        run = if k == last { run + 1 } else { 1 };
        last = k;
        if run > c.p {
            return Err(bad("interior knot multiplicity"));
        }
    }
    let rational = c.cpts.first().is_some_and(|cp| cp[3] != NOW);
    for cp in &c.cpts {
        if (cp[3] != NOW) != rational || (rational && !(cp[3] > 0.0 && cp[3] <= MAX_COORD)) {
            return Err(bad("weights"));
        }
    }
    Ok(())
}

/// The bytes `SmLine::WriteToDB` writes for `db` (binary: native-endian doubles and a
/// 4-byte boolean, then the B-spline header `n` (8 bytes), `p rat dim` (2 bytes
/// each), for `version > 32` `kt form` (8 bytes each) and the values; ASCII: one group
/// per line, values printed with 17 significant digits).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a B-spline header out of range (`n` above
/// [`MAX_INDEX`], degree above [`MAX_DEGREE`], dimension not 2 or 3, knot type or
/// form outside their enums), a value count that does not match it, or non-finite
/// values.
pub fn line_db_bytes(db: &LineDb, binary: bool, version: u64) -> Result<Vec<u8>, FfiError> {
    let b = &db.base;
    let n = usize::try_from(b.n).map_err(|_| bad("negative n"))?;
    let p = usize::try_from(b.p).map_err(|_| bad("negative degree"))?;
    if n > MAX_INDEX || p > MAX_DEGREE || (b.dim != 2 && b.dim != 3) || b.kt > 4 || b.form > 6 {
        return Err(bad("database header"));
    }
    let per = 2 + usize::from(b.dim != 2) + usize::from(b.rat != 0);
    if b.vals.len() != (n + 1) * per + n + p + 2 {
        return Err(bad("database value count"));
    }
    let line_vals = db
        .point
        .iter()
        .chain(db.vector.iter())
        .chain(db.domain.iter())
        .chain(core::iter::once(&db.scale))
        .chain(b.vals.iter());
    for &x in line_vals {
        finite(x)?;
    }
    if binary {
        let mut o = Vec::new();
        for x in db
            .point
            .iter()
            .chain(db.vector.iter())
            .chain(db.domain.iter())
        {
            o.extend_from_slice(&x.to_ne_bytes());
        }
        o.extend_from_slice(&db.scale.to_ne_bytes());
        o.extend_from_slice(&db.inside_out.to_ne_bytes());
        o.extend_from_slice(&b.n.to_ne_bytes());
        o.extend_from_slice(&b.p.to_ne_bytes());
        o.extend_from_slice(&b.rat.to_ne_bytes());
        o.extend_from_slice(&b.dim.to_ne_bytes());
        if version > 32 {
            o.extend_from_slice(&b.kt.to_ne_bytes());
            o.extend_from_slice(&b.form.to_ne_bytes());
        }
        for x in &b.vals {
            o.extend_from_slice(&x.to_ne_bytes());
        }
        Ok(o)
    } else {
        let f = |x: &f64| format!("{x:.17e}");
        let mut t = String::new();
        let line = |t: &mut String, xs: &[f64]| {
            let s: Vec<String> = xs.iter().map(f).collect();
            t.push_str(&s.join(" "));
            t.push('\n');
        };
        line(&mut t, &db.point);
        line(&mut t, &db.vector);
        line(&mut t, &db.domain);
        line(&mut t, &[db.scale]);
        let _ = writeln!(t, "{}", db.inside_out);
        let _ = write!(t, "{}\n{}\n{}\n{}\n", b.n, b.p, b.rat, b.dim);
        if version > 32 {
            let _ = write!(t, "{}\n{}\n", b.kt, b.form);
        }
        let (pts, knots) = b.vals.split_at((n + 1) * per);
        for c in pts.chunks(per) {
            line(&mut t, c);
        }
        for k in knots {
            line(&mut t, &[*k]);
        }
        Ok(t.into_bytes())
    }
}

/// The flat program [`run`] hands the shim.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for inputs [`run`] rejects.
pub fn encode(make: &Make, ops: &[Op]) -> Result<Vec<f64>, FfiError> {
    if ops.len() > MAX_OPS {
        return Err(bad("too many ops"));
    }
    let mut e = Enc(Vec::new());
    e.make(make)?;
    e.int(ops.len());
    for op in ops {
        e.op(op)?;
    }
    Ok(e.0)
}

/// Builds line A from `make`, runs `ops` on it and returns the record (see the
/// module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for rejected inputs; [`FfiError::Status`] for a shim
/// failure.
pub fn run(make: &Make, ops: &[Op]) -> Result<Vec<f64>, FfiError> {
    let prog = encode(make, ops)?;
    let n_in = i64::try_from(prog.len()).map_err(|_| bad("program too long"))?;
    call_with_buffer(256, |buf: &mut [f64], len| {
        // SAFETY: `prog` is valid for `n_in` reads; `buf` for `buf.len()` writes; `len`
        // is a valid i64; nothing is retained.
        unsafe {
            sys::sc_curve_line_run(
                prog.as_ptr(),
                n_in,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
