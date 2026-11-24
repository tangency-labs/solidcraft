//! A display state as a value: how the design is shown (what is hidden), captured, applied and
//! compared. Named states saved with the design hold the same value, with the looks of the bodies.

use serde::{Deserialize, Serialize};

use crate::Visibility;

/// What is shown: a copy of the session's [`Visibility`] lists, without `locked_bodies` (locking
/// is not how the design looks, so a state neither stores nor changes it). `==` compares two
/// states.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VisibilitySnapshot {
    #[serde(default)]
    pub visibility: Visibility,
}

impl VisibilitySnapshot {
    /// The state `visibility` holds now.
    pub fn capture(visibility: &Visibility) -> Self {
        Self { visibility: Visibility { locked_bodies: Vec::new(), ..visibility.clone() } }
    }
    /// Make `visibility` this state, exactly, except that the bodies locked stay locked. Entries
    /// naming a body, sketch or placement that no longer exists stay in the lists: they hide
    /// nothing, and undoing the delete brings the item back as it was.
    pub fn apply(&self, visibility: &mut Visibility) {
        let locked_bodies = std::mem::take(&mut visibility.locked_bodies);
        *visibility = Visibility { locked_bodies, ..self.visibility.clone() };
    }
    /// Does `visibility` show exactly what this state shows (locks apart)?
    pub fn matches(&self, visibility: &Visibility) -> bool {
        self.visibility == Visibility { locked_bodies: Vec::new(), ..visibility.clone() }
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

impl NamedDisplayState {
    /// A body was renamed: the state follows it, in what it hides and in the looks it keeps.
    pub fn rename_body(&mut self, old: &str, new: &str) {
        self.snapshot.visibility.rename_body(old, new);
        if let Some(looks) = &mut self.appearances {
            if let Some(look) = looks.bodies.remove(old) {
                looks.bodies.insert(new.to_string(), look);
            }
            for face in looks.faces.iter_mut().filter(|f| f.body == old) {
                face.body = new.to_string();
            }
        }
    }
}
