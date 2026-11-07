//! What the sheet scene's provider keeps of a drawing once it closes: each view's last lines, how the
//! view drew them and what they showed, and each dimension's last reading, so the drawing opened
//! again starts from them. No world state is held, so a closed drawing keeps no model. Only in
//! memory, never saved.

use std::collections::BTreeMap;
use std::sync::Arc;

use solidcraft_doc::drawing::dimension::Dimension;
use solidcraft_doc::drawing::view::ResolvedView;

use super::{Drawn, DrawnDimension, Lettering, SourceKey};

/// Each closed drawing's last lines, by drawing.
#[derive(Default)]
pub(super) struct KeptLines(BTreeMap<u64, Kept>);

/// A closed drawing's last lines: its views' and its dimensions', by id.
#[derive(Default)]
pub(super) struct Kept {
    pub(super) views: BTreeMap<u64, KeptView>,
    pub(super) dimensions: BTreeMap<u64, KeptDimension>,
}

/// A view's last lines: drawn as `view` drew, from a world state that showed `shows`.
pub(super) struct KeptView {
    pub(super) view: ResolvedView,
    pub(super) drawn: Drawn,
    pub(super) shows: Option<Arc<SourceKey>>,
}

/// A dimension's last reading: of `record`, on its view's drawing `view_serial`, with `lettering`.
pub(super) struct KeptDimension {
    pub(super) record: Arc<Dimension>,
    pub(super) view_serial: u64,
    pub(super) lettering: Lettering,
    pub(super) drawn: DrawnDimension,
}

impl KeptLines {
    /// Keeps `kept` as drawing `drawing`'s. It has none kept already: they were taken back when it
    /// opened.
    pub(super) fn keep(&mut self, drawing: u64, kept: Kept) {
        self.0.insert(drawing, kept);
    }

    /// Drawing `drawing`'s, taken out.
    pub(super) fn take(&mut self, drawing: u64) -> Kept {
        self.0.remove(&drawing).unwrap_or_default()
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
