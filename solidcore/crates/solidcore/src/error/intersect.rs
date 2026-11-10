//! Why an intersection or classification fails, in solidcore's terms.
//!
//! Upstream: none. These describe failures of the intersection subsystem
//! (`crate::intersect`: curve and point classification, ray tracing, dropping points and
//! curves onto surfaces, surface tracing, silhouettes, the advanced surface intersector
//! and the intersection API). Each also records, for difftest only, the status the C++
//! reports for it (crate-private `comparison_status`, read through
//! [`crate::SolidError::sm_status`]).
//!
//! | Rust                                | Status difftest compares   |
//! |-------------------------------------|----------------------------|
//! | [`IntersectFailure::Failed`]        | `SM_ERR`                   |
//! | [`IntersectFailure::Missing`]       | `SM_ERR_NULL_POINTER`      |
//! | [`IntersectFailure::OutsideDomain`] | `SM_ERR_OUTSIDE_OF_DOMAIN` |
//! | [`IntersectFailure::Inconsistent`]  | `SM_ERR_ASSERT_FAILURE`    |
//!
//! Deviations: not upstream. A failure says in a phrase what went wrong; the failure's
//! `operation` path says which routine. The handles these routines work on are the
//! classification's or intersector's own, not ids of the caller's B-rep, so no failure
//! names an entity. The failures that are checks of a caller's handles stay
//! [`crate::SolidError::InvalidInput`]. `surface_intersector` and `topology_solver` do
//! not use these yet.

use super::{
    ErrorKind, Failure, Reason, SM_ERR, SM_ERR_ASSERT_FAILURE, SM_ERR_NULL_POINTER,
    SM_ERR_OUTSIDE_OF_DOMAIN,
};

/// Why an intersection or classification fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum IntersectFailure {
    /// The routine could not continue, as the phrase says (for example "the ray hits no
    /// face").
    Failed(&'static str),
    /// Something the routine needs is missing, as the phrase says (for example "the
    /// edge's curve").
    Missing(&'static str),
    /// A point or parameter falls outside the domain it must lie in, as the phrase says
    /// (for example "the span's end does not snap onto the seam").
    OutsideDomain(&'static str),
    /// A consistency check of the routine's own data did not hold, as the phrase says.
    Inconsistent(&'static str),
}

impl IntersectFailure {
    /// The status the C++ reports for the failure (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::Failed(_) => SM_ERR,
            Self::Missing(_) => SM_ERR_NULL_POINTER,
            Self::OutsideDomain(_) => SM_ERR_OUTSIDE_OF_DOMAIN,
            Self::Inconsistent(_) => SM_ERR_ASSERT_FAILURE,
        }
    }

    /// The failure's stable [`ErrorKind`]: [`ErrorKind::Missing`] for
    /// [`IntersectFailure::Missing`], [`ErrorKind::OutsideDomain`] for
    /// [`IntersectFailure::OutsideDomain`], [`ErrorKind::Failed`] otherwise.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        ErrorKind::of_status(self.comparison_status())
    }
}

impl core::fmt::Display for IntersectFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Failed(what) | Self::OutsideDomain(what) => f.write_str(what),
            Self::Missing(what) => write!(f, "{what} is missing"),
            Self::Inconsistent(what) => write!(f, "a consistency check failed: {what}"),
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for an intersection or classification routine that
    /// fails with `failure`.
    #[must_use]
    pub(crate) fn intersect(operation: &'static str, failure: IntersectFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// [`crate::SolidError::Failed`] for an intersection or classification routine that
    /// could not continue, as the phrase says, with status [`SM_ERR`] (upstream
    /// `return SE(SM_ERR)`).
    #[must_use]
    pub(crate) fn intersect_failed(operation: &'static str, what: &'static str) -> Self {
        Self::intersect(operation, IntersectFailure::Failed(what))
    }

    /// [`crate::SolidError::Failed`] for something an intersection or classification
    /// routine needs that is not there, as the phrase says, with status
    /// [`SM_ERR_NULL_POINTER`] (upstream `NER(ptr)` on a `NULL` pointer).
    #[must_use]
    pub(crate) fn intersect_missing(operation: &'static str, what: &'static str) -> Self {
        Self::intersect(operation, IntersectFailure::Missing(what))
    }

    /// The intersection failure behind this error, for a [`crate::SolidError::Failed`]
    /// with a [`Reason::Intersect`].
    #[must_use]
    pub fn intersect_failure(&self) -> Option<&IntersectFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Intersect(i) => Some(i),
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
    fn intersect_failures_keep_the_status_and_kind_of_the_upstream_they_replace() {
        let cases = [
            (IntersectFailure::Failed("x"), SM_ERR),
            (IntersectFailure::Missing("x"), SM_ERR_NULL_POINTER),
            (
                IntersectFailure::OutsideDomain("x"),
                SM_ERR_OUTSIDE_OF_DOMAIN,
            ),
            (IntersectFailure::Inconsistent("x"), SM_ERR_ASSERT_FAILURE),
        ];
        for (failure, code) in cases {
            let typed = SolidError::intersect("intersect::op", failure);
            let before = SolidError::Upstream {
                operation: "intersect::op",
                code,
                entity: None,
            };
            assert_eq!(typed.sm_status(), before.sm_status(), "{failure:?}");
            assert_eq!(typed.upstream_status(), Some(code), "{failure:?}");
            assert_eq!(typed.kind(), before.kind(), "{failure:?}");
            assert_eq!(typed.operation(), before.operation(), "{failure:?}");
            assert_eq!(typed.entity(), None, "{failure:?}");
            assert_eq!(typed.is_port_failure(), before.is_port_failure());
            assert_eq!(typed.is_status_answer(), before.is_status_answer());
            assert_eq!(typed.intersect_failure(), Some(&failure));
        }
        assert_eq!(
            SolidError::intersect_missing("intersect::op", "the edge's curve").to_string(),
            "intersect::op: the edge's curve is missing"
        );
        assert_eq!(
            SolidError::intersect_failed("intersect::op", "the ray hits no face").kind(),
            ErrorKind::Failed
        );
    }

    /// The intersection routines that describe their failures build no bare `Upstream`
    /// status in their library code.
    #[test]
    fn the_described_intersect_routines_build_no_bare_statuses() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/intersect");
        let mut files = vec![
            "point_class.rs".to_owned(),
            "ray_tracer.rs".to_owned(),
            "surface_drop_curve.rs".to_owned(),
            "surface_drop_lib/p1.rs".to_owned(),
            "surface_drop_lib/p2.rs".to_owned(),
            "surface_drop_lib/p3.rs".to_owned(),
            "adv_surface_intersector.rs".to_owned(),
            "api_intersectors.rs".to_owned(),
            "surface_silhouette.rs".to_owned(),
            "surface_tracer.rs".to_owned(),
        ];
        files.extend((1..=9).map(|i| format!("curve_class/p{i}.rs")));
        for rel in files {
            let text = std::fs::read_to_string(src.join(&rel)).expect("read");
            let library = text
                .find("#[cfg(test)]\nmod tests")
                .map_or(text.as_str(), |at| &text[..at]);
            for needle in [
                "SolidError::sm_err(",
                "SolidError::null_pointer(",
                "SolidError::Upstream {",
            ] {
                assert!(!library.contains(needle), "{needle} in {rel}");
            }
        }
    }
}
