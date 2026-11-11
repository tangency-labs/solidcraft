//! Why a topology routine fails, in solidcore's terms.
//!
//! Upstream: none. These describe failures of the topology subsystem (`crate::topology`:
//! the B-rep constructor, cache and data streams, faces, edges, edge uses, loop uses,
//! vertex uses, shells, trimming tools, cutting, mirroring, planar sections, vertex fans,
//! projection, traversal and the file wrappers). Each also records, for difftest only,
//! the status the C++ reports for it (crate-private `comparison_status`, read through
//! [`crate::SolidError::sm_status`]).
//!
//! | Rust                                 | Status difftest compares      |
//! |--------------------------------------|-------------------------------|
//! | [`TopologyFailure::Failed`]          | `SM_ERR`                      |
//! | [`TopologyFailure::Missing`]         | `SM_ERR_NULL_POINTER`         |
//! | [`TopologyFailure::InvalidValue`]    | `SM_ERR_INVALID_INPUT`        |
//! | [`TopologyFailure::Inconsistent`]    | `SM_ERR_ASSERT_FAILURE`       |
//! | [`TopologyFailure::OutOfMemory`]     | `SM_ERR_OUT_OF_MEMORY`        |
//! | [`TopologyFailure::DropFailed`]      | the drop's own recorded status |
//!
//! Deviations: not upstream. A failure says in a phrase (or by its variant) what went
//! wrong; the failure's `operation` path says which routine, and its entity is the one
//! the site it replaces named (a vertex use for the vertex-use routines, none
//! elsewhere). The failures that are checks of a caller's arguments stay
//! [`crate::SolidError::InvalidInput`].
//! `brep/p9`'s face-orientation repair keeps its bare final status 1 (upstream `TRUE`),
//! which no failure here stands for.

use super::{
    EntityRef, ErrorKind, Failure, Reason, SM_ERR, SM_ERR_ASSERT_FAILURE, SM_ERR_INVALID_INPUT,
    SM_ERR_NULL_POINTER, SM_ERR_OUT_OF_MEMORY,
};

