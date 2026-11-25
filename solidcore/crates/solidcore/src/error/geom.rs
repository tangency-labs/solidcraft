//! Why a geometry utility fails, in solidcore's terms.
//!
//! Upstream: none. These describe failures of the geometry utilities (`crate::geom`:
//! vectors, matrices and transforms, boxes and extents, the point-set and plane
//! utilities, the one-dimensional and global solvers, grids, tolerance relations,
//! attributes and object caches). Each also records, for difftest only, the status the
//! C++ reports for it (crate-private `comparison_status`, read through
//! [`crate::SolidError::sm_status`]).
//!
//! | Rust                                  | Status difftest compares     |
//! |---------------------------------------|------------------------------|
//! | [`GeomFailure::CoincidentPoints`]     | `FALSE` (0)                  |
//! | [`GeomFailure::EdgeWithoutCurve`]     | `SM_ERR_NULL_POINTER`        |
//! | [`GeomFailure::NearlySingular`]       | `SM_ERR_WARNING`             |
//! | [`GeomFailure::ToleranceTooSmall`]    | `SM_ERR_INVALID_INPUT`       |
//! | every other cause                     | `SM_ERR`                     |
//! | [`GeomFailure::Defect`]               | the site's (`SM_ERR`)        |
//!
//! **The record** (#15034). A check that refuses builds a typed cause with its numbers; a
//! stage that fails through the kernel's own fault is a [`GeomFailure::Defect`] with its
//! reviewed reason, a defect ([`ErrorKind::StageFailed`]).
//!
//! **Classes.** A check of the caller's argument that is malformed (a shape, an index, a
//! dimension, an object not set up) is [`ErrorKind::InvalidInput`]; geometry that is
//! degenerate where the routine needs it not to be (no direction, parallel, collinear,
//! singular) is [`ErrorKind::Degenerate`]; a solve that stops unconverged with its numbers
//! is a defect ([`ErrorKind::StageFailed`]); a routine whose callee failed keeps that
//! error as its cause and takes its class from it ([`GeomFailure::CallFailed`]).
//!
//! Deviations: not upstream. The failure's `operation` path says which routine. No failure names an
//! entity: the sites they replace named none. The failures that are checks of a
//! caller's arguments stay [`crate::SolidError::InvalidInput`]. `expansion` does not use
//! these yet. `local_solve_nd` reads the singular system of
//! `Matrix::solve_linear_system` by its `SM_ERR` status.

use super::{
    ErrorKind, Failure, Reason, SM_ERR, SM_ERR_INVALID_INPUT, SM_ERR_NULL_POINTER, SM_ERR_WARNING,
};

