//! Why a NURBS routine fails, in solidcore's terms.
//!
//! Upstream: `source/SMLib/inc/NL_DataStruct.h` (the `NL_*_ERR` numbers), none otherwise.
//! These describe the failures of the NURBS routines (`crate::nurbs`: knots, bases,
//! curve and surface fitting, knot insertion, degree elevation, ...). NLib routines
//! report failure by returning an error flag and storing a number in the process-wide
//! `NL_ERROR` record; solidcore returns a [`NurbsFailure`] instead. Each also records,
//! for difftest only, the `NL_*_ERR` number the C++ stores for it (crate-private
//! `nlib_number`, read through [`nlib_number`]) and the status its `Sm*` callers
//! report for it (crate-private `comparison_status`, read through
//! [`crate::SolidError::sm_status`]: `SM_ERR`, never the `NL_*_ERR` number).
//!
//! | Rust                                  | Compared number                 |
//! |---------------------------------------|---------------------------------|
//! | [`NurbsFailure::CurveDefinition`]     | `NL_CUR_ERR` (1)                |
//! | [`NurbsFailure::SurfaceDefinition`]   | `NL_SUR_ERR` (2)                |
//! | [`NurbsFailure::Degree`], [`NurbsFailure::DegreeTooHigh`] | `NL_DEG_ERR` (3) |
//! | [`NurbsFailure::Knots`], [`NurbsFailure::KnotsDecrease`]  | `NL_KNT_ERR` (4) |
//! | [`NurbsFailure::Weight`], [`NurbsFailure::WeightOutOfRange`] | `NL_WEI_ERR` (5) |
//! | [`NurbsFailure::Parameter`], [`NurbsFailure::ParameterOutside`] | `NL_PAR_ERR` (6) |
//! | [`NurbsFailure::Index`], [`NurbsFailure::IndexOutOfRange`] | `NL_IND_ERR` (7) |
//! | [`NurbsFailure::ZeroDerivative`]      | `NL_DER_ERR` (8)                |
//! | [`NurbsFailure::Storage`]             | `NL_STO_ERR` (9)                |
//! | [`NurbsFailure::Numerical`]           | `NL_NUM_ERR` (11)               |
//! | [`NurbsFailure::NotConverged`]        | `NL_CON_ERR` (12)               |
//! | [`NurbsFailure::CallFailed`]          | `NL_CAL_ERR` (13)               |
//! | [`NurbsFailure::Input`]               | `NL_INP_ERR` (14)               |
//! | [`NurbsFailure::Memory`]              | `NL_MEM_ERR` (15)               |
//! | [`NurbsFailure::Singular`]            | `NL_SEQ_ERR` (16)               |
//! | [`NurbsFailure::Geometry`]            | `NL_GEO_ERR` (17)               |
//! | [`NurbsFailure::DerivativeOrder`], [`NurbsFailure::NoDerivativeRows`] | `NL_MXD_ERR` (20) |
//!
//! Deviations: not upstream. A failure of a routine that has the data (a degree, an
//! index, a parameter and its domain) carries it; the others are unit variants, and the
//! failure's `operation` path says which routine. `NL_MAT_ERR`, `NL_TOL_ERR`,
//! `NL_KML_ERR`, `NL_CUR_REV`, `NL_FIL_ERR` and `NL_SCN_ERR` have no failure here: no
//! routine of the port stores them.

use super::{ErrorKind, Failure, Reason, SM_ERR};

/// Why a NURBS routine fails.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum NurbsFailure {
    /// The curve's control point, knot and degree counts do not agree (`n + p + 1 != m`).
    CurveDefinition,
    /// The surface's control net, knot and degree counts do not agree in a direction.
    SurfaceDefinition,
    /// The degree is not allowed here (too low, too high, or not one the routine
    /// handles).
    Degree,
    /// The degree `given` is above the largest `max` the routines handle.
    DegreeTooHigh {
        /// The degree asked for.
        given: usize,
        /// The largest degree.
        max: usize,
    },
    /// The knot vector is not valid here.
    Knots,
    /// The knot vector decreases at knot `index` (it is smaller than the knot before it).
    KnotsDecrease {
        /// The index of the knot that is smaller than its predecessor.
        index: usize,
    },
    /// A weight is not valid here.
    Weight,
    /// The weight of control point `index` is `weight`, outside the range `[min, max]`
    /// the routines accept.
    WeightOutOfRange {
        /// The control point.
        index: usize,
        /// Its weight.
        weight: f64,
        /// The smallest weight accepted.
        min: f64,
        /// The largest weight accepted.
        max: f64,
    },
    /// A parameter is outside the domain.
    Parameter,
    /// The parameter is outside the curve or surface's domain `[lo, hi]`.
    ParameterOutside {
        /// The parameter.
        parameter: f64,
        /// The start of the domain.
        lo: f64,
        /// The end of the domain.
        hi: f64,
    },
    /// An index is out of range.
    Index,
    /// The index `index` is not below the length `len`.
    IndexOutOfRange {
        /// The index.
        index: usize,
        /// The number of items.
        len: usize,
    },
    /// A derivative is zero where a direction is needed.
    ZeroDerivative,
    /// A buffer supplied by the caller is too small for the result.
    Storage,
    /// A numerical failure (a division by zero, an overflow, a value that is not finite).
    Numerical,
    /// An iteration did not converge.
    NotConverged,
    /// A routine it calls rejected its arguments, or the direction is not one the
    /// routine handles.
    CallFailed,
    /// The input data is not valid.
    Input,
    /// The memory for the result cannot be allocated.
    Memory,
    /// The system of equations has no unique solution (it is singular).
    Singular,
    /// The geometry does not allow the operation (a degenerate or non-planar input).
    Geometry,
    /// The output has no row for even the point itself, so no derivative order can be
    /// evaluated into it.
    NoDerivativeRows,
    /// The derivative order `given` is above the largest `max` the routines evaluate.
    DerivativeOrder {
        /// The order asked for.
        given: usize,
        /// The largest order.
        max: usize,
    },
}

