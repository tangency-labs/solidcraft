//! Plate contact: the pair-level decision of Tab and Slot for two plates, the tab plate meeting
//! a plane of the slot plate ([`solidcore::plate_contact::pair_at`]). Ops copies the bodies and
//! hands the plane and the tolerance on as the caller gives them; the kernel reads the faces and
//! decides, and ops does no float maths of its own.

use solidcore::plate_contact::{self as pc, PairJoint, PairSkip, Plane};

use crate::body::Body;
use crate::contact::{copy_of, poisoned};
use crate::session::{run_on, Answer};
use crate::Result;

/// A pair's answer: the joint, or why there is none.
pub type Pair = core::result::Result<PairJoint, PairSkip>;

impl Answer for Pair {}

impl Body {
    /// This body as the tab plate T against `slot` (S) on `p`, a plane of one of S's outward
    /// planar faces, at `tol` (the caller's: ops invents none): a joint (how T meets `p` and its
    /// plate faces) or why there is none ([`solidcore::plate_contact::pair_at`]). Neither body
    /// changes.
    ///
    /// # Errors
    /// [`crate::Error::Refused`], in the kernel's words, for a `tol` not positive and finite, a
    /// bad plane, or a contact or overlap the kernel does not decide; those of the session.
    pub fn plate_pair(&self, slot: &Body, p: Plane, tol: f64) -> Result<Pair> {
        poisoned(&[self, slot])?;
        run_on("plate_contact::pair", &[("t", self), ("s", slot)], |k| {
            let t = copy_of(k, self)?;
            let s = copy_of(k, slot)?;
            pc::pair_at(k, &t, &s, p, tol)
        })
    }

    /// [`Body::plate_pair`] with the swap [m10] ([`solidcore::plate_contact::with_swap`]): when
    /// this body is skipped for want of plate faces or as lying face to face on `slot`, `slot` is
    /// tried as the tab plate on `q`, a plane of one of this body's outward planar faces, and a
    /// joint there makes the skip [`PairSkip::Swapped`].
    ///
    /// # Errors
    /// Those of [`Body::plate_pair`], for either way round.
    pub fn plate_pair_swapped(&self, slot: &Body, p: Plane, q: Plane, tol: f64) -> Result<Pair> {
        poisoned(&[self, slot])?;
        run_on(
            "plate_contact::pair_swapped",
            &[("t", self), ("s", slot)],
            |k| {
                let t = copy_of(k, self)?;
                let s = copy_of(k, slot)?;
                let forward = pc::pair_at(k, &t, &s, p, tol)?;
                pc::with_swap(|| Ok(forward), || pc::pair_at(k, &s, &t, q, tol))
            },
        )
    }
}
