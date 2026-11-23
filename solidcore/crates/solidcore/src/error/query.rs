//! Why a query fails, in solidcore's terms.
//!
//! Upstream: none. These describe failures of the query subsystem (`crate::query`: edge
//! and face queries, point classification on a face, contact and distance bounds,
//! continuity, normals, the face-surface intersection, the poly B-rep and its copy,
//! properties and tessellation, and ray casting on a B-rep). Each also records, for
//! difftest only, the status the C++ reports for it (crate-private `comparison_status`,
//! read through [`crate::SolidError::sm_status`]).
//!
//! | Rust                           | Status difftest compares |
//! |--------------------------------|--------------------------|
//! | [`QueryFailure::Failed`]       | `SM_ERR`                 |
//! | [`QueryFailure::Missing`]      | `SM_ERR_NULL_POINTER`    |
//! | [`QueryFailure::InvalidValue`] | `SM_ERR_INVALID_INPUT`   |
//! | [`QueryFailure::Defect`]       | `SM_ERR` (or the site's, through `with_status`) |
//! | the named causes, from [`QueryFailure::BoundOverflows`] on | the status of the phrase each replaced (`comparison_status`), or the site's through `with_status` |
//!
//! [`QueryFailure::Defect`] is a failure reviewed at its site as the kernel's fault, with its
//! reason: a defect ([`ErrorKind::StageFailed`], class 127). The phrase causes are sites
//! not yet converted (the conversion guide: `docs/failure-codes-and-reproduction.md` §6).
//!
//! Deviations: not upstream. A failure names what went wrong (a named cause, or a phrase
//! where a site is not yet converted); the failure's `operation` path says which routine.
//! A failure names an entity only where the site it replaces did (the point
//! classification's seam failure, at its face). A check of a caller's argument or body
//! that upstream answers with a status is a named cause of kind
//! [`ErrorKind::InvalidInput`] that keeps the status (for example
//! [`QueryFailure::FaceWithoutSurface`]); the checks that report no status stay
//! [`crate::SolidError::InvalidInput`].

use super::{ErrorKind, Failure, Reason, SM_ERR, SM_ERR_INVALID_INPUT, SM_ERR_NULL_POINTER};

/// Why a query fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum QueryFailure {
    /// The query could not continue, as the phrase says (for example "the loop does not
    /// close").
    Failed(&'static str),
    /// Something the query needs is missing, as the phrase says (for example "the edge's
    /// curve").
    Missing(&'static str),
    /// A value the query computes with has no defined result, as the phrase says (for
    /// example "a tangent of zero length has no direction"). Upstream returns
    /// `SM_ERR_INVALID_INPUT` as a status here, so its kind is
    /// [`ErrorKind::InvalidInput`].
    InvalidValue(&'static str),
    /// A failure that is the kernel's fault, reviewed at its site, with the written reason:
    /// `why` names the step that failed and why that is the kernel's fault, not the
    /// geometry's. A defect ([`ErrorKind::StageFailed`]); status `SM_ERR` unless its site
    /// says otherwise.
    Defect {
        /// The step that failed, and why that is the kernel's fault.
        why: &'static str,
    },
    /// A bound or size the query computes overflows the floating range, though every number
    /// given is finite: the inputs' size, or how far apart they are, reaches past it. Class
    /// `failed`.
    BoundOverflows,
    /// No station of the first curve dropped onto the other curve or the surface, so no
    /// deviation was measured. Class `failed`.
    NoStationDropped,
    /// No patch of the surface overlaps the UV box given: the box is within the rounding of
    /// the patches' knot range, but past every patch. [`ErrorKind::InvalidInput`].
    NoPatchInBox,
    /// A face of the caller's body has no surface. [`ErrorKind::InvalidInput`].
    FaceWithoutSurface,
    /// The surface the query reads (a B-spline surface, or the B-spline base of an analytic
    /// one) has no NURBS net: none was made for it, or it could not be made (a NURB that
    /// cannot be made stays missing), so the query has no net to read.
    /// [`ErrorKind::Unsupported`].
    SurfaceWithoutNurbs,
}

impl QueryFailure {
    /// The status the C++ reports for the failure (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::Failed(_)
            | Self::Defect { .. }
            | Self::BoundOverflows
            | Self::NoStationDropped
            | Self::NoPatchInBox
            | Self::SurfaceWithoutNurbs => SM_ERR,
            Self::Missing(_) | Self::FaceWithoutSurface => SM_ERR_NULL_POINTER,
            Self::InvalidValue(_) => SM_ERR_INVALID_INPUT,
        }
    }

    /// The failure's stable [`ErrorKind`]: [`ErrorKind::InvalidInput`] for
    /// [`QueryFailure::InvalidValue`] and for a check of the caller's argument or body,
    /// [`ErrorKind::Unsupported`] for a surface or object the routine does not take,
    /// [`ErrorKind::Missing`] for [`QueryFailure::Missing`], otherwise the kind of the status
    /// it replaced.
    /// [`QueryFailure::Defect`] is [`ErrorKind::StageFailed`], a defect.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::Defect { .. } => ErrorKind::StageFailed,
            Self::SurfaceWithoutNurbs => ErrorKind::Unsupported,
            Self::NoPatchInBox | Self::FaceWithoutSurface => ErrorKind::InvalidInput,
            _ => ErrorKind::of_status(self.comparison_status()),
        }
    }
}

