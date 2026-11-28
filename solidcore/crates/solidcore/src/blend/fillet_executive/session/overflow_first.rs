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
//! | [`FilletSession::add_earlier_pass_face`] | `SmFilletExecutive::AddEarlierPassFace` (the fork's `9822462`) |
//! | the stop in `FilletExecutive::refuse_blends_larger_than_faces` | the `m_bClassifyOnly` / `m_bOverflowFirst` stop of `CreateFilletBrep` |
//! | [`FilletSession::set_groups_apart`], [`FilletSession::bands_meet_apart`] | none (not upstream, #2784) |
//!
//! Deviations: both settings are per session and off by default, so a session that does
//! not set them is unchanged. They act where the fork's rail classification runs
//! (`FilletExecutive::refuse_blends_larger_than_faces`).
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
use crate::topology::{EdgeId, FaceId};

impl FilletSession<'_> {
    /// `AddEarlierPassFace` (the fork's, tangency-labs/smlib@9822462, smlib#130): `face`
    /// is one an earlier pass of the same blend made on this body (the rounds that
    /// overflow go first; the caller's `overflow_first` hands the faces its first pass
    /// made to the second pass). A later round that runs on tangentially into an edge of
    /// such a face meets that round as one blend, as when the two go together, rather
    /// than ending at a smooth vertex. Call it before [`Self::create_corners`].
    pub fn add_earlier_pass_face(&mut self, face: FaceId) {
        self.executive.add_earlier_pass_face(FilletObject::Topology(
            FilletBody::Target,
            EntityRef::Face(face),
        ));
    }

    /// `SetOverflowFirst` (the fork's, tangency-labs/smlib `241f7ba`, WR-634; off by
    /// default): when blends of several edges overflow their faces across cliffs (and no
    /// refusal of the cliff overflow applies: none adds material, none rounds into the
    /// body's material), `do_fillet` stops after classifying the rails, building nothing,
    /// with [`crate::blend::fillet_geom::p1::FilletErrorType::OverflowFirst`] in
    /// [`Self::error_info`] and the overflowing edges in [`Self::overflow_edges`].
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

    /// Not upstream (#2784; off by default): the caller found the edges in at least two
    /// groups that share no vertex, and builds blends whose bands meet across a face they
    /// share apart, group by group, then intersects what each leaves. With it, a refusal of
    /// bands that meet while no blend adds material is also noted as that event
    /// ([`Self::bands_meet_apart`]); the refusal itself, with its size, is unchanged.
    /// Preparing the edges keeps it.
    pub fn set_groups_apart(&mut self, apart: bool) {
        self.executive.set_groups_apart(apart);
    }

    /// Not upstream (#2784): whether `do_fillet`'s refusal is the event of
    /// [`Self::set_groups_apart`] (bands that meet across a face, no blend adding material),
    /// in which the caller builds the groups apart; `false` otherwise (preparing the edges
    /// clears it).
    #[must_use]
    pub fn bands_meet_apart(&self) -> bool {
        self.executive.bands_meet_apart()
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
    /// the mixed-convexity mitre (WR-642) or a corner set to bevel.
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