/// Why a topology routine fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TopologyFailure {
    /// The routine could not continue, as the phrase says (for example "the loop does
    /// not close").
    Failed(&'static str),
    /// Something the routine needs is missing, as the phrase says (for example "the
    /// edge's curve").
    Missing(&'static str),
    /// A value the routine is given or computes with is not usable, as the phrase says
    /// (for example "the plane normal has zero length"). Upstream returns
    /// `SM_ERR_INVALID_INPUT` as a status here, so its kind is
    /// [`ErrorKind::InvalidInput`].
    InvalidValue(&'static str),
    /// A consistency check of the routine's own data did not hold, as the phrase says.
    Inconsistent(&'static str),
    /// Memory for the data could not be had, as the phrase says.
    OutOfMemory(&'static str),
    /// Dropping the edge use's curve onto its face failed earlier, with `status`, and the
    /// edge use keeps that failure.
    DropFailed {
        /// The status the drop recorded (a comparison detail).
        status: i32,
    },
}

impl TopologyFailure {
    /// The status the C++ reports for the failure (a comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::Failed(_) => SM_ERR,
            Self::Missing(_) => SM_ERR_NULL_POINTER,
            Self::InvalidValue(_) => SM_ERR_INVALID_INPUT,
            Self::Inconsistent(_) => SM_ERR_ASSERT_FAILURE,
            Self::OutOfMemory(_) => SM_ERR_OUT_OF_MEMORY,
            Self::DropFailed { status } => *status,
        }
    }

    /// The failure's stable [`ErrorKind`]: [`ErrorKind::Missing`] for
    /// [`TopologyFailure::Missing`], [`ErrorKind::InvalidInput`] for
    /// [`TopologyFailure::InvalidValue`], the drop status's kind for
    /// [`TopologyFailure::DropFailed`], [`ErrorKind::Failed`] otherwise.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        ErrorKind::of_status(self.comparison_status())
    }
}

impl core::fmt::Display for TopologyFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Failed(what) | Self::InvalidValue(what) => f.write_str(what),
            Self::Missing(what) => write!(f, "{what} is missing"),
            Self::Inconsistent(what) => write!(f, "a consistency check failed: {what}"),
            Self::OutOfMemory(what) => write!(f, "out of memory for {what}"),
            Self::DropFailed { .. } => {
                f.write_str("the edge use's curve could not be dropped onto its face")
            }
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a topology routine that fails with `failure`.
    #[must_use]
    pub(crate) fn topology(operation: &'static str, failure: TopologyFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// [`crate::SolidError::Failed`] for a topology routine that fails with `failure`
    /// about `entity`.
    #[must_use]
    pub(crate) fn topology_at(
        operation: &'static str,
        failure: TopologyFailure,
        entity: EntityRef,
    ) -> Self {
        Failure::new(operation, failure).at(entity).into_error()
    }

    /// [`crate::SolidError::Failed`] for a topology routine that could not continue, as
    /// the phrase says, with status [`SM_ERR`] (upstream `return SE(SM_ERR)`).
    #[must_use]
    pub(crate) fn topology_failed(operation: &'static str, what: &'static str) -> Self {
        Self::topology(operation, TopologyFailure::Failed(what))
    }

    /// [`crate::SolidError::Failed`] for something a topology routine needs that is not
    /// there, as the phrase says, with status [`SM_ERR_NULL_POINTER`] (upstream
    /// `NER(ptr)` on a `NULL` pointer).
    #[must_use]
    pub(crate) fn topology_missing(operation: &'static str, what: &'static str) -> Self {
        Self::topology(operation, TopologyFailure::Missing(what))
    }

    /// [`crate::SolidError::Failed`] for a value a topology routine computes with that has
    /// no defined result, as the phrase says, with status [`SM_ERR_INVALID_INPUT`]
    /// (upstream `return(SM_ERR_INVALID_INPUT)`, or `SER` of a vector operation that
    /// returns it).
    #[must_use]
    pub(crate) fn topology_invalid_value(operation: &'static str, what: &'static str) -> Self {
        Self::topology(operation, TopologyFailure::InvalidValue(what))
    }

    /// The topology failure behind this error, for a [`crate::SolidError::Failed`] with a
    /// [`Reason::Topology`].
    #[must_use]
    pub fn topology_failure(&self) -> Option<&TopologyFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Topology(t) => Some(t),
                _ => None,
            },
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::SM_ERR_OUTSIDE_OF_DOMAIN;
    use crate::topology::VertexuseId;
    use crate::SolidError;

    #[test]
    fn topology_failures_keep_the_status_kind_and_entity_of_the_upstream_they_replace() {
        let vu = Some(EntityRef::Vertexuse(VertexuseId::from_index(3)));
        let cases = [
            (TopologyFailure::Failed("x"), SM_ERR),
            (TopologyFailure::Missing("x"), SM_ERR_NULL_POINTER),
            (TopologyFailure::InvalidValue("x"), SM_ERR_INVALID_INPUT),
            (TopologyFailure::Inconsistent("x"), SM_ERR_ASSERT_FAILURE),
            (TopologyFailure::OutOfMemory("x"), SM_ERR_OUT_OF_MEMORY),
            (
                TopologyFailure::DropFailed {
                    status: SM_ERR_OUTSIDE_OF_DOMAIN,
                },
                SM_ERR_OUTSIDE_OF_DOMAIN,
            ),
        ];
        for (failure, code) in cases {
            for entity in [None, vu] {
                let typed = match entity {
                    Some(e) => SolidError::topology_at("topology::op", failure, e),
                    None => SolidError::topology("topology::op", failure),
                };
                let before = SolidError::Upstream {
                    operation: "topology::op",
                    code,
                    entity,
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
                assert_eq!(typed.topology_failure(), Some(&failure));
            }
        }
        assert_eq!(
            SolidError::topology_missing("topology::op", "the edge's curve").to_string(),
            "topology::op: the edge's curve is missing"
        );
        assert_eq!(
            SolidError::topology_at(
                "topology::op",
                TopologyFailure::Failed("the vertex use has no point"),
                EntityRef::Vertexuse(VertexuseId::from_index(3)),
            )
            .to_string(),
            format!(
                "topology::op: the vertex use has no point ({})",
                EntityRef::Vertexuse(VertexuseId::from_index(3))
            )
        );
        assert_eq!(
            SolidError::topology_invalid_value("topology::op", "x").kind(),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            SolidError::topology_failed("topology::op", "x").kind(),
            ErrorKind::Failed
        );
        assert_eq!(SolidError::sm_err("topology::op").topology_failure(), None);
    }

    /// A sample of the second set of converted files (`brep/p1`, `p5` to `p10`,
    /// `face/p2`): each keeps the operation, status, kind and entity of its `Upstream`.
    #[test]
    fn the_brep_edit_and_face_samples_keep_their_upstream_answers() {
        let samples = [
            (
                "topology::brep::p1",
                TopologyFailure::InvalidValue("no new infinite region was given"),
                SM_ERR_INVALID_INPUT,
            ),
            (
                "topology::brep::p5",
                TopologyFailure::Failed("editing is not enabled"),
                SM_ERR,
            ),
            (
                crate::topology::brep::p5::NO_VERTEX_SECTOR_OP,
                TopologyFailure::Missing("a vertex sector at the curve's start"),
                SM_ERR_NULL_POINTER,
            ),
            (
                "topology::brep::p6",
                TopologyFailure::Missing("the face's upward face use"),
                SM_ERR_NULL_POINTER,
            ),
            (
                "topology::brep::p7::unstitch",
                TopologyFailure::Failed("not every face could be copied"),
                SM_ERR,
            ),
            (
                "topology::brep::p8",
                TopologyFailure::InvalidValue("the curve loops are not valid"),
                SM_ERR_INVALID_INPUT,
            ),
            (
                "topology::brep::p9",
                TopologyFailure::Missing("the face use's mate"),
                SM_ERR_NULL_POINTER,
            ),
            (
                "topology::face::trim_curve_breaks",
                TopologyFailure::Failed("the trim curve has fewer than two knots"),
                SM_ERR,
            ),
        ];
        for (operation, failure, code) in samples {
            let typed = SolidError::topology(operation, failure);
            let before = SolidError::Upstream {
                operation,
                code,
                entity: None,
            };
            assert_eq!(typed.sm_status(), before.sm_status(), "{typed:?}");
            assert_eq!(typed.upstream_status(), Some(code), "{typed:?}");
            assert_eq!(typed.kind(), before.kind(), "{typed:?}");
            assert_eq!(typed.operation(), before.operation(), "{typed:?}");
            assert_eq!(typed.entity(), None, "{typed:?}");
            assert_eq!(typed.is_port_failure(), before.is_port_failure());
            assert_eq!(typed.is_status_answer(), before.is_status_answer());
        }
    }

    /// The converted topology routines describe their failures: their library code
    /// builds no bare `Upstream` status.
    #[test]
    fn the_topology_routines_build_no_bare_statuses() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/topology");
        for (rel, held) in [
            "api_files.rs",
            "api_projection.rs",
            "brep/cutting.rs",
            "brep/local_operation.rs",
            "brep/mirror.rs",
            "brep/p1.rs",
            "brep/p10.rs",
            "brep/p2.rs",
            "brep/p3.rs",
            "brep/p4.rs",
            "brep/p5.rs",
            "brep/p6.rs",
            "brep/p7.rs",
            "brep/p8.rs",
            "brep/planar_section.rs",
            "brep/validation/assert_valid.rs",
            "brep/validation/assert_valid/seams.rs",
            "brep/validation/pointers.rs",
            "brep/validation/soundness.rs",
            "brep/vertex_fans.rs",
            "brep_cache.rs",
            "brep_constructor.rs",
            "brep_data.rs",
            "brep_data_write.rs",
            "database_io.rs",
            "edge.rs",
            "edgeuse/p1.rs",
            "edgeuse/p2.rs",
            "edgeuse/p3.rs",
            "face/p1.rs",
            "face/p2.rs",
            "face/p3.rs",
            "geometry_write.rs",
            "loopuse.rs",
            "part_file.rs",
            "proto_topology.rs",
            "proto_topology/p1.rs",
            "proto_topology/p1/api_transform.rs",
            "proto_topology/p1/assembly.rs",
            "proto_topology/p1/copy_brep_map.rs",
            "proto_topology/p1/owning_topology.rs",
            "proto_topology/p1/proto.rs",
            "proto_topology/p2.rs",
            "proto_topology/p2/imprint_ends.rs",
            "proto_topology/p3.rs",
            "shell.rs",
            "topology.rs",
            "topology_traverser.rs",
            "trimming_tools.rs",
            "trimming_tools/curve_loops.rs",
            "vertex/corner.rs",
            "vertexuse.rs",
        ]
        .map(|rel| (rel, 0))
        .into_iter()
        // held back: the face-orientation repair's final status 1 (upstream `TRUE`)
        .chain([("brep/p9.rs", 1)])
        {
            let text = std::fs::read_to_string(src.join(rel))
                .expect("read")
                .replace("\r\n", "\n");
            let library = text
                .find("\n#[cfg(test)]\nmod ")
                .map_or(text.as_str(), |at| &text[..at]);
            for needle in ["SolidError::sm_err(", "SolidError::null_pointer("] {
                assert!(!library.contains(needle), "{needle} in {rel}");
            }
            assert_eq!(
                library.matches("SolidError::Upstream {").count(),
                held,
                "bare Upstream statuses in {rel}"
            );
        }
    }
}