/// Why a geometry utility fails.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum GeomFailure {
    /// An attribute line of a file being read has no line end: either it is longer than
    /// the reader allows, or the input ends first. [`ErrorKind::InvalidInput`].
    AttributeLineUnterminated {
        /// The bytes read without a line end.
        bytes: u32,
        /// Whether the input ended (else the line passed the reader's limit).
        end_of_input: bool,
    },
    /// A dimension that must be 2 or 3 is neither. [`ErrorKind::InvalidInput`].
    BadDimension {
        /// The dimension given.
        dimension: u64,
    },
    /// A box (a polar box or a pseudo box) is used before it is set up.
    /// [`ErrorKind::InvalidInput`].
    BoxNotSetUp,
    /// Brent's one-dimensional minimisation used all its iterations without meeting its
    /// tolerance: a defect ([`ErrorKind::StageFailed`]).
    BrentDidNotConverge {
        /// The iterations run.
        iterations: u64,
        /// The last estimate.
        at: f64,
        /// The bracket left, `[min, max]`.
        bracket: [f64; 2],
    },
    /// A routine this one calls failed; the failure keeps that routine's error as its
    /// cause and takes its class from it.
    CallFailed {
        /// The class of the callee's error, the cause.
        class: ErrorKind,
    },
    /// Two of the three points a circle should pass through coincide.
    CoincidentPoints,
    /// The points a circle should pass through are collinear: `measure` is below the
    /// kernel's zero for them. [`ErrorKind::Degenerate`].
    CollinearPoints {
        /// The middle point's distance from the chord, or the system's determinant.
        measure: f64,
    },
    /// A failure that is the kernel's fault, reviewed at its site, with the written reason:
    /// `why` names the step that failed and why that is the kernel's fault, not the
    /// geometry's. A defect ([`ErrorKind::StageFailed`]); status `SM_ERR` unless its site
    /// says otherwise.
    Defect {
        /// The step that failed, and why that is the kernel's fault.
        why: &'static str,
    },
    /// Two intervals to intersect (or to join) are disjoint.
    Disjoint {
        /// The first interval, `[min, max]`.
        a: [f64; 2],
        /// The second interval, `[min, max]`.
        b: [f64; 2],
    },
    /// The caller's edge has no curve. [`ErrorKind::InvalidInput`]; status
    /// `SM_ERR_NULL_POINTER` (upstream `NER`).
    EdgeWithoutCurve,
    /// The grid element given has no surface (it is not a surface element).
    /// [`ErrorKind::InvalidInput`].
    ElementWithoutSurface,
    /// An index is not below the number of entries. [`ErrorKind::InvalidInput`].
    IndexOutOfRange {
        /// The index given.
        index: usize,
        /// The number of entries.
        len: usize,
    },
    /// The Jacobi eigen-solver stopped after `sweeps` sweeps with the off-diagonal sum
    /// still not below the kernel's zero: a defect ([`ErrorKind::StageFailed`]).
    JacobiDidNotConverge {
        /// The sweeps run.
        sweeps: usize,
        /// The off-diagonal sum measured at the last sweep.
        off_diagonal: f64,
    },
    /// The linear system for the circle's centre is nearly singular: the three points
    /// lie (nearly) on one line.
    NearlySingular,
    /// The global solve's branch still needs subdividing after its local solve, but none
    /// of its trees has a node left to subdivide. [`ErrorKind::Failed`].
    NoNodeToSubdivide {
        /// The trees of the branch.
        trees: usize,
    },
    /// No vector perpendicular to the given one was found (it has no direction after
    /// unitizing, as an overflowed vector has). [`ErrorKind::Degenerate`].
    NoPerpendicular,
    /// The subdivision-tree node given has no owner object. [`ErrorKind::InvalidInput`].
    NodeWithoutOwner,
    /// A plane normal that must be a unit vector is longer than one by more than the
    /// kernel's zero. [`ErrorKind::InvalidInput`].
    NormalNotUnit {
        /// The normal's squared length.
        length_squared: f64,
    },
    /// A matrix the routine needs square is not. [`ErrorKind::InvalidInput`].
    NotSquare {
        /// The matrix's rows.
        rows: usize,
        /// The matrix's columns.
        cols: usize,
    },
    /// A matrix the routine needs symmetric is not ([`crate::geom::matrix::Matrix::is_symmetric`]).
    /// [`ErrorKind::InvalidInput`].
    NotSymmetric,
    /// Relating the pair would break the relation's cardinality.
    /// [`ErrorKind::InvalidInput`].
    PairBreaksCardinality,
    /// A pair to remove from a relation is not related: one of its keys is absent.
    /// [`ErrorKind::InvalidInput`].
    PairNotRelated,
    /// Two directions (lines, a line and a plane, or two derivatives) are parallel: the
    /// measure of their crossing is below the kernel's zero. [`ErrorKind::Degenerate`].
    Parallel {
        /// The cross product's squared length, or the dot product with the normal.
        measure: f64,
    },
    /// A point to normalise lies outside the extent (or off a degenerate side of it), so
    /// it has no normalised coordinates.
    PointOutsideExtent {
        /// The point's `u`.
        u: f64,
        /// The point's `v`.
        v: f64,
    },
    /// A point set that holds at most two points already holds `points`.
    /// [`ErrorKind::InvalidInput`].
    PointSetFull {
        /// The points the set holds.
        points: usize,
    },
    /// A line meets a sphere along a chord too short to tell apart from a touch: the
    /// discriminant is below the kernel's zero. [`ErrorKind::Degenerate`].
    ShortChord {
        /// The discriminant `r^2 - b^2`.
        discriminant: f64,
    },
    /// A linear system or matrix is singular: its pivot or determinant, `measure`, is
    /// below the kernel's zero for it. [`ErrorKind::Degenerate`].
    Singular {
        /// The pivot or determinant that fell below the kernel's zero.
        measure: f64,
    },
    /// A span to add to a periodic extent does not lie in `[0, period]`.
    /// [`ErrorKind::InvalidInput`].
    SpanOutsidePeriod {
        /// The span's start.
        from: f64,
        /// The span's end.
        to: f64,
        /// The extent's period.
        period: f64,
    },
    /// The tolerance to set is below the smallest tolerance the kernel tells apart from
    /// zero.
    ToleranceTooSmall {
        /// The tolerance that would be set.
        tolerance: f64,
    },
    /// The closest points of two lines are farther apart than the distance asked for.
    /// [`ErrorKind::NotWithinTolerance`].
    TooFarApart {
        /// The lines' distance.
        distance: f64,
        /// The distance asked for.
        limit: f64,
    },
    /// A matrix to decompose by singular values is empty or has fewer rows than columns.
    /// [`ErrorKind::InvalidInput`].
    TooFewRows {
        /// The matrix's rows.
        rows: usize,
        /// The matrix's columns.
        cols: usize,
    },
    /// The object type a cache is asked for is neither a B-rep nor a poly B-rep.
    /// [`ErrorKind::Unsupported`].
    UnsupportedTargetType {
        /// The object type given.
        target_type: i64,
    },
    /// The grid's voxel array cannot be allocated: `voxels` voxels are more than the
    /// `limit` the kernel allocates (or, below it, the machine refused the memory).
    /// [`ErrorKind::InvalidInput`].
    VoxelArrayTooLarge {
        /// The voxels asked for.
        voxels: usize,
        /// The most voxels the kernel allocates.
        limit: usize,
    },
    /// A curve's derivative of `order` has zero length, so its Frenet frame is not
    /// defined. [`ErrorKind::Degenerate`].
    ZeroDerivative {
        /// The derivative's order (3 for the torsion vector built from the first two).
        order: u8,
        /// The derivative's length (squared for the first derivative).
        length: f64,
    },
    /// A difference step for a Jacobian's column is zero: the parameter's interval leaves
    /// no room to step either way from `at`.
    ZeroDifferenceStep {
        /// The parameter's index.
        parameter: usize,
        /// The parameter's value.
        at: f64,
    },
    /// A line, ray or direction has no direction: its length is below the kernel's zero.
    /// [`ErrorKind::Degenerate`].
    ZeroDirection {
        /// The direction's length.
        length: f64,
    },
}

