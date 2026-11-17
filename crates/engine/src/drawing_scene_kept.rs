//! What the sheet scene's provider keeps of a drawing once it closes: each view's last lines, how the
//! view drew them and what they showed, and each dimension's last reading, so the drawing opened
//! again starts from them, each note's of a view, which rides with them, and, for the drawing closed
//! last, each view's pictures' last pixels, shown stale until drawn again. No world state is held, so a
//! closed drawing keeps no model. The views' lines are also kept in the design's file
//! ([`solidcraft_io::SheetView`]): a drawing opened from it starts from them as from lines kept in
//! memory ([`from_file`], [`saved`]), with their pictures' pixels and the looks they were drawn from.

use std::collections::BTreeMap;
use std::sync::Arc;

use solidcraft_doc::drawing::Drawing;
use solidcraft_doc::drawing::view::ResolvedView;
use solidcraft_geom::drawing_view::ViewFrame;
use solidcraft_io::{FiledSheet, SheetPixels, SheetView, ShownBody};

use super::{
    Annotation, Drawn, DrawnDimension, DrawnFill, DrawnFrom, DrawnSketch, Lettering, MAX_PICTURE_PIXEL_TESTS, Pixels, SketchContext, SourceInputs,
    SourceKey, Status, ThreadInfo, Vec2,
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
/// (picking draws them again, [`super::line_at`]); their dimensions are read again. Each view's
/// pictures the file kept come with them ([`pixels`]).
pub(super) fn from_file(d: &Drawing, filed: &FiledSheet) -> Kept {
    let kept = |saved: SheetView| -> Option<(u64, KeptView)> {
        let view = d.resolve_view(saved.view).ok().filter(|view| view.flat.is_none() && digest(view) == saved.resolved)?;
        let frame = ViewFrame::new(view.basis, view.scale.value, saved.model_centre, Vec2::ZERO).ok()?;
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
        };
        let pixels = saved.pictures.into_iter().filter_map(|picture| pixels(&drawn, picture)).collect();
        Some((saved.view, KeptView { view, drawn, shows: Some(Arc::new(SourceKey(shows))), pixels }))
    };
    Kept { views: solidcraft_io::sheet_views(filed).into_iter().filter_map(kept).collect(), ..Kept::default() }
}

/// A picture's pixels the file kept, for the view `drawn` from the file: when the view draws that
/// picture and they fill its grid at their dpi ([`super::window_grid`]), within the pixel tests a
/// view's pictures may cost. Drawn from looks numbered as the file says, for the drawing the view is
/// given when it opens ([`super::Cache::take_kept`]).
fn pixels(drawn: &Drawn, picture: SheetPixels) -> Option<(u64, Pixels)> {
    let SheetPixels { source, looks, dpi, tests, size, rows } = picture;
    let drawn_picture = super::pictures_of(drawn).find(|p| p.source == source)?;
    let grid = (dpi.is_finite() && dpi > 0.0).then(|| super::window_grid(drawn_picture, dpi).ok()).flatten()?;
    if [grid.width, grid.height] != size || tests > MAX_PICTURE_PIXEL_TESTS {
        return None;
    }
    let from = DrawnFrom { serial: 0, looks: Arc::default(), dpi: Some(dpi) };
    Some((source, Pixels { grid, rows: Ok(rows?), from, tests, filed: Some(looks) }))
}

/// View `id` of `d` as its file keeps it, when it is current: drawn as `d` draws it now, from a model
/// that showed `now`, with `pictures`. A flat pattern view's lines are quick to draw again, and aren't
/// kept.
pub(super) fn saved(
    d: &Drawing,
    id: u64,
    view: &ResolvedView,
    drawn: &Drawn,
    shows: Option<&SourceKey>,
    now: &SourceKey,
    pictures: Vec<SheetPixels>,
) -> Option<SheetView> {
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
        pictures,
    })
}

/// A view as it draws, or the bodies' looks, as a number: the same in every run of one build (their
/// `Debug` text holds no hash map, and [`DefaultHasher::new`] is the same in each), which is the only
/// build that reads it.
///
/// [`DefaultHasher::new`]: std::collections::hash_map::DefaultHasher::new
pub(super) fn digest(value: &impl std::fmt::Debug) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    format!("{value:?}").hash(&mut hasher);
    hasher.finish()
}
