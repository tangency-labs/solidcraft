//! What the sheet scene's provider keeps of a drawing once it closes: each view's last lines, how the
//! view drew them and what they showed, and each dimension's last reading, so the drawing opened
//! again starts from them, each note's of a view, which rides with them, and, for the drawing closed
//! last, each view's pictures' last pixels, shown stale until drawn again. No world state is held, so a
//! closed drawing keeps no model. The pictures are only in memory, never saved; the views' lines are
//! also kept in the design's file ([`solidcraft_io::SheetView`]): a drawing opened from it starts from
//! them as from lines kept in memory ([`from_file`], [`saved`]).

use std::collections::BTreeMap;
use std::sync::Arc;

use solidcraft_doc::drawing::Drawing;
use solidcraft_doc::drawing::view::ResolvedView;
use solidcraft_io::{SheetView, ShownBody};

use super::{
    Annotation, Drawn, DrawnDimension, DrawnFill, DrawnSketch, Lettering, Pixels, SketchContext, SourceInputs, SourceKey, Status, ThreadInfo, Vec2,
};

/// Each closed drawing's last lines, by drawing.
#[derive(Default)]
pub(super) struct KeptLines(BTreeMap<u64, Kept>);

/// A closed drawing's last lines: its views' and its dimensions', by id.
#[derive(Clone, Default)]
pub(super) struct Kept {
    pub(super) views: BTreeMap<u64, KeptView>,
    pub(super) dimensions: BTreeMap<u64, KeptDimension>,
    pub(super) sketches: BTreeMap<u64, KeptSketch>,
    pub(super) fills: BTreeMap<u64, KeptFill>,
}

/// The authored sketch's last marks and the definitions used to solve them. Neither
/// the definitions nor the drawing-only context holds a document or a world state.
#[derive(Clone)]
pub(super) struct KeptSketch {
    pub(super) source: Arc<SourceInputs>,
    pub(super) context: Arc<SketchContext>,
    pub(super) drawn: DrawnSketch,
}

/// A closed drawing's last fill paint, without a document or a model state.
#[derive(Clone)]
pub(super) struct KeptFill {
    pub(super) source: Arc<SourceInputs>,
    pub(super) context: Arc<SketchContext>,
    pub(super) drawn: DrawnFill,
}

/// A view's last lines: drawn as `view` drew, from a world state that showed `shows`.
#[derive(Clone)]
pub(super) struct KeptView {
    pub(super) view: ResolvedView,
    pub(super) drawn: Drawn,
    pub(super) shows: Option<Arc<SourceKey>>,
    pub(super) pixels: BTreeMap<u64, Pixels>,
}

/// A dimension's last reading, or a note's: of `record`, on its view's drawing `view_serial`, with
/// `lettering`.
#[derive(Clone)]
pub(super) struct KeptDimension {
    pub(super) record: Annotation,
    pub(super) view_serial: u64,
    pub(super) lettering: Lettering,
    pub(super) drawn: DrawnDimension,
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
    pub(super) fn of(&self, drawing: u64) -> Option<&Kept> {
        self.0.get(&drawing)
    }
}

/// Drawing `d`'s lines its file kept ([`solidcraft_io::sheet_views`]), as lines kept in memory: each
/// view's that still draws as it did when it was saved. Whether they show what the model shows now
/// is asked as of any kept lines ([`super::keep_what_shows_the_same`]). Their lines name no body
/// (picking draws them again, [`super::line_at`]); their dimensions are read again.
pub(super) fn from_file(d: &Drawing, bytes: &[u8]) -> Kept {
    let kept = |saved: SheetView| -> Option<(u64, KeptView)> {
        let view = d.resolve_view(saved.view).ok().filter(|view| view.flat.is_none() && digest(view) == saved.resolved)?;
        let frame = crate::drawing_view::frame_at(view.basis, view.scale.value, saved.model_centre, Vec2::ZERO, super::CMD).ok()?;
        let shows = saved.shows.into_iter().map(|b| (b.path, b.body, b.reference, b.geometry, b.threads)).collect();
        let drawn = Drawn {
            group: Some(saved.group),
            status: Status::Drawn,
            stats: None,
            boundary: saved.boundary,
            frame: Some(frame),
            centre: saved.model_centre,
            owners: None,
            why: saved.why,
            hatches: Vec::new(),
        };
        Some((saved.view, KeptView { view, drawn, shows: Some(Arc::new(SourceKey(shows))), pixels: BTreeMap::new() }))
    };
    Kept { views: solidcraft_io::sheet_views(bytes).into_iter().filter_map(kept).collect(), ..Kept::default() }
}

/// View `id` of `d` as its file keeps it, when it is current: drawn as `d` draws it now, from a model
/// that showed `now`. A flat pattern view's lines are quick to draw again, and aren't kept.
pub(super) fn saved(d: &Drawing, id: u64, view: &ResolvedView, drawn: &Drawn, shows: Option<&SourceKey>, now: &SourceKey) -> Option<SheetView> {
    let current = view.flat.is_none() && shows == Some(now) && d.resolve_view(id).ok().as_ref() == Some(view);
    let (true, Status::Drawn, Some(group), Some(_)) = (current, &drawn.status, &drawn.group, drawn.frame) else { return None };
    let shown = |(path, body, reference, geometry, threads): &(Vec<u64>, String, bool, u64, Vec<ThreadInfo>)| ShownBody {
        path: path.clone(),
        body: body.clone(),
        reference: *reference,
        geometry: *geometry,
        threads: threads.clone(),
    };
    Some(SheetView {
        view: id,
        resolved: digest(view),
        shows: now.0.iter().map(shown).collect(),
        group: group.clone(),
        boundary: drawn.boundary,
        model_centre: drawn.centre,
        why: drawn.why.clone(),
    })
}

/// A view as it draws, as a number: the same in every run of one build (its `Debug` text holds no
/// hash map, and [`DefaultHasher::new`] is the same in each), which is the only build that reads it.
///
/// [`DefaultHasher::new`]: std::collections::hash_map::DefaultHasher::new
fn digest(view: &ResolvedView) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    format!("{view:?}").hash(&mut hasher);
    hasher.finish()
}
