//! What the sheet scene's provider keeps of a drawing once it closes: each view's last lines, how the
//! view drew them and what they showed, so the drawing opened again starts from them. No world state
//! is held, so a closed drawing keeps no model. Only in memory, never saved.

use std::collections::BTreeMap;
use std::sync::Arc;

use solidcraft_doc::drawing::view::ResolvedView;

use super::{Drawn, SourceKey};

/// Each closed drawing's views' last lines, by drawing and view.
#[derive(Default)]
pub(super) struct KeptLines(BTreeMap<u64, BTreeMap<u64, KeptView>>);

/// A view's last lines: drawn as `view` drew, from a world state that showed `shows`.
pub(super) struct KeptView {
    pub(super) view: ResolvedView,
    pub(super) drawn: Drawn,
    pub(super) shows: Option<Arc<SourceKey>>,
}

impl KeptLines {
    /// Keeps `views` as drawing `drawing`'s. It has none kept already: they were taken back when it
    /// opened.
    pub(super) fn keep(&mut self, drawing: u64, views: BTreeMap<u64, KeptView>) {
        self.0.insert(drawing, views);
    }

    /// Drawing `drawing`'s, taken out.
    pub(super) fn take(&mut self, drawing: u64) -> BTreeMap<u64, KeptView> {
        self.0.remove(&drawing).unwrap_or_default()
    }

    /// Forgets each drawing `exists` says is gone.
    pub(super) fn retain(&mut self, exists: impl Fn(u64) -> bool) {
        self.0.retain(|drawing, _| exists(*drawing));
    }

    /// Drawing `drawing`'s, by view.
    #[cfg(test)]
    pub(super) fn of(&self, drawing: u64) -> impl Iterator<Item = (&u64, &KeptView)> {
        self.0.get(&drawing).into_iter().flatten()
    }
}
