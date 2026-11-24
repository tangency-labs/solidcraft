//! Display states in a session: capture and apply a [`VisibilitySnapshot`], find which saved
//! state is showing now, and Isolate's layers.
//!
//! A layer holds the state from before it began; leaving it puts that state back exactly, so
//! whatever was hidden or shown by hand while the layer was active belongs to the layer. The
//! stack is view state, never saved: [`Session::saved_view`] is the bottom layer's state while
//! isolated, which is what the design file, `is_dirty` and the recovery autosave use. It goes
//! through undo, redo and checkpoints together with the visibility it belongs to
//! ([`crate::Snapshot::isolation`]).

use std::borrow::Cow;

use solidcraft_doc::display_state::VisibilitySnapshot;

use crate::{Session, Visibility};

/// One level of isolation.
#[derive(Clone, Debug, PartialEq)]
pub struct IsolationLayer {
    /// The state to go back to when the layer is left.
    pub before: VisibilitySnapshot,
}

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
        self.refresh_display();
    }
    /// The saved display state that shows exactly what is shown now (what is hidden, and the
    /// looks of states that store them), if any.
    pub fn active_display_state(&self) -> Option<&str> {
        self.doc
            .display_states
            .iter()
            .find(|d| d.snapshot.matches(&self.visibility) && d.appearances.as_ref().is_none_or(|a| *a == self.doc.appearances))
            .map(|d| d.name.as_str())
    }
    /// How many isolation layers are active (0: not isolated).
    pub fn isolation_depth(&self) -> usize {
        self.isolation.len()
    }
    /// The visibility the design file holds: the state from before the first isolation while
    /// isolated (with the locks as they are now: a lock is not part of how the design looks).
    pub fn saved_view(&self) -> Cow<'_, Visibility> {
        match self.isolation.first() {
            None => Cow::Borrowed(&self.visibility),
            Some(layer) => Cow::Owned(Visibility { locked_bodies: self.visibility.locked_bodies.clone(), ..layer.before.visibility.clone() }),
        }
    }
    /// Take the state to go back to, before a command changes `visibility` to isolate.
    pub(crate) fn begin_isolation(&self) -> IsolationLayer {
        IsolationLayer { before: self.capture_visibility() }
    }
    /// The command isolated: `layer` is a new layer on top. A press that hides nothing more adds
    /// none, because it would pretend something happened. True when a layer was added.
    pub(crate) fn finish_isolation(&mut self, layer: IsolationLayer) -> bool {
        if layer.before.matches(&self.visibility) {
            return false;
        }
        self.isolation.push(layer);
        true
    }
    /// Leave the top layer: the state from before it comes back. False when not isolated.
    pub fn unisolate(&mut self) -> bool {
        let Some(layer) = self.isolation.pop() else { return false };
        self.apply_visibility(&layer.before);
        true
    }
    /// Leave every layer: the state from before the first one comes back. False when not isolated.
    pub fn unisolate_all(&mut self) -> bool {
        if self.isolation.is_empty() {
            return false;
        }
        let base = self.isolation.remove(0);
        self.isolation.clear();
        self.apply_visibility(&base.before);
        true
    }
    /// A body was renamed: the states to go back to follow it.
    pub fn rename_body_in_layers(&mut self, old: &str, new: &str) {
        for layer in &mut self.isolation {
            layer.before.visibility.rename_body(old, new);
        }
    }
}