impl NurbsFailure {
    /// The status the C++ reports for any of the failures (a comparison detail): its `Sm*`
    /// callers surface an NLib failure as `SER(SM_ERR)`.
    pub(crate) const COMPARISON_STATUS: i32 = SM_ERR;

    /// The `NL_*_ERR` number the C++ stores for the failure (a comparison detail).
    pub(crate) const fn nlib_number(&self) -> i32 {
        match self {
            Self::CurveDefinition => 1,
            Self::SurfaceDefinition => 2,
            Self::Degree | Self::DegreeTooHigh { .. } => 3,
            Self::Knots | Self::KnotsDecrease { .. } => 4,
            Self::Weight | Self::WeightOutOfRange { .. } => 5,
            Self::Parameter | Self::ParameterOutside { .. } => 6,
            Self::Index | Self::IndexOutOfRange { .. } => 7,
            Self::ZeroDerivative => 8,
            Self::Storage => 9,
            Self::Numerical => 11,
            Self::NotConverged => 12,
            Self::CallFailed => 13,
            Self::Input => 14,
            Self::Memory => 15,
            Self::Singular => 16,
            Self::Geometry => 17,
            Self::DerivativeOrder { .. } | Self::NoDerivativeRows => 20,
        }
    }

    /// The failure's stable [`ErrorKind`].
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::NotConverged => ErrorKind::NotConverged,
            Self::Parameter | Self::ParameterOutside { .. } => ErrorKind::OutsideDomain,
            _ => ErrorKind::Failed,
        }
    }
}

impl core::fmt::Display for NurbsFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::CurveDefinition => {
                f.write_str("the curve's control point, knot and degree counts do not agree")
            }
            Self::SurfaceDefinition => {
                f.write_str("the surface's control net, knot and degree counts do not agree")
            }
            Self::Degree => f.write_str("the degree is not allowed here"),
            Self::DegreeTooHigh { given, max } => {
                write!(f, "degree {given} is above the largest degree, {max}")
            }
            Self::Knots => f.write_str("the knot vector is not valid here"),
            Self::KnotsDecrease { index } => {
                write!(f, "the knot vector decreases at knot {index}")
            }
            Self::Weight => f.write_str("a weight is not valid here"),
            Self::WeightOutOfRange {
                index,
                weight,
                min,
                max,
            } => write!(
                f,
                "the weight of control point {index} is {weight}, outside [{min}, {max}]"
            ),
            Self::Parameter => f.write_str("a parameter is outside the domain"),
            Self::ParameterOutside { parameter, lo, hi } => {
                write!(
                    f,
                    "parameter {parameter} is outside the domain [{lo}, {hi}]"
                )
            }
            Self::Index => f.write_str("an index is out of range"),
            Self::IndexOutOfRange { index, len } => {
                write!(f, "index {index} is out of range for {len} items")
            }
            Self::ZeroDerivative => f.write_str("a derivative is zero where a direction is needed"),
            Self::Storage => f.write_str("a buffer supplied by the caller is too small"),
            Self::Numerical => f.write_str("a numerical failure (a division by zero or overflow)"),
            Self::NotConverged => f.write_str("the iteration did not converge"),
            Self::CallFailed => f.write_str(
                "a routine it calls rejected its arguments, or the direction is not handled",
            ),
            Self::Input => f.write_str("the input data is not valid"),
            Self::Memory => f.write_str("the memory for the result cannot be allocated"),
            Self::Singular => f.write_str("the system of equations is singular"),
            Self::Geometry => f.write_str("the geometry does not allow the operation"),
            Self::NoDerivativeRows => f.write_str("the output has no row for the point itself"),
            Self::DerivativeOrder { given, max } => {
                write!(
                    f,
                    "derivative order {given} is above the largest order, {max}"
                )
            }
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a NURBS routine that fails with `failure`; the
    /// status its `Sm*` callers report for it is `SM_ERR` (NLib's `N_ErrSet` followed by
    /// an error return).
    #[must_use]
    pub(crate) fn nurbs(operation: &'static str, failure: NurbsFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// The NURBS failure behind this error, for a [`crate::SolidError::Failed`] with a
    /// [`Reason::Nurbs`].
    #[must_use]
    pub fn nurbs_failure(&self) -> Option<&NurbsFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Nurbs(n) => Some(n),
                _ => None,
            },
            _ => None,
        }
    }

    /// True for the failure of a NURBS routine ([`Reason::Nurbs`]). The callers that
    /// ignore an NLib failure's flag, as upstream does, drop it by this test.
    #[must_use]
    pub(crate) fn is_nurbs_failure(&self) -> bool {
        self.nurbs_failure().is_some() || matches!(self, Self::Nlib { .. })
    }
}

/// The `NL_*_ERR` number a NURBS failure stands for, when `error` is one.
///
/// For difftest's comparison only: it is removed with `sm_status` in the last PR of the
/// typed-errors work. Callers match [`crate::SolidError::kind`] and the [`Reason`].
#[must_use]
pub fn nlib_number(error: &crate::SolidError) -> Option<i32> {
    match error {
        crate::SolidError::Failed(f) => match &f.reason {
            Reason::Nurbs(n) => Some(n.nlib_number()),
            _ => None,
        },
        crate::SolidError::Nlib { code, .. } => Some(code.code()),
        _ => None,
    }
}