impl GeomFailure {
    /// The status the C++ reports for the failure (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            // upstream returns FALSE, not a status
            Self::CoincidentPoints => 0,
            Self::EdgeWithoutCurve => SM_ERR_NULL_POINTER,
            Self::NearlySingular => SM_ERR_WARNING,
            Self::ToleranceTooSmall { .. } => SM_ERR_INVALID_INPUT,
            _ => SM_ERR,
        }
    }

    /// The failure's stable [`ErrorKind`]: as each variant says; [`ErrorKind::Failed`] for
    /// the causes that name no other class.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::AttributeLineUnterminated { .. }
            | Self::BadDimension { .. }
            | Self::BoxNotSetUp
            | Self::EdgeWithoutCurve
            | Self::ElementWithoutSurface
            | Self::IndexOutOfRange { .. }
            | Self::NodeWithoutOwner
            | Self::NormalNotUnit { .. }
            | Self::NotSquare { .. }
            | Self::NotSymmetric
            | Self::PairBreaksCardinality
            | Self::PairNotRelated
            | Self::PointSetFull { .. }
            | Self::SpanOutsidePeriod { .. }
            | Self::TooFewRows { .. }
            | Self::VoxelArrayTooLarge { .. } => ErrorKind::InvalidInput,
            Self::BrentDidNotConverge { .. }
            | Self::Defect { .. }
            | Self::JacobiDidNotConverge { .. } => ErrorKind::StageFailed,
            Self::CallFailed { class } => *class,
            Self::CollinearPoints { .. }
            | Self::NoPerpendicular
            | Self::Parallel { .. }
            | Self::ShortChord { .. }
            | Self::Singular { .. }
            | Self::ZeroDerivative { .. }
            | Self::ZeroDirection { .. } => ErrorKind::Degenerate,
            Self::TooFarApart { .. } => ErrorKind::NotWithinTolerance,
            Self::UnsupportedTargetType { .. } => ErrorKind::Unsupported,
            _ => ErrorKind::of_status(self.comparison_status()),
        }
    }
}

