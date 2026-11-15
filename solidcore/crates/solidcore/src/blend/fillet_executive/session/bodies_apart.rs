//! Blends built as a body apart (not upstream; solidcore-native, #15129): the weld bead.
//!
//! A weld never changes the part it lies on. The session traces the blends and builds their
//! faces into the intermediate body as a fillet does, then, instead of trimming the target's
//! faces and merging the fillet into it, closes the intermediate body on the target's faces
//! into a body of its own ([`FilletSession::closed_removal_region`], the cliff overflow's
//! closing). The target is only read: no face of it is trimmed, split or deleted, and no
//! edge or vertex is added.

use super::FilletSession;
use crate::topology::Brep;
use crate::{Kernel, Result};

impl FilletSession<'_> {
    /// Whether the filleting builds the blends as a body apart (#15129): the target is left
    /// as it was and the body is taken with [`Self::take_body_apart`]. Off by default.
    pub fn set_bodies_apart(&mut self, apart: bool) {
        self.objects.apart = apart;
    }

    /// The body apart the last filleting built with [`Self::set_bodies_apart`], once.
    pub fn take_body_apart(&mut self) -> Option<Brep> {
        self.objects.body_apart.take()
    }

    /// Whether the filleting builds a body apart.
    pub(super) const fn bodies_apart(&self) -> bool {
        self.objects.apart
    }

    /// Closes the intermediate body into the body apart, the target untouched.
    ///
    /// # Errors
    /// Those of [`Self::closed_removal_region`].
    pub(super) fn close_body_apart(&mut self, k: &mut Kernel) -> Result<()> {
        let body = self.closed_removal_region(k, true)?;
        self.objects.body_apart = Some(body);
        Ok(())
    }
}
