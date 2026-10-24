//! Display states in a session: capture and apply a [`VisibilitySnapshot`], and Isolate's
//! volatile layers. A layer holds the state from before it began; leaving it puts that state
//! back exactly, so whatever was hidden or shown by hand while the layer was active belongs to
//! the layer. The stack is never saved: [`Session::saved_view`] is the bottom layer's state while
//! isolated, which is what `design()`, `is_dirty` and the recovery autosave use.

use solidcraft_doc::display_state::VisibilitySnapshot;

use crate::params::bad;
use crate::{Result, Session, Visibility};

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
        self.revision += 1;
    }
    /// The saved display state that shows exactly what is shown now (what is hidden, and the
    /// looks of states that store them), if any: the camera is not compared.
    pub fn active_display_state(&self) -> Option<&str> {
        self.doc
            .display_states
            .iter()
            .find(|d| d.snapshot.visibility == self.visibility && d.appearances.as_ref().is_none_or(|a| *a == self.doc.appearances))
            .map(|d| d.name.as_str())
    }
    /// How many isolation layers are active (0: not isolated).
    pub fn isolation_depth(&self) -> usize {
        self.isolation.len()
    }
    /// The visibility the design file holds: the state before the first isolation while isolated.
    pub fn saved_view(&self) -> &Visibility {
        self.isolation.first().map_or(&self.visibility, |l| &l.before.visibility)
    }

    /// Isolate bodies (by name) as a new layer: every other body is hidden.
    pub fn isolate_bodies(&mut self, keep: &[String]) -> Result<usize> {
        let all: Vec<String> = self.world_state().bodies.iter().map(|b| b.name.clone()).collect();
        if let Some(missing) = keep.iter().find(|k| !all.contains(k)) {
            return Err(bad("display.isolate", format!("no body `{missing}`")));
        }
        self.push_isolation();
        self.visibility.hide_all_bodies_except(all, keep);
        self.finish_isolation("display.isolate")
    }
    /// Isolate exact occurrence placements (and everything under them) as a new layer.
    pub fn isolate_paths(&mut self, paths: &[Vec<u64>]) -> Result<usize> {
        let (hidden, root_bodies) = self.isolate_paths_lists(paths)?;
        self.push_isolation();
        self.visibility.hidden_occurrences = hidden;
        self.visibility.hidden_bodies = root_bodies;
        self.finish_isolation("display.isolate")
    }
    /// A layer that hides nothing is not a layer: it would pretend something happened.
    fn finish_isolation(&mut self, cmd: &str) -> Result<usize> {
        if self.isolation.last().is_some_and(|l| l.before.visibility == self.visibility) {
            self.isolation.pop();
            return Err(bad(cmd, "Nothing to isolate: everything else is already hidden"));
        }
        self.revision += 1;
        Ok(self.isolation.len())
    }
    fn push_isolation(&mut self) {
        let before = self.capture_visibility();
        self.isolation.push(IsolationLayer { before });
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
        for l in &mut self.isolation {
            l.before.visibility.rename_body(old, new);
        }
    }

    /// Hidden placements and bodies for isolating `paths`: the placements and bodies that are not
    /// the selection, nor above it (ancestor bodies stay).
    fn isolate_paths_lists(&self, paths: &[Vec<u64>]) -> Result<(Vec<Vec<u64>>, Vec<String>)> {
        let cmd = "browser.visibility";
        let mut hidden = Vec::new();
        let mut ancestor_bodies = Vec::new();
        let mut stack = vec![Vec::<u64>::new()];
        let mut count = 0;
        while let Some(parent) = stack.pop() {
            for child in self.occurrence_children(&parent)? {
                count += 1;
                if count > 100_000 {
                    return Err(bad(cmd, "isolate exceeds 100000 placements"));
                }
                if paths.iter().any(|selected| child.path.starts_with(selected)) {
                    continue;
                }
                if paths.iter().any(|selected| selected.starts_with(&child.path)) {
                    ancestor_bodies.extend(child.bodies.into_iter().map(|b| b.world));
                    stack.push(child.path);
                } else {
                    hidden.push(child.path);
                }
            }
        }
        let mut root_bodies: Vec<String> =
            self.model.state().bodies.iter().filter(|b| self.doc.model_body_component(b) == 0).map(|b| b.name.clone()).collect();
        root_bodies.extend(ancestor_bodies);
        Ok((hidden, root_bodies))
    }
}
