//! The fork's overflow-first hooks of the live session: stop after the rails are
//! classified and report the edges whose blends overflow across cliffs.
//!
//! Upstream: the fork tangency-labs/smlib `241f7ba` (`source/SMLib/inc/SmFilletExecutive.h`,
//! `source/SMLib/src/SmFilletExecutive.cpp`, `source/SMLib/inc/SmFilletGeom.h`; WR-634); not
//! in SMLib at the pin.
//!
//! | Rust | Upstream (fork) |
//! |------|-----------------|
//! | [`FilletSession::set_overflow_first`] | `SmFilletExecutive::SetOverflowFirst` |
//! | [`FilletSession::set_classify_only`] | `SmFilletExecutive::SetClassifyOnly` |
//! | [`FilletSession::overflow_edges`] | `SmFilletExecutive::GetOverflowEdges` |
//! | [`FilletSession::mitred_edges`] | `SmFilletExecutive::GetMitredEdges` (the fork's `9ba2b12`) |
//! | the stop in `FilletExecutive::refuse_blends_larger_than_faces` | the `m_bClassifyOnly` / `m_bOverflowFirst` stop of `CreateFilletBrep` |
//!
//! Deviations: both settings are per session and off by default, so a session that does
//! not set them is unchanged. They act only where the fork's rail classification runs in
//! the port, with `fixes.fillet_refuse_rails_leaving_faces` and
//! `fixes.fillet_cliff_overflow` on; without those flags the session builds as before.
//! The stop is reported as `FilletErrorType::OverflowFirst` (11; the fork's
//! `SM_FILERR_OVERFLOW_FIRST`). The overflowing edges come back as target edge ids
//! rather than a pointer array. The ordering itself (blend the edges that overflow on
//! their own first, then what is left of the others) is the caller's, as in the fork,
//! where it is in the C API; so is the fork's later use of the mitred pairs
//! (`9ba2b12`: a round mitred to one that goes first goes first with it), of which only
//! the SMLib accessors are here ([`FilletSession::mitred_edges`],
//! `FilletCorner::is_mitre`, `FilletCorner::filleted_edgeuses`). No recursion.

use super::{FilletBody, FilletObject, FilletSession};
use crate::error::EntityRef;
use crate::topology::EdgeId;

impl FilletSession<'_> {
    /// `SetOverflowFirst` (the fork's, tangency-labs/smlib `241f7ba`, WR-634; off by
    /// default): when blends of several edges overflow their faces across cliffs (and no
    /// refusal of the cliff overflow applies: none adds material, none rounds into the
    /// body's material), `do_fillet` stops after classifying the rails, building nothing,
    /// with [`crate::blend::fillet_geom::p1::FilletErrorType::OverflowFirst`] in
    /// [`Self::error_info`] and the overflowing edges in [`Self::overflow_edges`]. Acts only
    /// with `fixes.fillet_refuse_rails_leaving_faces` and `fixes.fillet_cliff_overflow`.
    /// Preparing the edges keeps it.
    pub fn set_overflow_first(&mut self, first: bool) {
        self.executive.set_overflow_first(first);
    }

    /// `SetClassifyOnly` (the fork's, tangency-labs/smlib `241f7ba`, WR-634; off by
    /// default): `do_fillet` always stops after classifying the rails, building nothing,
    /// with [`crate::blend::fillet_geom::p1::FilletErrorType::OverflowFirst`] and
    /// [`Self::overflow_edges`] the edges whose blends overflow (empty when every blend
    /// fits). Acts only with the flags of [`Self::set_overflow_first`]. Preparing the
    /// edges keeps it.
    pub fn set_classify_only(&mut self, only: bool) {
        self.executive.set_classify_only(only);
    }

    /// `GetOverflowEdges` (the fork's, tangency-labs/smlib `241f7ba`, WR-634): the target
    /// edges whose blends overflow, in solver order, when `do_fillet` stopped with
    /// [`crate::blend::fillet_geom::p1::FilletErrorType::OverflowFirst`]; empty otherwise
    /// (preparing the edges clears them).
    #[must_use]
    pub fn overflow_edges(&self) -> Vec<EdgeId> {
        self.executive
            .overflow_edges()
            .iter()
            .filter_map(|edge| match edge {
                FilletObject::Topology(FilletBody::Target, EntityRef::Edge(edge)) => Some(*edge),
                _ => None,
            })
            .collect()
    }

    /// `GetMitredEdges` (the fork's, tangency-labs/smlib `9ba2b12`, WR-634): when
    /// `do_fillet` stopped with
    /// [`crate::blend::fillet_geom::p1::FilletErrorType::OverflowFirst`], the target edges
    /// of each two blends mitred at a face's concave corner (an Nx2 corner whose
    /// `IsMitre` is set), two by two in corner order; empty otherwise (preparing the edges
    /// clears them). Mitres at such corners come from
    /// `fixes.fillet_mitre_concave_face_corners` (WR-642) or a corner set to bevel.
    #[must_use]
    pub fn mitred_edges(&self) -> Vec<EdgeId> {
        self.executive
            .mitred_edges()
            .iter()
            .filter_map(|edge| match edge {
                FilletObject::Topology(FilletBody::Target, EntityRef::Edge(edge)) => Some(*edge),
                _ => None,
            })
            .collect()
    }
}