impl core::fmt::Display for GeomFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::AttributeLineUnterminated {
                bytes,
                end_of_input: true,
            } => write!(
                f,
                "the input ends {bytes} bytes into an attribute line, before its line end"
            ),
            Self::AttributeLineUnterminated {
                bytes,
                end_of_input: false,
            } => write!(f, "an attribute line has no line end within {bytes} bytes"),
            Self::BadDimension { dimension } => {
                write!(f, "the dimension {dimension} is neither 2 nor 3")
            }
            Self::BoxNotSetUp => f.write_str("the box is not set up"),
            Self::BrentDidNotConverge {
                iterations,
                at,
                bracket,
            } => write!(
                f,
                "{}: the Brent solve did not converge after {iterations} iterations (at {at}, bracket [{}, {}])",
                ErrorKind::StageFailed,
                bracket[0],
                bracket[1]
            ),
            Self::CallFailed { .. } => f.write_str("a routine it calls failed"),
            Self::CoincidentPoints => f.write_str("two of the points coincide"),
            Self::CollinearPoints { measure } => {
                write!(f, "the points are collinear (measure {measure:e})")
            }
            Self::Defect { why } => write!(f, "{}: {why}", ErrorKind::StageFailed),
            Self::Disjoint { a, b } => write!(
                f,
                "the intervals [{}, {}] and [{}, {}] are disjoint",
                a[0], a[1], b[0], b[1]
            ),
            Self::EdgeWithoutCurve => f.write_str("the edge has no curve"),
            Self::ElementWithoutSurface => f.write_str("the grid element has no surface"),
            Self::IndexOutOfRange { index, len } => {
                write!(f, "the index {index} is not below the {len} entries")
            }
            Self::JacobiDidNotConverge {
                sweeps,
                off_diagonal,
            } => write!(
                f,
                "{}: the Jacobi sweeps did not converge after {sweeps} sweeps (off-diagonal sum {off_diagonal:e})",
                ErrorKind::StageFailed
            ),
            Self::NearlySingular => {
                f.write_str("the points lie (nearly) on one line: the system is nearly singular")
            }
            Self::NoNodeToSubdivide { trees } => write!(
                f,
                "none of the branch's {trees} trees has a node left to subdivide"
            ),
            Self::NoPerpendicular => f.write_str("no perpendicular vector found"),
            Self::NodeWithoutOwner => f.write_str("the tree node has no owner object"),
            Self::NormalNotUnit { length_squared } => write!(
                f,
                "the plane normal is not a unit vector (its squared length is {length_squared:e})"
            ),
            Self::NotSquare { rows, cols } => {
                write!(f, "the {rows}x{cols} matrix is not square")
            }
            Self::NotSymmetric => f.write_str("the matrix is not symmetric"),
            Self::PairBreaksCardinality => {
                f.write_str("relating the pair would break the relation's cardinality")
            }
            Self::PairNotRelated => f.write_str("the pair is not related"),
            Self::Parallel { measure } => write!(
                f,
                "the directions are parallel (their crossing measures {measure:e})"
            ),
            Self::PointOutsideExtent { u, v } => {
                write!(f, "the point ({u}, {v}) lies outside the extent")
            }
            Self::PointSetFull { points } => write!(
                f,
                "the point set already holds {points} points, and it holds at most two"
            ),
            Self::ShortChord { discriminant } => write!(
                f,
                "the line meets the sphere along a chord too short to resolve (discriminant {discriminant:e})"
            ),
            Self::Singular { measure } => write!(
                f,
                "the system is singular (its pivot or determinant is {measure:e})"
            ),
            Self::SpanOutsidePeriod { from, to, period } => write!(
                f,
                "the span from {from} to {to} does not lie in the period [0, {period}]"
            ),
            Self::ToleranceTooSmall { tolerance } => write!(
                f,
                "the tolerance {tolerance} is too small to tell apart from zero"
            ),
            Self::TooFarApart { distance, limit } => write!(
                f,
                "the lines are {distance:e} apart, farther than {limit:e}"
            ),
            Self::TooFewRows { rows, cols } => write!(
                f,
                "the {rows}x{cols} matrix is empty or has fewer rows than columns"
            ),
            Self::UnsupportedTargetType { target_type } => write!(
                f,
                "the object type {target_type} is neither a B-rep nor a poly B-rep"
            ),
            Self::VoxelArrayTooLarge { voxels, limit } => write!(
                f,
                "the grid's {voxels} voxels cannot be allocated (the kernel allocates at most {limit})"
            ),
            Self::ZeroDerivative { order, length } => write!(
                f,
                "the curve's derivative of order {order} has zero length ({length:e})"
            ),
            Self::ZeroDifferenceStep { parameter, at } => write!(
                f,
                "the difference step of parameter {parameter} at {at} is zero"
            ),
            Self::ZeroDirection { length } => {
                write!(f, "the direction has no length ({length:e})")
            }
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a geometry utility that fails with `failure`.
    #[must_use]
    #[track_caller]
    pub(crate) fn geom(operation: &'static str, failure: GeomFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// A map from a callee's error to the failure of `operation` that keeps it as its
    /// cause ([`GeomFailure::CallFailed`], status [`SM_ERR`], as upstream's
    /// `if (status) SER(SM_ERR)` after the call). Use as
    /// `.map_err(SolidError::geom_call(OP))`. [`crate::SolidError::Unported`],
    /// [`crate::SolidError::Cancelled`] and [`crate::SolidError::PreparationRequired`] pass
    /// through unchanged, as in `nurbs_routine`.
    pub(crate) fn geom_call(operation: &'static str) -> impl Fn(Self) -> Self {
        move |e| match e {
            Self::Unported { .. } | Self::Cancelled { .. } | Self::PreparationRequired { .. } => e,
            _ => Failure::new(operation, GeomFailure::CallFailed { class: e.kind() })
                .caused_by(e)
                .into_error(),
        }
    }

    /// [`crate::SolidError::Failed`] for a reviewed defect of a geometry utility whose
    /// upstream site reports `status` (`SM_ERR_NULL_POINTER` for a `NER`).
    #[must_use]
    #[track_caller]
    pub(crate) fn geom_defect_with_status(
        operation: &'static str,
        why: &'static str,
        status: i32,
    ) -> Self {
        Failure::new(operation, GeomFailure::Defect { why })
            .with_status(status)
            .into_error()
    }

    /// The geometry-utility failure behind this error, for a
    /// [`crate::SolidError::Failed`] with a [`Reason::Geom`].
    #[must_use]
    pub fn geom_failure(&self) -> Option<&GeomFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Geom(g) => Some(g),
                _ => None,
            },
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SolidError;

    #[test]
    fn geom_failures_keep_the_status_and_kind_of_the_upstream_they_replace() {
        let cases = [
            (
                GeomFailure::Disjoint {
                    a: [0.0, 1.0],
                    b: [2.0, 3.0],
                },
                SM_ERR,
            ),
            (GeomFailure::NearlySingular, SM_ERR_WARNING),
            (GeomFailure::CoincidentPoints, 0),
            (
                GeomFailure::ToleranceTooSmall { tolerance: 0.0 },
                SM_ERR_INVALID_INPUT,
            ),
        ];
        for (failure, code) in cases {
            let typed = SolidError::geom("geom::op", failure);
            let before = SolidError::failed_routine("geom::op", code);
            assert_eq!(typed.sm_status(), before.sm_status(), "{failure:?}");
            assert_eq!(typed.failed_routine_status(), Some(code), "{failure:?}");
            assert_eq!(SolidError::failed_routine_status(&typed), Some(code));
            assert_eq!(typed.kind(), before.kind(), "{failure:?}");
            assert_eq!(typed.operation(), before.operation(), "{failure:?}");
            assert_eq!(typed.entity(), before.entity(), "{failure:?}");
            assert_eq!(typed.is_port_failure(), before.is_port_failure());
            assert_eq!(typed.is_status_answer(), before.is_status_answer());
            assert_eq!(
                typed.is_unported_or_cancelled(),
                before.is_unported_or_cancelled()
            );
            assert_eq!(typed.geom_failure(), Some(&failure));
        }
        assert_eq!(
            SolidError::geom(
                "geom::op",
                GeomFailure::ToleranceTooSmall { tolerance: 0.0 }
            )
            .kind(),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            SolidError::invalid_input("geom::op", "x").geom_failure(),
            None
        );
    }

    /// A typed cause keeps the status of the bare status it replaced.
    #[test]
    fn a_typed_cause_keeps_its_status() {
        for cause in [
            GeomFailure::Disjoint {
                a: [0.0, 1.0],
                b: [2.0, 3.0],
            },
            GeomFailure::PointOutsideExtent { u: 2.0, v: 0.5 },
            GeomFailure::ZeroDifferenceStep {
                parameter: 0,
                at: 0.5,
            },
        ] {
            let e = SolidError::geom("geom::op", cause);
            assert_eq!(e.failed_routine_status(), Some(SM_ERR), "{cause:?}");
            assert_eq!(e.kind(), ErrorKind::Failed, "{cause:?}");
            assert!(!e.is_defect(), "{cause:?}");
        }
        assert_eq!(
            SolidError::geom(
                "geom::op",
                GeomFailure::Disjoint {
                    a: [0.0, 1.0],
                    b: [2.0, 3.0]
                }
            )
            .to_string(),
            "geom::op: the intervals [0, 1] and [2, 3] are disjoint"
        );
    }

    /// A reviewed defect is of class 127 with its site's status, and says why.
    #[test]
    fn a_reviewed_defect_is_class_127_with_its_reason() {
        let why = "the bracketing step lost its sign change, which the kernel's bracket guarantees";
        let defect = GeomFailure::Defect { why };
        super::super::tests::assert_defect(
            &SolidError::geom("geom::op", defect),
            &SolidError::failed_routine("geom::op", SM_ERR),
            "geom::op",
            SM_ERR,
            &format!("geom::op: stage_failed: {why}"),
        );
        assert_eq!(
            SolidError::geom("geom::op", defect).geom_failure(),
            Some(&defect)
        );
    }
}
