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
//!
//! Deviations: not upstream. A failure says in a phrase what went wrong; the failure's
//! `operation` path says which routine. A failure names an entity only where the site
//! it replaces did (the point classification's seam failure, at its face). The failures
//! that are checks of a caller's arguments stay [`crate::SolidError::InvalidInput`].

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
}

impl QueryFailure {
    /// The status the C++ reports for the failure (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::Failed(_) => SM_ERR,
            Self::Missing(_) => SM_ERR_NULL_POINTER,
            Self::InvalidValue(_) => SM_ERR_INVALID_INPUT,
        }
    }

    /// The failure's stable [`ErrorKind`]: [`ErrorKind::Missing`] for
    /// [`QueryFailure::Missing`], [`ErrorKind::InvalidInput`] for
    /// [`QueryFailure::InvalidValue`], [`ErrorKind::Failed`] otherwise.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        ErrorKind::of_status(self.comparison_status())
    }
}

impl core::fmt::Display for QueryFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Failed(what) | Self::InvalidValue(what) => f.write_str(what),
            Self::Missing(what) => write!(f, "{what} is missing"),
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

    /// The query routines describe their failures: their library code builds no bare
    /// `Upstream` status, except the files listed (with the bare sites each may still hold)
    /// whose conversion waits on other open work.
    #[test]
    fn the_query_routines_build_no_bare_statuses() {
        assert_eq!(bare_statuses_in("query", 20), Vec::<(String, usize)>::new());
    }

    /// The files under `src/<area>` (and `src/<area>.rs`) whose library code builds bare
    /// statuses, with the number each builds, sorted by path; at least `least` files are
    /// read. Test files (`tests.rs`, `*_tests.rs`) are skipped, and the library part of a
    /// file ends at its test module: `#[cfg(test)]`, then any attributes (such as an
    /// `#[allow(..)]` for the tests' unwraps), then `mod` with any visibility.
    pub(in crate::error) fn bare_statuses_in(area: &str, least: usize) -> Vec<(String, usize)> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = vec![root.join(format!("{area}.rs"))];
        let mut dirs = vec![root.join(area)];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(dir).expect("read dir") {
                let path = entry.expect("entry").path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|x| x == "rs") {
                    files.push(path);
                }
            }
        }
        assert!(files.len() >= least, "{files:?}");
        let mut found = Vec::new();
        for path in files {
            let relative = path
                .strip_prefix(&root)
                .expect("under src")
                .to_string_lossy()
                .replace('\\', "/");
            if relative.ends_with("/tests.rs") || relative.ends_with("_tests.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path)
                .expect("read")
                .replace("\r\n", "\n");
            let library = text
                .match_indices("\n#[cfg(test)]\n")
                .find(|(at, _)| {
                    let mut rest = text[at + "\n#[cfg(test)]\n".len()..].lines();
                    rest.find(|line| {
                        let line = line.trim_start();
                        !(line.starts_with("#[") || line.starts_with("clippy::") || line == ")]")
                    })
                    .is_some_and(|line| {
                        let line = line
                            .strip_prefix("pub(crate) ")
                            .or_else(|| line.strip_prefix("pub(super) "))
                            .or_else(|| line.strip_prefix("pub "))
                            .unwrap_or(line);
                        line.starts_with("mod ")
                    })
                })
                .map_or(text.as_str(), |(at, _)| &text[..at]);
            let bare: usize = [
                "SolidError::sm_err(",
                "SolidError::null_pointer(",
                "SolidError::Upstream {",
                "SolidError::nl_ser(",
            ]
            .iter()
            .map(|needle| library.matches(needle).count())
            .sum();
            if bare > 0 {
                found.push((relative, bare));
            }
        }
        found.sort();
        found
    }
}
