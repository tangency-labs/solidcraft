//! Display states in a session: capture and apply a [`VisibilitySnapshot`], and find which saved
//! state is showing now.

use solidcraft_doc::display_state::VisibilitySnapshot;

use crate::Session;

impl Session {
    /// The state now.
    pub fn capture_visibility(&self) -> VisibilitySnapshot {
        VisibilitySnapshot::capture(&self.visibility)
    }
    /// Make the state `snapshot`, exactly, and let the views notice.
    pub fn apply_visibility(&mut self, snapshot: &VisibilitySnapshot) {
        snapshot.apply(&mut self.visibility);
        // Placements may have been deleted since the snapshot was taken.
        let hidden = std::mem::take(&mut self.visibility.hidden_occurrences);
        self.visibility.hidden_occurrences = hidden.into_iter().filter(|p| self.occurrence_placement(p).is_ok()).collect();
        self.revision += 1;
    }
    /// The saved display state that shows exactly what is shown now (what is hidden, and the
    /// looks of states that store them), if any.
    pub fn active_display_state(&self) -> Option<&str> {
        self.doc
            .display_states
            .iter()
            .find(|d| d.snapshot.visibility == self.visibility && d.appearances.as_ref().is_none_or(|a| *a == self.doc.appearances))
            .map(|d| d.name.as_str())
    }
}
