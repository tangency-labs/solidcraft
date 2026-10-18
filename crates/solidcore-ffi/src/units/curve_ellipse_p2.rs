//! FFI wrappers for unit `curve.ellipse_p2`.
//!
//! Shim: `harness/cpp/shim/curve_ellipse_p2.cpp` (`sc_curve_ellipse_p2_run`).
//! Upstream: `source/SMLib/src/SmEllipse.cpp`, `source/SMLib/inc/SmEllipse.h`.
//!
//! A program builds ellipse A (`new (ctx) SmEllipse(C, X, Y, SmExtent1d(d0, d1), rx, ry,
//! dim, &ctx, pOptNurb, bInsideOut)`, `pOptNurb` the NURB of a second ellipse when
//! given) and runs [`Op`]s on it. The record is, when A has a NURB, per op `op status
//! [outputs]` (outputs only for status `0` = `SM_SUCCESS`, else the `SmStatus`; `-2`:
//! the other ellipse of [`Op::IntersectEllipse`] has no NURB and is not called), then
//! the state of A:
//!
//! | [`Op`]               | upstream                                          | outputs |
//! |----------------------|---------------------------------------------------|---------|
//! | `State`              | queries                                           | `type class_type degree start end analytic circle maxdom[2] domain_boundary canon_status O X Y r1 r2 dom[2] io io polar_class_type`, polar class string, type string, class string (chars then `-1`), one `IsKindOf` flag per type |
//! | `EvalStep`           | `EvaluateSTEP(t, nd)`                             | `nd + 1` vectors |
//! | `EvalStepPoint`      | `EvaluateSTEPPoint(t)`                            | point |
//! | `StepInv`            | `STEPInversion(P, loc?, zone_tol)`                | `t`, `loc` when classifying |
//! | `ToNurbs`            | `ConvertTFromSTEPToNURBS(t)`                      | `t` |
//! | `PolarToNurbs`       | `m_vPolarConverter.ConvertToNURBSParameter(t, .., zone_tol)` | `t exact` |
//! | `AngleToNurb`        | `m_vPolarConverter.AngleParamToNurbParam`         | `t` |
//! | `IsDegenerate`       | `IsDegenerate(tol)`                               | flag |
//! | `StartPlane`         | `TransformPointToStartPlane(P, tol, .., snap)`    | `point n angles(n, 2 when 0) inside` |
//! | `Drop`               | `DropPointFast`                                   | solutions |
//! | `IntersectEllipse`   | `IntersectWithEllipse`                            | `more` solutions |
//! | `IntersectLine`      | `IntersectWithLine` (bounded `SmLine`)            | `more` solutions |
//! | `Transform`          | `Transform`                                       | |
//! | `To3d`               | `ConvertTo3D`                                     | |
//! | `SetUp`              | `m_vPolarConverter.SetUpPolarConversion(this, TRUE, ..)` | |
//! | `Eq`                 | `operator==` (other ellipse)                      | flag |
//! | `SetStepInterval`    | `SetSTEPInterval`                                 | |
//!
//! Intervals are given as normalised parameters `[f0, f1]` of the curve's natural
//! interval. Solutions are `count`, then per solution `type num_vars num_objs obj0 obj1
//! value start[0] start[1]` (objects: `0` A, `1` the other curve, `-1` none; `start[1]`
//! `0` for one variable). A state is `O[3] X[3] Y[3] dmin dmax rx ry inside_out dim
//! form has_nurb [n p m cpts(x y z w) knots] possible has_curve nk knots na angles ne
//! exact`.

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

/// Largest magnitude of coordinates, lengths, parameters and tolerances.
pub const MAX_COORD: f64 = 1e6;
/// Largest magnitude of a STEP domain end (degrees).
pub const MAX_ANGLE: f64 = 720.0;
/// Most ops per program.
pub const MAX_OPS: usize = 64;
/// Most types of [`Op::State`].
pub const MAX_TYPES: usize = 64;

