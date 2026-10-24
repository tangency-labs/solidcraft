//! A display state as a value: how the design is shown (what is hidden), captured, applied and
//! compared. A stack of these, each temporary and unnamed, is Isolate's layers
//! (`Session::isolate_bodies`); named states saved with the design are the same value.

use serde::{Deserialize, Serialize};

use crate::Visibility;

/// What is shown: a copy of the session's [`Visibility`] lists. `==` compares two states.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VisibilitySnapshot {
    #[serde(default)]
    pub visibility: Visibility,
}

impl VisibilitySnapshot {
    /// The state `visibility` holds now.
    pub fn capture(visibility: &Visibility) -> Self {
        Self { visibility: visibility.clone() }
    }
    /// Make `visibility` this state, exactly. Entries naming a body, sketch or placement that no
    /// longer exists stay in the lists: they hide nothing, and undoing the delete brings the
    /// item back as it was.
    pub fn apply(&self, visibility: &mut Visibility) {
        *visibility = self.visibility.clone();
    }
}

/// A display state saved with the design: what is shown and how it looks. The camera is not part
/// of it: Named Views own that (an old file's `camera` field is ignored).
/// Its looks are display data too: applying it changes no geometry.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NamedDisplayState {
    pub name: String,
    #[serde(default)]
    pub snapshot: VisibilitySnapshot,
    /// How the bodies look (colours, appearances) when the state was saved. Absent in states
    /// saved before appearances were part of a state: applying those leaves the looks alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearances: Option<crate::appearance::Appearances>,
}