impl core::fmt::Display for QueryFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Failed(what) | Self::InvalidValue(what) => f.write_str(what),
            Self::Missing(what) => write!(f, "{what} is missing"),
            Self::Defect { why } => write!(f, "{}: {why}", ErrorKind::StageFailed),
            Self::BoundOverflows => f.write_str("the bound overflows the floating range"),
            Self::NoStationDropped => f.write_str("no station dropped onto the other"),
            Self::NoPatchInBox => f.write_str("no patch of the surface overlaps the UV box"),
            Self::FaceWithoutSurface => f.write_str("the face has no surface"),
            Self::SurfaceWithoutNurbs => f.write_str("the surface has no NURBS net"),
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a query that fails with `failure`.
    #[must_use]
    pub(crate) fn query(operation: &'static str, failure: QueryFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// [`crate::SolidError::Failed`] for a query that could not continue, as the phrase
    /// says, with status [`SM_ERR`] (upstream `return SE(SM_ERR)`).
    #[must_use]
    pub(crate) fn query_failed(operation: &'static str, what: &'static str) -> Self {
        Self::query(operation, QueryFailure::Failed(what))
    }

    /// [`crate::SolidError::Failed`] for something a query needs that is not there, as the
    /// phrase says, with status [`SM_ERR_NULL_POINTER`] (upstream `NER(ptr)` on a `NULL`
    /// pointer).
    #[must_use]
    pub(crate) fn query_missing(operation: &'static str, what: &'static str) -> Self {
        Self::query(operation, QueryFailure::Missing(what))
    }

    /// [`crate::SolidError::Failed`] for a value a query computes with that has no defined
    /// result, as the phrase says, with status [`SM_ERR_INVALID_INPUT`] (upstream
    /// `return(SM_ERR_INVALID_INPUT)`, or `SER` of a vector operation that returns it).
    #[must_use]
    pub(crate) fn query_invalid_value(operation: &'static str, what: &'static str) -> Self {
        Self::query(operation, QueryFailure::InvalidValue(what))
    }

    /// The query failure behind this error, for a [`crate::SolidError::Failed`] with a
    /// [`Reason::Query`].
    #[must_use]
    pub fn query_failure(&self) -> Option<&QueryFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Query(q) => Some(q),
                _ => None,
            },
            _ => None,
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::SolidError;

    #[test]
    fn query_failures_keep_the_status_and_kind_of_the_upstream_they_replace() {
        let cases = [
            (QueryFailure::Failed("x"), SM_ERR),
            (QueryFailure::Missing("x"), SM_ERR_NULL_POINTER),
            (QueryFailure::InvalidValue("x"), SM_ERR_INVALID_INPUT),
        ];
        for (failure, code) in cases {
            let typed = SolidError::query("query::op", failure);
            let before = SolidError::Upstream {
                operation: "query::op",
                code,
                entity: None,
            };
            assert_eq!(typed.sm_status(), before.sm_status(), "{failure:?}");
            assert_eq!(typed.upstream_status(), Some(code), "{failure:?}");
            assert_eq!(super::super::upstream_status(&typed), Some(code));
            assert_eq!(typed.kind(), before.kind(), "{failure:?}");
            assert_eq!(typed.operation(), before.operation(), "{failure:?}");
            assert_eq!(typed.entity(), before.entity(), "{failure:?}");
            assert_eq!(typed.is_port_failure(), before.is_port_failure());
            assert_eq!(typed.is_status_answer(), before.is_status_answer());
            assert_eq!(
                typed.is_unported_or_cancelled(),
                before.is_unported_or_cancelled()
            );
            assert_eq!(typed.query_failure(), Some(&failure));
        }
        assert_eq!(
            SolidError::query_invalid_value("query::op", "x"),
            SolidError::query("query::op", QueryFailure::InvalidValue("x"))
        );
        assert_eq!(
            SolidError::query_missing("query::op", "the edge's curve").to_string(),
            "query::op: the edge's curve is missing"
        );
        assert_eq!(
            SolidError::query_failed("query::op", "the loop does not close").to_string(),
            "query::op: the loop does not close"
        );
        assert_eq!(
            SolidError::query_invalid_value("query::op", "x").kind(),
            ErrorKind::InvalidInput
        );
        assert_eq!(SolidError::sm_err("query::op").query_failure(), None);
    }

    /// Each named cause reports the status of the phrase it replaced, and its kind.
    #[test]
    fn the_named_query_causes_keep_the_status_they_replaced() {
        use ErrorKind as K;
        use QueryFailure as F;
        let cases = [
            (F::BoundOverflows, SM_ERR, K::Failed),
            (F::NoStationDropped, SM_ERR, K::Failed),
            (F::NoPatchInBox, SM_ERR, K::InvalidInput),
            (F::FaceWithoutSurface, SM_ERR_NULL_POINTER, K::InvalidInput),
            (F::SurfaceWithoutNurbs, SM_ERR, K::Unsupported),
        ];
        for (failure, code, kind) in cases {
            let e = SolidError::query("query::op", failure);
            assert_eq!(e.sm_status(), Some(code), "{failure:?}");
            assert_eq!(super::super::upstream_status(&e), Some(code), "{failure:?}");
            assert_eq!(e.kind(), kind, "{failure:?}");
            assert_eq!(e.query_failure(), Some(&failure));
            assert!(!e.is_defect(), "{failure:?}");
        }
        assert_eq!(
            SolidError::query("query::op", F::FaceWithoutSurface).to_string(),
            "query::op: the face has no surface"
        );
    }

    /// The query routines describe their failures: their library code builds no bare
    /// `Upstream` status, except the files listed (with the bare sites each may still hold)
    /// whose conversion waits on other open work.
    #[test]
    fn the_query_routines_build_no_bare_statuses() {
        assert_eq!(bare_statuses_in("query", 20), Vec::<(String, usize)>::new());
    }

    /// The files under `src/<area>` (and `src/<area>.rs`) whose library code builds bare
    /// statuses, with the number each builds, sorted by path; at least `least` files are
    /// read. Kept for the ratchets that call it; the scan is [`super::super::bare_scan`].
    pub(in crate::error) fn bare_statuses_in(area: &str, least: usize) -> Vec<(String, usize)> {
        super::super::bare_scan::bare_statuses_in(area, least)
    }

    /// A reviewed defect is of class 127 with the status of the phrase cause it replaces,
    /// and says why.
    #[test]
    fn a_reviewed_defect_is_class_127_with_its_reason() {
        let why = "the face's cache was built just before and has no loops, which a built cache always has";
        let defect = QueryFailure::Defect { why };
        let op = "query::op";
        super::super::tests::assert_defect(
            &SolidError::query(op, defect),
            &SolidError::query(op, QueryFailure::Failed("x")),
            op,
            SM_ERR,
            &format!("{op}: stage_failed: {why}"),
        );
        let missing = Failure::new(op, defect)
            .with_status(SM_ERR_NULL_POINTER)
            .into_error();
        super::super::tests::assert_defect(
            &missing,
            &SolidError::query(op, QueryFailure::Missing("x")),
            op,
            SM_ERR_NULL_POINTER,
            &format!("{op}: stage_failed: {why}"),
        );
        assert_eq!(missing.query_failure(), Some(&defect));
    }

    /// The sites that build a query phrase cause only fall:
    /// per file, in the library code of `solidcore` and `solidcore-ops`, as
    /// [`super::super::site_scan`] counts them. A PR that converts sites lowers its
    /// files' counts here, and a file at 0 loses its line (the conversion guide:
    /// `docs/failure-codes-and-reproduction.md` §6).
    #[test]
    fn the_query_phrase_cause_sites_only_shrink() {
        use super::super::site_scan::{assert_pinned, crate_sites, Builder, Scan, Wrappers};
        const PHRASES: Scan = Scan {
            builders: &[
                Builder::Variant("QueryFailure", "Failed"),
                Builder::Variant("QueryFailure", "Missing"),
                Builder::Variant("QueryFailure", "InvalidValue"),
                Builder::Method("query_failed", Some(1)),
                Builder::Method("query_missing", Some(1)),
                Builder::Method("query_invalid_value", Some(1)),
            ],
            wrappers: Wrappers::Phrase,
        };
        #[rustfmt::skip] // one file per line, so conversions in parallel merge cleanly
        const PINNED: &[(&str, usize)] = &[
            ("solidcore/src/query/api_queries.rs", 36),
            ("solidcore/src/query/brep.rs", 2),
            ("solidcore/src/query/brep/ray.rs", 2),
            ("solidcore/src/query/brep/ray/live.rs", 4),
            ("solidcore/src/query/edge.rs", 10),
            ("solidcore/src/query/face/p1.rs", 2),
            ("solidcore/src/query/face/p2.rs", 19),
            ("solidcore/src/query/face/p3.rs", 2),
            ("solidcore/src/query/face/p4.rs", 2),
            ("solidcore/src/query/poly.rs", 13),
            ("solidcore/src/query/poly/copy.rs", 11),
            ("solidcore/src/query/poly_properties.rs", 2),
            ("solidcore/src/query/tess.rs", 6),
        ];
        assert_pinned("query phrase-cause sites", &crate_sites(&PHRASES), PINNED);
    }
}