mod sys {
    extern "C" {
        pub fn sc_curve_ellipse_p2_run(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Constructor arguments of an ellipse.
#[derive(Debug, Clone, PartialEq)]
pub struct EllipseIn {
    /// Center.
    pub center: [f64; 3],
    /// X axis (to the 0 degree point); nonzero.
    pub x_axis: [f64; 3],
    /// Y axis (to the 90 degree point); nonzero.
    pub y_axis: [f64; 3],
    /// STEP domain ends in degrees (`SmExtent1d(d0, d1)`); its length is in
    /// `[1e-4, 360]`.
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

/// A bounded line `SmLine(P, V, SmExtent1d(d0, d1), scale, 3, &ctx)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineIn {
    /// Line point.
    pub point: [f64; 3],
    /// Direction (nonzero).
    pub vector: [f64; 3],
    /// STEP domain ends.
    pub domain: [f64; 2],
    /// Speed (positive).
    pub scale: f64,
}

/// An operation on ellipse A (see the module docs).
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Queries; `IsKindOf` for each type.
    State {
        /// Types to test.
        types: Vec<i64>,
    },
    /// `EvaluateSTEP(t, nd)` (`nd <= 4`).
    EvalStep {
        /// STEP angle.
        t: f64,
        /// Derivative count.
        nd: u32,
    },
    /// `EvaluateSTEPPoint(t)`.
    EvalStepPoint {
        /// STEP angle.
        t: f64,
    },
    /// `STEPInversion`.
    StepInv {
        /// Point.
        p: [f64; 3],
        /// Classify the point.
        classify: bool,
        /// Zone tolerance (`>= 0`).
        zone_tol: f64,
    },
    /// `ConvertTFromSTEPToNURBS`.
    ToNurbs {
        /// STEP angle.
        t: f64,
    },
    /// `SmPolarConversion::ConvertToNURBSParameter`.
    PolarToNurbs {
        /// Polar angle.
        t: f64,
        /// Zone tolerance (`>= 0`).
        zone_tol: f64,
    },
    /// `SmPolarConversion::AngleParamToNurbParam`.
    AngleToNurb {
        /// `start_nurb end_nurb start_angle end_angle angle`.
        args: [f64; 5],
    },
    /// `IsDegenerate(tol)`.
    IsDegenerate {
        /// Tolerance.
        tol: f64,
    },
    /// `TransformPointToStartPlane`.
    StartPlane {
        /// Point.
        p: [f64; 3],
        /// Seam tolerance (`>= 0`).
        tol: f64,
        /// Snap to seams.
        snap: bool,
    },
    /// `DropPointFast`.
    Drop {
        /// Normalised interval.
        ivl: [f64; 2],
        /// `SmSolverOperationType` (`0..=9`).
        op: i32,
        /// Test point.
        p: [f64; 3],
        /// In-pointing vector.
        in_vec: Option<[f64; 3]>,
        /// Distance tolerance (`>= 0`).
        tol: f64,
        /// Target distance.
        target: Option<f64>,
        /// `SmSolutionRequestedType` (`0` single, `1` all).
        req: i32,
    },
    /// `IntersectWithEllipse`.
    IntersectEllipse {
        /// Normalised interval of A.
        ivl: [f64; 2],
        /// The other ellipse; `None` = A itself.
        other: Option<EllipseIn>,
        /// Normalised interval of the other ellipse.
        other_ivl: [f64; 2],
        /// Tolerance (`>= 0`).
        tol: f64,
    },
    /// `IntersectWithLine`.
    IntersectLine {
        /// Normalised interval of A.
        ivl: [f64; 2],
        /// The line.
        line: LineIn,
        /// Normalised interval of the line.
        line_ivl: [f64; 2],
        /// Tolerance (`>= 0`).
        tol: f64,
    },
    /// `Transform` by the placement `SetCanonical(origin, x, y)`.
    Transform {
        /// Origin.
        origin: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// Scale.
        scale: Option<[f64; 3]>,
    },
    /// `ConvertTo3D`.
    To3d,
    /// `SetUpPolarConversion`.
    SetUp,
    /// `operator==`.
    Eq {
        /// The other ellipse.
        other: EllipseIn,
    },
    /// `SetSTEPInterval(SmExtent1d(d0, d1))`.
    SetStepInterval {
        /// Domain ends.
        domain: [f64; 2],
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
        if !(0.0..=1.0).contains(&a) || !(0.0..=1.0).contains(&b) || a > b {
            return Err(bad("normalised interval outside [0, 1] or reversed"));
        }
        self.num(a)?;
        self.num(b)
    }
    fn ellipse(&mut self, e: &EllipseIn, with_nurb: bool) -> Result<(), FfiError> {
        nonzero(e.x_axis)?;
        nonzero(e.y_axis)?;
        let [d0, d1] = e.domain;
        if d0.is_nan() || d1.is_nan() || d0.abs() > MAX_ANGLE || d1.abs() > MAX_ANGLE {
            return Err(bad("domain end beyond 720 degrees"));
        }
        let len = (d1 - d0).abs();
        if !(1e-4..=360.0).contains(&len) {
            return Err(bad("domain length outside [1e-4, 360]"));
        }
        if e.radii.iter().any(|r| r.is_nan() || *r <= 0.0) {
            return Err(bad("radius not positive"));
        }
        if e.dim != 2 && e.dim != 3 {
            return Err(bad("dimension not 2 or 3"));
        }
        self.vec(e.center)?;
        self.vec(e.x_axis)?;
        self.vec(e.y_axis)?;
        self.num(d0)?;
        self.num(d1)?;
        self.num(e.radii[0])?;
        self.num(e.radii[1])?;
        self.int(e.dim as usize);
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
    fn line(&mut self, l: &LineIn) -> Result<(), FfiError> {
        nonzero(l.vector)?;
        if l.scale.is_nan() || l.scale <= 0.0 {
            return Err(bad("line scale not positive"));
        }
        self.vec(l.point)?;
        self.vec(l.vector)?;
        self.num(l.domain[0])?;
        self.num(l.domain[1])?;
        self.num(l.scale)
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
            Op::EvalStep { t, nd } => {
                if *nd > 4 {
                    return Err(bad("too many derivatives"));
                }
                self.int(2);
                self.num(*t)?;
                self.int(*nd as usize);
            }
            Op::EvalStepPoint { t } => {
                self.int(3);
                self.num(*t)?;
            }
            Op::StepInv {
                p,
                classify,
                zone_tol,
            } => {
                self.int(4);
                self.vec(*p)?;
                self.flag(*classify);
                self.tol(*zone_tol)?;
            }
            Op::ToNurbs { t } => {
                self.int(5);
                self.num(*t)?;
            }
            Op::PolarToNurbs { t, zone_tol } => {
                self.int(6);
                self.num(*t)?;
                self.tol(*zone_tol)?;
            }
            Op::AngleToNurb { args } => {
                self.int(7);
                args.iter().try_for_each(|&x| self.num(x))?;
            }
            Op::IsDegenerate { tol } => {
                self.int(8);
                self.num(*tol)?;
            }
            Op::StartPlane { p, tol, snap } => {
                self.int(9);
                self.vec(*p)?;
                self.tol(*tol)?;
                self.flag(*snap);
            }
            Op::Drop {
                ivl,
                op,
                p,
                in_vec,
                tol,
                target,
                req,
            } => {
                if !(0..=9).contains(op) || !(0..=1).contains(req) {
                    return Err(bad("solver operation or request out of range"));
                }
                self.int(10);
                self.frac(*ivl)?;
                self.int(*op as usize);
                self.vec(*p)?;
                self.flag(in_vec.is_some());
                self.vec(in_vec.unwrap_or([0.0; 3]))?;
                self.tol(*tol)?;
                self.flag(target.is_some());
                self.num(target.unwrap_or(0.0))?;
                self.int(*req as usize);
            }
            Op::IntersectEllipse {
                ivl,
                other,
                other_ivl,
                tol,
            } => {
                self.int(11);
                self.frac(*ivl)?;
                self.flag(other.is_none());
                if let Some(o) = other {
                    self.ellipse(o, false)?;
                }
                self.frac(*other_ivl)?;
                self.tol(*tol)?;
            }
            Op::IntersectLine {
                ivl,
                line,
                line_ivl,
                tol,
            } => {
                self.int(12);
                self.frac(*ivl)?;
                self.line(line)?;
                self.frac(*line_ivl)?;
                self.tol(*tol)?;
            }
            Op::Transform {
                origin,
                x,
                y,
                scale,
            } => {
                nonzero(*x)?;
                nonzero(*y)?;
                self.int(13);
                self.vec(*origin)?;
                self.vec(*x)?;
                self.vec(*y)?;
                self.flag(scale.is_some());
                self.vec(scale.unwrap_or([1.0; 3]))?;
            }
            Op::To3d => self.int(14),
            Op::SetUp => self.int(15),
            Op::Eq { other } => {
                self.int(16);
                self.ellipse(other, false)?;
            }
            Op::SetStepInterval { domain } => {
                self.int(17);
                self.num(domain[0])?;
                self.num(domain[1])?;
            }
        }
        Ok(())
    }
}

/// The flat program of ellipse `a` and `ops`.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for inputs the shim must not pass upstream.
pub fn encode(a: &EllipseIn, ops: &[Op]) -> Result<Vec<f64>, FfiError> {
    if ops.len() > MAX_OPS {
        return Err(bad("too many ops"));
    }
    let mut e = Enc(Vec::new());
    e.ellipse(a, true)?;
    e.int(ops.len());
    for op in ops {
        e.op(op)?;
    }
    Ok(e.0)
}

/// Builds ellipse A, runs `ops` on it and returns the record (see the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for rejected inputs; [`FfiError::Status`] for a shim
/// failure.
pub fn run(a: &EllipseIn, ops: &[Op]) -> Result<Vec<f64>, FfiError> {
    let prog = encode(a, ops)?;
    let n_in = i64::try_from(prog.len()).map_err(|_| bad("program too long"))?;
    call_with_buffer(256, |buf: &mut [f64], len| {
        // SAFETY: `prog` is valid for `n_in` reads; `buf` for `buf.len()` writes; `len`
        // is a valid i64; nothing is retained.
        unsafe {
            sys::sc_curve_ellipse_p2_run(
                prog.as_ptr(),
                n_in,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
