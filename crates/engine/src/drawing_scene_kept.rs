//! What the sheet scene's provider keeps of a drawing once it closes: each view's and each
//! dimension's (or note's) entry as it was, all it drew and what that was drawn from, with its world
//! state let go, so a closed drawing keeps no model; for the drawing closed last, each view's
//! pictures' last pixels too, shown stale until drawn again. The drawing opened again starts from
//! them. Only in memory, never saved.

use std::collections::BTreeMap;
use std::sync::Arc;

use solidcraft_doc::ModelState;

use super::{DimensionEntry, Entry};

/// Each closed drawing's last lines, by drawing.
#[derive(Default)]
pub(super) struct KeptLines(BTreeMap<u64, Kept>);

/// A closed drawing's last lines: its views' and its dimensions' entries, by id.
#[derive(Clone, Default)]
pub(super) struct Kept {
    pub(super) views: BTreeMap<u64, Entry>,
    pub(super) dimensions: BTreeMap<u64, DimensionEntry>,
}

impl Kept {
    /// The entries of a drawing closing, kept whole but drawn from a world state that has gone, as
    /// [`super::Cache::redraw`]'s are: so they hold no model, and each is out of date once taken back.
    pub(super) fn closing(mut views: BTreeMap<u64, Entry>, mut dimensions: BTreeMap<u64, DimensionEntry>) -> Kept {
        let gone: Arc<ModelState> = Arc::default();
        views.values_mut().for_each(|view| view.state = gone.clone());
        dimensions.values_mut().for_each(|dimension| dimension.state = gone.clone());
        Kept { views, dimensions }
    }
}

impl KeptLines {
    /// Keeps `kept` as drawing `drawing`'s. It has none kept already: they were taken back when it
    /// opened. Only the drawing closed last keeps its pictures' pixels (a picture can be 64 MB), so
    /// what is kept stays within one drawing's pictures.
    pub(super) fn keep(&mut self, drawing: u64, kept: Kept) {
        for other in self.0.values_mut() {
            other.views.values_mut().for_each(|view| view.pixels.clear());
        }
        self.0.insert(drawing, kept);
    }

    /// Drawing `drawing`'s, taken out.
    pub(super) fn take(&mut self, drawing: u64) -> Kept {
        self.0.remove(&drawing).unwrap_or_default()
    }

    /// Drawing `drawing`'s alone, copied, if it has any kept.
    pub(super) fn copy_of(&self, drawing: Option<u64>) -> KeptLines {
        KeptLines(drawing.and_then(|d| Some((d, self.0.get(&d)?.clone()))).into_iter().collect())
    }

    /// Forgets each drawing `exists` says is gone.
    pub(super) fn retain(&mut self, exists: impl Fn(u64) -> bool) {
        self.0.retain(|drawing, _| exists(*drawing));
    }

    /// Drawing `drawing`'s.
    #[cfg(test)]
    pub(super) fn of(&self, drawing: u64) -> Option<&Kept> {
        self.0.get(&drawing)
    }
}
