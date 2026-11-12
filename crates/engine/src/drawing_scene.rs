//! The sheet scene (#4890 W10, #3890 W15): everything drawn on one sheet of a drawing as one paper
//! scene ([`Scene`]), for the sheet page and every export, so the screen and the files draw the
//! same thing. Each group paints in its kind's band ([`ObjectKind::band`]), each band over the ones
//! before it: the sheet's drawing border ([`SheetLayout::paper`]); the images placed on it
//! ([`groups`]), so a view's lines draw over a photo; each view on the sheet, its lines as
//! [`paper_group`] makes them; the dimensions and notes of those views
//! ([`crate::drawing_dimension_paper`], [`crate::drawing_note_paper`]); the sheet's tables, once they
//! join; the notes on the sheet itself; then its title block's logos, lines and projection symbol, in
//! the drawing's projection ([`TitleBlockLayout::paper`]), with its labels and values set on them
//! ([`title_block_text`]). Within a band, groups paint in the order they join: views by id, their
//! dimensions and notes by view and id. A shaded view's group holds its pictures under its lines;
//! nothing here draws their pixels ([`crate::drawing_picture`]).
//!
//! [`build`] draws a sheet at once, each view the one way: its lines worked out centred on the view,
//! then moved to its centre, so the same lines on the same centre are the same bytes however they
//! got there. It reads the model as it is: no model is evaluated, but a view of another configuration
//! evaluates a copy of the design ([`copy_for`]); the design is unchanged.
//!
//! The sheet page draws through the provider instead, which keeps each view's lines between frames:
//! [`extract`] returns the last scene at once and says which views are out of date, and [`step`]
//! brings one of them up to date, a short piece of work for the UI's tick: a view's projection with
//! its dimensions in the same swap, so new lines never show under old dimensions, or, once the views
//! are, the dimensions of one view that changed. A view moved across its sheet is moved, never
//! projected again, and an edited dimension is read again alone. A view drawn from a world state that
//! showed the same bodies, where and as they are now (`SourceKey`), is up to date: an appearance,
//! or an edit taken back, draws nothing. The provider evaluates no model either, but in the job that
//! draws a view of another configuration ([`Job::run`]): nothing the window does each frame makes
//! that view's copy, so it stays out of date until its job draws it. Its cache is the
//! session's view state, beside the drawing: never saved, never an undo step, and dropped when its
//! drawing closes, all but its views' and dimensions' lines (`drawing_scene_kept.rs`), which the
//! drawing opened again starts from.
//!
//! A note on a view rides its view's dimensions' path ([`Annotation`]): read with them, kept with
//! them, and read again by the same rules, and also after any change to the document, as its text
//! reads the drawing's and the design's fields. A note on a sheet reads no model: it is set with the
//! sheet's title block ([`sheet_notes`]).
//!
//! A deferred drawing ([`Updates::Deferred`]) draws nothing once the model has moved on from its last
//! `drawing.update`, which isn't saved: its last lines show, its views listed in `stale`, and every command
//! that reads a sheet ([`as_shown`]) reads those and lists the views and their dimensions out of date.
//!
//! [`hit`] says which of a scene's objects is under a point of the sheet, the one rule for picking
//! there: the sheet page's hover and `drawing.scene.hit`.
//!
//! A view can also be drawn beside the window (#4892 U3): [`begin`] asks for the first view out of
//! date, [`Job::run`] works its lines out on a copy of the session (on a worker thread), and
//! [`finish`] takes them only for the latest request, while nothing it was asked from has changed.
//! A result that comes too late changes nothing. [`step`] is the same three, run at once.
//!
//! Lines stand where the design places its parts: an occurrence move not captured yet is left out,
//! as dimensions leave it out.
//!
//! [`SheetLayout::paper`]: solidcraft_geom::drawing_sheet::SheetLayout::paper
//! [`groups`]: crate::drawing_pictures::groups
//! [`TitleBlockLayout::paper`]: solidcraft_geom::title_block::TitleBlockLayout::paper

use std::borrow::Borrow;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use solidcore_record::{Loop2, Region2, Seg2, Vec2};
use solidcraft_doc::bom::Tree;
use solidcraft_doc::drawing::dimension::{self, Dimension};
use solidcraft_doc::drawing::note::{Anchor, Note};
use solidcraft_doc::drawing::view::{ResolvedView, View};
use solidcraft_doc::drawing::{Drawing, Sheet, Updates};
use solidcraft_doc::drawing_view::TangentEdges;
use solidcraft_doc::{DocError, Document, ModelState, ids};
use solidcraft_geom::cell::{Part, Projection};
use solidcraft_geom::drawing_view::ViewFrame;
use solidcraft_geom::hlr::Source;
use solidcraft_geom::paper::line::{LineGroup, LineKind};
use solidcraft_geom::paper::text::{Font, TextRun, font_in};
use solidcraft_geom::paper::{DrawnFor, Group, Item, MAX_RECORDS, ObjectKind, Picture, Scene};
use solidcraft_geom::prelude::*;
use solidcraft_geom::{Aabb2, ArcMap};
use solidcraft_sketch::PaperFonts;

use crate::drawing_picture::{picture_items, view_parts};
use crate::drawing_view::{
    CHORD_MM, Choice, LineRequest, MIN_MODEL_CHORD_MM, Shown, ShownSet, Stats, Styles, Unmeasured, ViewSessions, copy_for, paper_lines,
};
use crate::params::bad;
use crate::title_block_text::{self, CellText, Set};
use crate::{EngineError, Result, Session};

#[path = "drawing_scene_kept.rs"]
mod kept;
use kept::{Kept, KeptDimension, KeptLines, KeptView};

/// The command the sheet scene answers for, in its errors.
pub const CMD: &str = "drawing.scene";

/// Numbers every scene the provider assembles, in every session, so a page never sees one number
/// for two scenes, not even across closing and opening its drawing.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Numbers every view asked of [`begin`], in every session, so a result is only ever taken by the
/// cache that asked for it: not by the one a drawing gets when it is closed and opened again.
static REQUEST: AtomicU64 = AtomicU64::new(0);

/// What became of one view of a sheet scene.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Its lines are in the scene.
    Drawn,
    /// It has nothing to measure, so it draws nothing: the sheet page shows its placeholder.
    Placeholder(Unmeasured),
    /// It can't be drawn yet, or its lines are more than a paper scene holds: why. The sheet's
    /// other views are drawn all the same.
    Failed(String),
}

impl Status {
    /// Its name in replies.
    pub fn name(&self) -> &'static str {
        match self {
            Status::Drawn => "drawn",
            Status::Placeholder(_) => "placeholder",
            Status::Failed(_) => "failed",
        }
    }

    /// Why it has no lines: a placeholder's or a failed view's reason.
    pub fn reason(&self) -> Option<String> {
        match self {
            Status::Drawn => None,
            Status::Placeholder(why) => Some(why.to_string()),
            Status::Failed(why) => Some(why.clone()),
        }
    }
}

/// One view of a sheet scene.
#[derive(Clone, Debug)]
pub struct ViewState {
    pub view: u64,
    pub status: Status,
    /// What working out its lines cost, when they were.
    pub stats: Option<Stats>,
    /// What it says beside its lines, when its status doesn't say why, as a note does: the joints or
    /// motion study its position names that are gone ([`crate::drawing_view::position_reason`]).
    pub why: Option<String>,
}

/// One dimension of a sheet scene.
#[derive(Clone, Debug, PartialEq)]
pub struct DimensionState {
    pub dimension: u64,
    pub view: u64,
    /// Where it stands with the model.
    pub status: dimension::Status,
    /// Whether its lines are in the scene. A dimension that isn't found again has none, and neither
    /// has one whose view isn't drawn.
    pub drawn: bool,
    /// Why not, when its status doesn't say.
    pub why: Option<String>,
}

/// One note of a sheet scene: on a view (or what it shows), or on the sheet itself.
#[derive(Clone, Debug, PartialEq)]
pub struct NoteState {
    pub note: u64,
    /// The view it moves with; none for a note on the sheet.
    pub view: Option<u64>,
    /// Where what it points at stands with the model: ok for a note that points at nothing.
    pub status: dimension::Status,
    /// Whether it is in the scene. A lost note keeps its text there, without a leader.
    pub drawn: bool,
    /// Why not, or why its text or leader is left out.
    pub why: Option<String>,
}

/// Where one object of a sheet's scene sits on paper, for picking and marking it. Beside the scene,
/// not in it, so no writer sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SheetObject {
    pub object: u64,
    pub kind: ObjectKind,
    /// The box round what it draws, along its lines' paths; with no lines, its boundary. Never empty
    /// ([`SheetObject::on_paper`]).
    pub bounds_mm: Aabb2,
    /// A view's boundary ([`crate::drawing_view::on_sheet`]), so a view that can't be drawn can
    /// still be picked.
    pub boundary_mm: Option<Aabb2>,
}

impl SheetObject {
    /// `object` where it sits on paper: round what it draws (`drawn`), else at its boundary. What
    /// draws nothing and has no boundary, such as a note whose one field has no value, isn't on the
    /// paper or off it, so it is no object: nothing picks it, marks it or finds it off the sheet.
    fn on_paper(object: u64, kind: ObjectKind, drawn: Aabb2, boundary_mm: Option<Aabb2>) -> Option<Self> {
        let bounds_mm = Some(drawn).filter(|b| !b.is_empty()).or(boundary_mm)?;
        Some(Self { object, kind, bounds_mm, boundary_mm })
    }
}

/// One sheet drawn at once ([`build`]).
#[derive(Clone, Debug)]
pub struct SheetScene {
    pub scene: Scene,
    /// Each view on the sheet, in id order.
    pub views: Vec<ViewState>,
    /// The sheet's border, then each view on it, then each dimension and note drawn, in paint order:
    /// each that is on paper ([`SheetObject::on_paper`]).
    pub objects: Vec<SheetObject>,
    /// Each dimension of a view on the sheet, by view and id.
    pub dimensions: Vec<DimensionState>,
    /// Each note of a view on the sheet, by view and id, then each note on the sheet, by id.
    pub notes: Vec<NoteState>,
    /// The views, dimensions and notes a deferred drawing shows as they last were ([`as_shown`]).
    pub out_of_date: Vec<u64>,
    /// What each view whose lines are in the scene drew them from: its world state and its bodies.
    drawn: Vec<(Arc<ModelState>, Arc<Owners>)>,
}

impl SheetScene {
    /// Each body the views in the scene draw, with the world state it was drawn from: what the sheet
    /// shows of the model, a deferred drawing's as it last was.
    pub fn bodies_drawn(&self) -> impl Iterator<Item = (&Arc<ModelState>, &Shown)> {
        self.drawn.iter().flat_map(|(state, owners)| owners.bodies.iter().map(move |shown| (state, shown)))
    }
}

/// Whether view `id` is one of `views` whose lines are in the scene.
fn in_scene(views: &[ViewState], id: u64) -> bool {
    views.iter().any(|view| view.view == id && view.status == Status::Drawn)
}

/// One sheet as the provider last drew it ([`extract`]).
#[derive(Clone, Debug)]
pub struct SheetView {
    pub scene: Arc<Scene>,
    /// Goes up each time `scene` changes, so the page redraws only then.
    pub generation: u64,
    /// Each view on the sheet drawn so far, in id order, as it was last drawn.
    pub views: Vec<ViewState>,
    /// The views out of date, in id order: not drawn yet, or drawn from another model or with other
    /// settings than they have now. Their last lines stay in the scene until [`step`] draws them.
    pub stale: Vec<u64>,
    /// The sheet's border, then each view and dimension drawn so far, where it was last drawn.
    pub objects: Arc<[SheetObject]>,
    /// Each dimension of a view on the sheet read so far, by view and id, as it last was.
    pub dimensions: Vec<DimensionState>,
    /// The dimensions out of date whose views aren't: [`step`] reads them once the views are done.
    pub stale_dimensions: Vec<u64>,
    /// Each note of a view on the sheet read so far, then each note on the sheet, as it last was.
    pub notes: Vec<NoteState>,
    /// The notes of views up to date that are out of date: [`step`] reads them with the dimensions.
    pub stale_notes: Vec<u64>,
    /// A title block's, a dimension's or a note's text waits for the search for a font to fall back to: the
    /// page extracts again on its next tick, and the text is set once the search has answered.
    pub waiting_for_fonts: bool,
}

/// What is under a point of a sheet ([`hit`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub object: u64,
    pub kind: ObjectKind,
    /// The point is within the band of the box's edge: where a view is grabbed rather than clicked
    /// through.
    pub on_edge: bool,
}

/// The object of `objects`, in paint order, under `point_mm` on paper: of those whose box holds the
/// point within `band_mm`, the smallest, then the topmost. A view's box is its boundary, so one that
/// can't be drawn is still there; a dimension's is round its lines. The sheet's border is never hit.
/// A point or a band that isn't a finite number, or a band below 0, hits nothing. One pass over the
/// objects, cheap enough for every hover.
pub fn hit(objects: &[SheetObject], point_mm: Vec2, band_mm: f64) -> Option<Hit> {
    if !point_mm.is_finite() || !band_mm.is_finite() || band_mm < 0.0 {
        return None;
    }
    let mut found: Option<(f64, Hit)> = None;
    for object in objects.iter().filter(|o| o.kind != ObjectKind::DrawingBorder) {
        let b = object.boundary_mm.unwrap_or(object.bounds_mm);
        if !b.inflated(band_mm).contains(point_mm) {
            continue;
        }
        let size = b.size();
        let area = size.x * size.y;
        // `<=`: of two the same size, the one painted later is on top.
        if found.as_ref().is_none_or(|(smallest, _)| area <= *smallest) {
            let to_edge = [point_mm.x - b.min.x, b.max.x - point_mm.x, point_mm.y - b.min.y, b.max.y - point_mm.y];
            let on_edge = to_edge.iter().any(|gap| gap.abs() <= band_mm);
            found = Some((area, Hit { object: object.object, kind: object.kind, on_edge }));
        }
    }
    found.map(|(_, hit)| hit)
}

/// The stroke of `items` nearest `point_mm`, within `band_mm`, and how far it is: of two as near, the
/// one painted later. Only the pieces whose box reaches the point are measured; pictures, fills and
/// text are passed over. A point that isn't a finite number is near nothing.
pub fn nearest_stroke(items: &[Item], point_mm: Vec2, band_mm: f64) -> Option<(usize, f64)> {
    let mut found: Option<(usize, f64)> = None;
    for (at, item) in items.iter().enumerate() {
        let Item::Stroke { path, .. } = item else { continue };
        for seg in path.iter().filter(|seg| seg.bounds().inflated(band_mm).contains(point_mm)) {
            let distance = seg.point_at(seg.closest_param(point_mm)).dist(point_mm);
            if distance <= band_mm && found.is_none_or(|(_, nearest)| distance <= nearest) {
                found = Some((at, distance));
            }
        }
    }
    found
}

/// A line of a view on a sheet ([`line_at`]): the body that drew it and what of the body.
#[derive(Clone, Debug, PartialEq)]
pub struct LineHit {
    pub view: u64,
    pub body: Shown,
    pub source: Source,
    /// How far the point is from it, on paper.
    pub distance_mm: f64,
}

/// The line under `point_mm` on sheet `sheet` of the drawing open, within `band_mm`: in the view
/// [`hit`] there, the nearest of its strokes ([`nearest_stroke`]). The sheet's views out of date are
/// drawn first, as the page draws them ([`step`]), so the line is one the page shows. A view that
/// can't be drawn, or isn't in the scene, has no lines. A view of another configuration evaluates a
/// copy of the design; the design is unchanged.
pub fn line_at(s: &mut Session, sheet: u64, point_mm: Vec2, band_mm: f64) -> Result<Option<LineHit>> {
    let d = s.drawing(CMD)?.clone();
    extract(s, sheet)?;
    settle(s, &d);
    let answer = extract(s, sheet)?;
    let views: Vec<SheetObject> = answer.objects.iter().filter(|o| o.kind == ObjectKind::View).copied().collect();
    let Some(view) = hit(&views, point_mm, band_mm).map(|h| h.object) else { return Ok(None) };
    if !answer.views.iter().any(|v| v.view == view && v.status == Status::Drawn) {
        return Ok(None);
    }
    let centre = views_on(&d, sheet)?.into_iter().find(|(id, ..)| *id == view).map(|(.., centre)| centre);
    let drawn = s.drawing_scenes.views.get(&view).map(|e| &e.drawn);
    let (Some(centre), Some(Drawn { group: Some(group), owners: Some(owners), .. })) = (centre, drawn) else { return Ok(None) };
    // The group is centred on the view, as it was drawn.
    let Some((at, distance_mm)) = nearest_stroke(&group.items, point_mm - centre, band_mm) else { return Ok(None) };
    let Some((body, source)) = owners.items.get(at).copied().flatten().and_then(|(shown, source)| Some((owners.bodies.get(shown)?.clone(), source)))
    else {
        return Ok(None);
    };
    Ok(Some(LineHit { view, body, source, distance_mm }))
}

/// The id of the sheet `named` names in `drawing` (its id or its one name), refused as `cmd`: the
/// sheet a command asks to draw.
pub fn sheet_id(drawing: &Drawing, named: &str, cmd: &str) -> Result<u64> {
    let at = drawing.sheet_index(named).map_err(|e| bad(cmd, e.to_string()))?;
    drawing.sheets.get(at).map(|sheet| sheet.id).ok_or_else(|| bad(cmd, format!("no sheet `{named}`")))
}

/// Sheet `sheet` of `drawing`, one of `s`'s design's drawings, drawn at once: its border, then each
/// of its views by id. A view with nothing to measure, or one that can't be drawn, is left out and
/// says why; the sheet is refused only when it can't be drawn at all.
pub fn build(s: &Session, drawing: &Drawing, sheet: u64) -> Result<SheetScene> {
    let record = sheet_of(drawing, sheet)?;
    let on = views_on(drawing, sheet)?;
    // Where the design places its parts, as the provider draws them.
    let copy = (!s.pending_moves.is_empty()).then(|| designed(s));
    let s = copy.as_ref().unwrap_or(s);
    // Worked out once for the sheet for each state of the assembly its views draw (#13048): every
    // view of that state draws from the same one. Each view's lines are placed as they are drawn, so
    // at most one view's are held beside the scene.
    let mut sessions = ViewSessions::new(s);
    let mut shown: BTreeMap<Choice, std::result::Result<ShownSet, Unmeasured>> = BTreeMap::new();
    // What each view drew its lines from, kept as it is drawn (the lines themselves aren't): the
    // state of the assembly it draws, and its bodies.
    let mut owners = Vec::new();
    let drawn = on.iter().map(|(id, view, centre)| {
        let choice = sessions.choice(view);
        let drawn = match sessions.drawing(&choice) {
            Ok(from) => draw(from, shown.entry(choice.clone()).or_insert_with(|| crate::drawing_view::shown(from)), *id, view),
            Err(why) => draw(s, &Err(why), *id, view),
        };
        owners.extend(drawn.owners.clone().map(|of| (*id, choice, of)));
        (*id, *centre, drawn)
    });
    let lettering = Lettering::of(s, drawing);
    let (text, _) = block_text(record, &lettering, &cell_texts(s, drawing, sheet), true)?;
    let (notes, _) = sheet_notes(s, drawing, sheet, &lettering, true);
    let (images, logos) = (crate::drawing_pictures::groups(drawing, sheet), crate::drawing_pictures::logo_items(drawing, sheet));
    let mut assembly = assemble(record, drawing.projection, &text, &images, &logos, drawn)?;
    // After the views, as they need the views' frames: one read for the sheet's dimensions, each on
    // the session its view drew from.
    let frames: BTreeMap<u64, ViewFrame> = assembly.placed.iter().map(|(view, (frame, _))| (*view, *frame)).collect();
    let wanted = dimensions_on(drawing, on.iter().map(|(id, ..)| *id));
    let group = |view: u64| on.binary_search_by_key(&view, |(id, ..)| *id).ok().and_then(|at| on.get(at)).map(|(_, v, _)| v.line_group.value);
    let dimensions = draw_dimensions(s, Some(&mut sessions), drawing, &wanted, |view| Some((*frames.get(&view)?, group(view)?)), true);
    assembly.add_dimensions(dimensions.iter());
    assembly.add_sheet_notes(&notes);
    let (scene, views, objects, dimensions, notes) = assembly.finish()?;
    // Drawn at once, so nothing is as it last was.
    let out_of_date = Vec::new();
    // Each drawn view's bodies with the world state of the session it drew from: a view that draws its
    // parts apart has its copy's, as the sheet shows them.
    let state_of = |choice: &Choice| Some(shown.get(choice)?.as_ref().ok()?.state.clone());
    let drawn = owners.into_iter().filter(|(id, ..)| in_scene(&views, *id)).filter_map(|(_, choice, of)| Some((state_of(&choice)?, of))).collect();
    Ok(SheetScene { scene, views, objects, dimensions, notes, out_of_date, drawn })
}

/// Sheet `sheet` of `d`, the drawing open, as a command reads it: drawn at once ([`build`]), or for a
/// deferred drawing, what its last [`update`] allows, with what that leaves behind listed.
pub fn as_shown(s: &mut Session, d: &Drawing, sheet: u64) -> Result<SheetScene> {
    if d.updates == Updates::Automatic {
        return build(s, d, sheet);
    }
    extract(s, sheet)?;
    settle(s, d);
    let shown = extract(s, sheet)?;
    // A held view's dimensions and notes are out of date with it: listed after the views, never read
    // again here against its old lines. `stale_dimensions` and `stale_notes` hold those of the views up
    // to date.
    let up_to_date = shown.stale_dimensions.into_iter().chain(shown.stale_notes);
    let annotations = out_of_date_annotations(d, &shown.stale, up_to_date);
    let out_of_date = shown.stale.iter().copied().chain(annotations).collect();
    let (scene, objects) = (Arc::unwrap_or_clone(shown.scene), shown.objects.to_vec());
    // Each view in the scene as it was last drawn, from the world state it was drawn from.
    let entries = &s.drawing_scenes.views;
    let drawn_from = |view: &ViewState| entries.get(&view.view).and_then(|entry| Some((entry.state.clone(), entry.drawn.owners.clone()?)));
    let drawn = shown.views.iter().filter(|view| view.status == Status::Drawn).filter_map(drawn_from).collect();
    Ok(SheetScene { scene, views: shown.views, objects, dimensions: shown.dimensions, notes: shown.notes, out_of_date, drawn })
}

/// Draws what is waiting on the sheet last extracted, as the sheet page would: each view once, then
/// each view's dimensions once, so a script reads what the page shows.
pub(crate) fn settle(s: &mut Session, d: &Drawing) {
    for _ in 0..=d.views.len().saturating_mul(2) {
        if !step(s) {
            break;
        }
    }
}

/// Lets the drawing open draw from the model as it is now, until that moves on: then a deferred
/// drawing holds again. [`step`] does the drawing, on whichever sheet is shown.
pub(crate) fn update(s: &mut Session) {
    s.drawing_scenes.updated = s.world_state().ok();
}

/// `d`, the drawing open, is deferred and the model has moved on from its last [`update`].
pub(crate) fn held(s: &Session, d: &Drawing) -> bool {
    s.world_state().is_ok_and(|state| s.drawing_scenes.holds(d, &state))
}

/// A view's lines for a sheet scene, centred on the view, or why it has none.
#[derive(Clone, Debug)]
struct Drawn {
    group: Option<Arc<Group>>,
    status: Status,
    stats: Option<Stats>,
    /// Its boundary, centred on the view as its lines are.
    boundary: Option<Aabb2>,
    /// The frame its lines were drawn through, for its dimensions.
    frame: Option<ViewFrame>,
    /// What drew each item of `group`, for [`line_at`].
    owners: Option<Arc<Owners>>,
    /// What it says beside its lines ([`ViewState::why`]).
    why: Option<String>,
}

/// What drew each item of a view's group: for a stroke, the body (its index among `bodies`) and what
/// of it; nothing for a picture.
#[derive(Debug)]
struct Owners {
    bodies: Vec<Shown>,
    items: Vec<Option<(usize, Source)>>,
}

/// A dimension's lines for a sheet scene, centred on its view, and how it read; or a note's.
#[derive(Clone, Debug)]
struct DrawnDimension {
    dimension: u64,
    view: u64,
    /// A dimension or a note.
    kind: ObjectKind,
    status: dimension::Status,
    group: Option<Arc<Group>>,
    /// The fonts its group's text uses.
    fonts: Vec<Font>,
    why: Option<String>,
    /// Whether its text waits for a font to fall back to: [`extract`] reads it again until it doesn't.
    pending: bool,
}

/// What a view's dimensions path reads: a dimension, or a note that moves with the view, and the view.
#[derive(Clone, Debug)]
enum Annotation {
    Dimension(Arc<Dimension>),
    Note(Arc<Note>, u64),
}

impl Annotation {
    fn id(&self) -> u64 {
        match self {
            Annotation::Dimension(record) => record.id,
            Annotation::Note(note, _) => note.id,
        }
    }

    fn view(&self) -> u64 {
        match self {
            Annotation::Dimension(record) => record.view,
            Annotation::Note(_, view) => *view,
        }
    }

    /// The same record as `other`, not only an equal one.
    fn is(&self, other: &Annotation) -> bool {
        match (self, other) {
            (Annotation::Dimension(a), Annotation::Dimension(b)) => Arc::ptr_eq(a, b),
            (Annotation::Note(a, _), Annotation::Note(b, _)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    fn is_note(&self) -> bool {
        matches!(self, Annotation::Note(..))
    }

    /// Whether what it shows reads the drawing's and the design's fields as well as the model: a
    /// note's text with a field does, so it is read again after any change to the document.
    fn reads_fields(&self) -> bool {
        matches!(self, Annotation::Note(note, _) if note.content.0.iter().any(|part| matches!(part, Part::Field { .. })))
    }
}

/// What a drawing's dimensions read and are set in: its units (the design's, when it follows them),
/// its language and its font. A dimension drawn in other ones is read again.
#[derive(Clone, Debug, PartialEq)]
struct Lettering {
    units: String,
    language: String,
    font: String,
}

impl Lettering {
    fn of(s: &Session, d: &Drawing) -> Lettering {
        Lettering { units: s.doc.drawing_units(d).to_string(), language: d.language.clone(), font: d.font.clone() }
    }
}

/// View `id`, drawing as `view` does, of the bodies `shown` holds: its lines, the hidden ones too
/// when its style draws them, sampled at
/// [`CHORD_MM`] on paper, or coarser past 100:1, where that would ask the mesh for chords finer than
/// [`MIN_MODEL_CHORD_MM`]; centred on the view ([`assemble`] moves them to its centre). Lines past
/// the paper scene's [`MAX_RECORDS`] are refused here, so no view holds more than a scene takes.
fn draw(s: &Session, shown: &std::result::Result<ShownSet, Unmeasured>, id: u64, view: &ResolvedView) -> Drawn {
    // What the view chooses to show ([`crate::drawing_view::measured`]), and its reference parts' reach.
    let measured = crate::drawing_view::measured(shown, view).map(|(set, extents)| {
        let around = crate::drawing_view::shows(&set, &view.basis, &extents, view.reference_shown_percent);
        (set, extents, around)
    });
    let placing = measured.as_ref().map(|(_, extents, around)| (*extents, *around)).map_err(|why| *why);
    let boundary = crate::drawing_view::placed(view, placing, Vec2::ZERO, CMD).ok().map(|placed| placed.boundary);
    let why = crate::drawing_view::position_reason(s, view);
    let none = |status| Drawn { group: None, status, stats: None, boundary, frame: None, owners: None, why: why.clone() };
    let styles = Styles::from(view);
    let not_drawable = crate::drawing_view::not_drawable(view.visual_style.value, view.reference_style.value);
    if let Some(why) = not_drawable.or_else(|| crate::drawing_view::not_drawable_apart(view)) {
        return none(Status::Failed(why.into()));
    }
    // Its reference parts draw too, around what the extents measure.
    let (set, extents, around) = match measured {
        Ok(measured) => measured,
        Err(why) => return none(Status::Placeholder(why)),
    };
    let tangent_edges = view.tangent_edges.value == TangentEdges::Show;
    // The margin keeps the mesh's chord at or above its floor once divided back by the factor.
    let chord_mm = CHORD_MM.max(MIN_MODEL_CHORD_MM * view.scale.value.factor() * (1.0 + 1e-9));
    let reference_hides_model = view.reference_hides_model.value;
    let (hidden, reference_hidden) = (view.visual_style.value.draws_hidden(), view.reference_style.value.draws_hidden());
    let request = LineRequest { shows: around, hidden, reference_hidden, reference_hides_model, tangent_edges, chord_mm };
    let frame = match crate::drawing_view::frame(view.basis, view.scale.value, &extents.model, Vec2::ZERO, CMD) {
        Ok(frame) => frame,
        Err(e) => return none(Status::Failed(reason(e))),
    };
    let lines = frame.boundary(&around).map_err(|e| bad(CMD, e.to_string())).and_then(|rect| {
        let mut lines = crate::drawing_view::lines(s, &set, &frame, &request, CMD)?;
        styles.leave_shaded_to_pictures(&mut lines);
        Ok((lines, rect))
    });
    match lines {
        Ok((lines, rect)) => {
            let (mut group, drew) = paper_lines(&lines, id, view.line_group.value);
            let pictures = picture_items(id, &view_parts(&set, styles), rect);
            // The pictures go under the strokes, so the owners start with one empty entry for each.
            let owner = |at: usize| lines.lines.get(at).map(|line| (line.shown, line.source));
            let items = std::iter::repeat_n(None, pictures.len()).chain(drew.into_iter().map(owner)).collect();
            group.items.splice(0..0, pictures);
            if group.records() > MAX_RECORDS {
                return none(Status::Failed(format!("its lines are more than a paper scene's {MAX_RECORDS} records: choose a smaller scale")));
            }
            let owners = Some(Arc::new(Owners { bodies: set.into_owned().bodies, items }));
            Drawn { group: Some(Arc::new(group)), status: Status::Drawn, stats: Some(lines.stats), boundary, frame: Some(frame), owners, why }
        }
        Err(e) => none(Status::Failed(reason(e))),
    }
}

/// Why a sheet leaves out a dimension or a note whose view it doesn't draw.
pub(crate) const NOT_DRAWN: &str = "its view isn't drawn";
/// Why a sheet leaves out a dimension that measures something its view has no place to draw.
pub(crate) const NO_PLACE: &str = "what it measures has no place to draw on its view";

/// `records` (each its id and its view's) of `d` as `read` gives them: each on the session its view
/// draws from in `sessions` ([`ViewSessions::read_on`]), or with none, all on `s`, which is then the
/// one view's own session (a job's copy).
fn read_with<T>(
    s: &Session,
    sessions: Option<&mut ViewSessions>,
    d: &Drawing,
    records: &[(u64, u64)],
    read: impl Fn(&solidcraft_doc::Document, &solidcraft_doc::Model, &Drawing, &[u64]) -> solidcraft_doc::Result<Vec<T>>,
) -> solidcraft_doc::Result<Vec<T>> {
    match sessions {
        Some(sessions) => sessions.read_on(d, records, read),
        None => read(&s.doc, &s.model, d, &records.iter().map(|(id, _)| *id).collect::<Vec<_>>()),
    }
}

/// The records of `wanted` read once, each by its kind on the session its view draws from
/// ([`read_with`]), each drawn through its view's frame (`None`: its view isn't drawn), in its line
/// group, in `wanted`'s order. A record that can't be read says so instead.
fn draw_dimensions(
    s: &Session,
    mut sessions: Option<&mut ViewSessions>,
    d: &Drawing,
    wanted: &[Annotation],
    frame: impl Fn(u64) -> Option<(ViewFrame, LineGroup)>,
    wait: bool,
) -> Vec<DrawnDimension> {
    let records: Vec<&Arc<Dimension>> = wanted.iter().filter_map(|a| if let Annotation::Dimension(r) = a { Some(r) } else { None }).collect();
    let notes: Vec<&Arc<Note>> = wanted.iter().filter_map(|a| if let Annotation::Note(n, _) = a { Some(n) } else { None }).collect();
    let on: Vec<(u64, u64)> = records.iter().map(|record| (record.id, record.view)).collect();
    let read = read_with(s, sessions.as_deref_mut(), d, &on, solidcraft_doc::Document::resolve_dimensions);
    let lettering = PaperFonts::new(&d.font, &d.language, wait);
    let mut noted = drawn_notes(s, sessions, d, &notes, &frame, &lettering).into_iter();
    let one = |at: usize, record: &Arc<Dimension>| match read.as_ref().map(|all| all.get(at)) {
        Ok(Some(resolved)) => {
            let paper = frame(record.view)
                .and_then(|(frame, lines)| crate::drawing_dimension_paper::paper_group(record, resolved, &frame, lines, &lettering));
            let drawn = |group, fonts, why, pending| DrawnDimension {
                dimension: record.id,
                view: record.view,
                kind: ObjectKind::Dimension,
                status: resolved.status.clone(),
                group,
                fonts,
                why,
                pending,
            };
            match paper {
                Some(paper) => drawn(Some(Arc::new(paper.group)), paper.fonts, paper.why, paper.pending),
                None => {
                    let nothing = !resolved.points.is_empty() && frame(record.view).is_some();
                    drawn(None, Vec::new(), nothing.then(|| NO_PLACE.to_string()), false)
                }
            }
        }
        Ok(None) | Err(_) => {
            let status = dimension::Status::Unmeasurable(format!("dimension {} can't be read", ids::encode(record.id)));
            let kind = ObjectKind::Dimension;
            DrawnDimension { dimension: record.id, view: record.view, kind, status, group: None, fonts: Vec::new(), why: None, pending: false }
        }
    };
    // Each dimension by its place among the dimensions read, each note in turn.
    let (mut lines, mut at) = (Vec::with_capacity(wanted.len()), 0);
    for record in wanted {
        match record {
            Annotation::Dimension(record) => {
                lines.push(one(at, record));
                at += 1;
            }
            Annotation::Note(..) => lines.extend(noted.next()),
        }
    }
    lines
}

/// The dimensions and notes of `d` a sheet shows as they last were, by id: those of each view out of
/// date (`stale_views`), as their view is, then `up_to_date`, those of the views up to date that were
/// read from another record, model, drawing of their view, lettering or revision
/// ([`Cache::stale_dimensions`]).
fn out_of_date_annotations(d: &Drawing, stale_views: &[u64], up_to_date: impl IntoIterator<Item = u64>) -> Vec<u64> {
    let held = dimensions_on(d, stale_views.iter().copied()).into_iter().map(|record| record.id());
    held.chain(up_to_date).collect()
}

/// `notes`, each on its view (`frame`, as [`draw_dimensions`] gives it), read once, as dimensions are
/// ([`read_with`]): its text with its fields read for its view's sheet, and its leader to where it
/// points now, through the view's frame. A note whose view isn't drawn has no lines; one that can't
/// be read says so.
fn drawn_notes(
    s: &Session,
    sessions: Option<&mut ViewSessions>,
    d: &Drawing,
    notes: &[&Arc<Note>],
    frame: &impl Fn(u64) -> Option<(ViewFrame, LineGroup)>,
    lettering: &PaperFonts,
) -> Vec<DrawnDimension> {
    if notes.is_empty() {
        return Vec::new();
    }
    let on: Vec<(u64, u64)> = notes.iter().map(|note| (note.id, note.anchor.view().unwrap_or_default())).collect();
    let read = read_with(s, sessions, d, &on, solidcraft_doc::Document::resolve_notes);
    let state = s.model.state();
    let mut tree = Tree::new(&s.doc, &state);
    let one = |at: usize, note: &Arc<Note>, tree: &mut Tree<'_>| {
        let view = note.anchor.view().unwrap_or_default();
        let unread = || dimension::Status::Unmeasurable(format!("note {} can't be read", ids::encode(note.id)));
        let resolved = read.as_ref().ok().and_then(|all| all.get(at));
        let status = resolved.map_or_else(unread, |r| r.anchor.status.clone());
        let base =
            DrawnDimension { dimension: note.id, view, kind: ObjectKind::Note, status, group: None, fonts: Vec::new(), why: None, pending: false };
        let (Some(resolved), Some((frame, group))) = (resolved, frame(view)) else { return base };
        let text = d
            .sheet_of(&note.anchor)
            .and_then(|at| title_block_text::read(&s.doc, tree, d, at, std::iter::once(&note.content)).pop())
            .unwrap_or_default();
        let tips: Vec<Vec2> =
            std::iter::once(&resolved.anchor).chain(&resolved.branches).filter_map(|end| end.point).map(|p| frame.to_paper(p)).collect();
        // Centred on the view, as its frame is: the scene moves it to the view's centre.
        let paper = crate::drawing_note_paper::note_group(note, &text, &tips, LineKind::Thin.width_mm(group), lettering);
        DrawnDimension { group: Some(Arc::new(paper.group)), fonts: paper.fonts, why: paper.why, pending: paper.pending, ..base }
    };
    notes.iter().enumerate().map(|(at, note)| one(at, note, &mut tree)).collect()
}

/// A note on a sheet, set with the sheet's title block: it reads no model, so it is set again only
/// when the sheet's text is ([`Assembled::reads`]).
#[derive(Clone, Debug, PartialEq)]
struct SheetNote {
    note: u64,
    group: Group,
    fonts: Vec<Font>,
    why: Option<String>,
}

/// The notes on sheet `sheet` of `d`, by id, set in `lettering` in its line group's thin lines, each
/// with its fields read for that sheet; and whether a text waits for a font to fall back to, which
/// only the window's sheet leaves to the search's thread (`wait` false).
fn sheet_notes(s: &Session, d: &Drawing, sheet: u64, lettering: &Lettering, wait: bool) -> (Vec<SheetNote>, bool) {
    let on: Vec<&Arc<Note>> = d.notes.iter().filter(|note| note.anchor == Anchor::Sheet(sheet)).collect();
    let Some(at) = d.sheets.iter().position(|one| one.id == sheet).filter(|_| !on.is_empty()) else { return (Vec::new(), false) };
    let line_mm = LineKind::Thin.width_mm(d.sheets.get(at).and_then(|one| one.line_group).unwrap_or(d.line_group));
    let state = s.model.state();
    let texts = title_block_text::read(&s.doc, &mut Tree::new(&s.doc, &state), d, at, on.iter().map(|note| &note.content));
    let fonts = PaperFonts::new(&lettering.font, &lettering.language, wait);
    let mut pending = false;
    let mut set = |(note, text): (&&Arc<Note>, String)| {
        let paper = crate::drawing_note_paper::note_group(note, &text, &[], line_mm, &fonts);
        pending |= paper.pending;
        SheetNote { note: note.id, group: paper.group, fonts: paper.fonts, why: paper.why }
    };
    let notes = on.iter().zip(texts).map(&mut set).collect();
    (notes, pending)
}

/// The dimensions and the notes of `views` of `d`, by view and id.
fn dimensions_on(d: &Drawing, views: impl Iterator<Item = u64>) -> Vec<Annotation> {
    let views: BTreeSet<u64> = views.collect();
    let dimensions = d.dimensions.iter().filter(|record| views.contains(&record.view)).map(|record| Annotation::Dimension(record.clone()));
    let notes = d.notes.iter().filter_map(|note| Some(Annotation::Note(note.clone(), note.anchor.view().filter(|view| views.contains(view))?)));
    let mut on: Vec<Annotation> = dimensions.chain(notes).collect();
    on.sort_by_key(|record| (record.view(), record.id()));
    on
}

/// A sheet's scene being put together: what [`assemble`] gives, for its dimensions to join.
struct Assembly {
    /// The sheet's size and fonts: [`Assembly::finish`] puts the groups in it.
    scene: Scene,
    /// The groups that joined, each with its paint band ([`ObjectKind::band`]), in the order they joined.
    groups: Vec<(u8, Group)>,
    /// The groups that joined, as a paper scene counts them.
    records: usize,
    /// One group checked alone on the sheet before it joins, so one past the limits leaves out only
    /// itself, and checking costs one pass over each group.
    alone: Scene,
    states: Vec<ViewState>,
    /// The objects on the sheet, each with its group's paint band, in the order they joined.
    objects: Vec<(u8, SheetObject)>,
    /// The frame and centre of each view whose lines are in the scene.
    placed: BTreeMap<u64, (ViewFrame, Vec2)>,
    dimensions: Vec<DimensionState>,
    notes: Vec<NoteState>,
}

/// Why a group stayed out of a scene.
enum LeftOut {
    TooManyRecords,
    Invalid(String),
}

impl Assembly {
    /// Puts `group`, drawn `of` a view or the sheet and whose text uses `fonts`, in the scene moved to
    /// `centre`, in its paint band, and gives the box round it there. Only a group that joins adds to
    /// the scene's fonts, and a font's records it adds count.
    fn add(&mut self, group: &Group, fonts: &[Font], centre: Vec2, of: DrawnFor) -> std::result::Result<Aabb2, LeftOut> {
        self.alone.add_fonts(fonts.iter().cloned());
        let more = group.records().saturating_add(self.scene.font_records_beyond(&self.alone.fonts));
        if self.records.saturating_add(more) > MAX_RECORDS {
            self.alone.fonts.clear();
            return Err(LeftOut::TooManyRecords);
        }
        self.alone.groups.push(moved(group, centre));
        if let Err(e) = self.alone.validate() {
            self.alone.groups.clear();
            self.alone.fonts.clear();
            return Err(LeftOut::Invalid(e.to_string()));
        }
        self.records += more;
        let bounds = self.alone.groups.first().map_or(Aabb2::EMPTY, |one| group_bounds(one, &self.alone.fonts));
        let band = group.kind.band(of);
        self.groups.extend(self.alone.groups.drain(..).map(|joined| (band, joined)));
        self.scene.add_fonts(std::mem::take(&mut self.alone.fonts));
        Ok(bounds)
    }

    /// Adds `dimensions` (and the notes of views) after the views, each moved to its view's centre.
    /// One whose view isn't in the scene or that would take it past a paper scene's limits is left out
    /// and says why.
    fn add_dimensions<'a>(&mut self, dimensions: impl Iterator<Item = &'a DrawnDimension>) {
        for one in dimensions {
            let mut why = one.why.clone();
            let mut drawn = false;
            match (self.placed.get(&one.view).map(|(_, centre)| *centre), &one.group) {
                (None, _) => why = Some(NOT_DRAWN.into()),
                (Some(centre), Some(group)) => match self.add(group, &one.fonts, centre, DrawnFor::View) {
                    Ok(bounds_mm) => {
                        drawn = true;
                        self.objects.extend(SheetObject::on_paper(one.dimension, one.kind, bounds_mm, None).map(banded(DrawnFor::View)));
                    }
                    Err(LeftOut::TooManyRecords) => {
                        why = Some(format!("with its lines the sheet is more than a paper scene's {MAX_RECORDS} records"))
                    }
                    Err(LeftOut::Invalid(e)) => why = Some(e),
                },
                (Some(_), None) => {}
            }
            let status = one.status.clone();
            match one.kind {
                ObjectKind::Note => self.notes.push(NoteState { note: one.dimension, view: Some(one.view), status, drawn, why }),
                _ => self.dimensions.push(DimensionState { dimension: one.dimension, view: one.view, status, drawn, why }),
            }
        }
    }

    /// Adds `notes`, the sheet's own, after the views' dimensions and notes. One that would take the
    /// scene past a paper scene's limits is left out and says why.
    fn add_sheet_notes(&mut self, notes: &[SheetNote]) {
        for one in notes {
            let mut why = one.why.clone();
            let drawn = match self.add(&one.group, &one.fonts, Vec2::ZERO, DrawnFor::Sheet) {
                Ok(bounds_mm) => {
                    self.objects.extend(SheetObject::on_paper(one.note, ObjectKind::Note, bounds_mm, None).map(banded(DrawnFor::Sheet)));
                    true
                }
                Err(LeftOut::TooManyRecords) => {
                    why = Some(format!("with its text the sheet is more than a paper scene's {MAX_RECORDS} records"));
                    false
                }
                Err(LeftOut::Invalid(e)) => {
                    why = Some(e);
                    false
                }
            };
            self.notes.push(NoteState { note: one.note, view: None, status: dimension::Status::Ok, drawn, why });
        }
    }

    /// The scene with its groups in paint order, checked whole, and its objects in the same order.
    fn finish(self) -> Result<(Scene, Vec<ViewState>, Vec<SheetObject>, Vec<DimensionState>, Vec<NoteState>)> {
        let mut scene = self.scene;
        scene.groups = in_paint_order(self.groups);
        scene.validate().map_err(|e| bad(CMD, e.to_string()))?;
        Ok((scene, self.states, in_paint_order(self.objects), self.dimensions, self.notes))
    }
}

/// `joined` without its bands, band by band. The sort is stable, so what shares a band keeps the
/// order it joined in.
fn in_paint_order<T>(mut joined: Vec<(u8, T)>) -> Vec<T> {
    joined.sort_by_key(|(band, _)| *band);
    joined.into_iter().map(|(_, one)| one).collect()
}

/// An object on the sheet with the paint band of its group, drawn `of` a view or the sheet.
fn banded(of: DrawnFor) -> impl Fn(SheetObject) -> (u8, SheetObject) {
    move |object| (object.kind.band(of), object)
}

/// The scene of sheet `record`: its border, then `images`, then each of `views` (its id, centre and
/// lines, in id order) moved to its centre, with its title block (`logos` under its lines), its symbol in
/// `projection` and its cells' text as [`block_text`] set it, which its band puts on top
/// ([`Assembly::finish`]). A view that would take the scene past a paper scene's limits is left out and
/// says why, and the views after it are still drawn when they fit.
fn assemble<D: Borrow<Drawn>>(
    record: &Sheet,
    projection: Projection,
    text: &Set,
    images: &[Group],
    logos: &[Item],
    views: impl Iterator<Item = (u64, Vec2, D)>,
) -> Result<Assembly> {
    let size_mm = record.sheet.size_mm().map_err(|e| bad(CMD, e.to_string()))?;
    let layout = record.border.as_ref().map(|border| border.layout(&record.sheet)).transpose().map_err(|e| bad(CMD, e.to_string()))?;
    let mut border = layout.as_ref().map_or_else(Vec::new, |layout| layout.paper(record.id));
    // The block sits on the border. It joins with the border (its text's fonts too), so no view takes
    // its records, and its band paints it over the views.
    let (block, fonts) = match (&record.title_block, &layout) {
        (Some(block), Some(layout)) => {
            let placed = block.layout(layout).map_err(|e| bad(CMD, e.to_string()))?;
            let mut group = placed.paper(record.id, projection);
            group.items.splice(0..0, logos.iter().cloned());
            group.items.extend(text.runs.iter().cloned().map(Item::Text));
            (Some(group), text.fonts.clone())
        }
        _ => (None, Vec::new()),
    };
    // The border's groups are one object, measured before the images painted with them join them:
    // an image isn't one of the objects a sheet's commands pick.
    let bounds_mm = border.iter().map(|group| group_bounds(group, &[])).fold(Aabb2::EMPTY, |all, one| all.union(&one));
    let objects: Vec<(u8, SheetObject)> =
        SheetObject::on_paper(record.id, ObjectKind::DrawingBorder, bounds_mm, None).map(banded(DrawnFor::Sheet)).into_iter().collect();
    border.extend_from_slice(images);
    border.extend(block);
    let records = border.iter().map(Group::records).fold(0usize, usize::saturating_add);
    let groups = border.into_iter().map(|group| (group.kind.band(DrawnFor::Sheet), group)).collect();
    let mut scene = Scene::new(size_mm, Vec::new());
    let alone = Scene::new(size_mm, Vec::with_capacity(1));
    // The block's fonts, each once: all they hold counts, as the table held none before.
    scene.add_fonts(fonts);
    let records = records.saturating_add(alone.font_records_beyond(&scene.fonts));
    let (states, placed, dimensions, notes) = (Vec::new(), BTreeMap::new(), Vec::new(), Vec::new());
    let mut assembly = Assembly { scene, groups, records, alone, states, objects, placed, dimensions, notes };
    for (view, centre, drawn) in views {
        let drawn: &Drawn = drawn.borrow();
        let mut status = drawn.status.clone();
        let mut lines = None;
        if let Some(group) = &drawn.group {
            match assembly.add(group, &[], centre, DrawnFor::View) {
                Ok(bounds) => {
                    lines = Some(bounds);
                    if let Some(frame) = drawn.frame {
                        assembly.placed.insert(view, (frame, centre));
                    }
                }
                Err(LeftOut::TooManyRecords) => {
                    status = Status::Failed(format!(
                        "with its lines the sheet is more than a paper scene's {MAX_RECORDS} records: put it on another sheet, or choose a smaller scale"
                    ));
                }
                Err(LeftOut::Invalid(e)) => status = Status::Failed(format!("{e}: choose a smaller scale")),
            }
        }
        let boundary_mm = drawn.boundary.map(|b| Aabb2 { min: b.min + centre, max: b.max + centre });
        assembly
            .objects
            .extend(SheetObject::on_paper(view, ObjectKind::View, lines.unwrap_or(Aabb2::EMPTY), boundary_mm).map(banded(DrawnFor::View)));
        assembly.states.push(ViewState { view, status, stats: drawn.stats, why: drawn.why.clone() });
    }
    Ok(assembly)
}

/// The box round what `group` draws: along its lines' paths and its text's outlines, in `fonts`,
/// and round its pictures.
fn group_bounds(group: &Group, fonts: &[Font]) -> Aabb2 {
    let mut bounds = Aabb2::EMPTY;
    for item in &group.items {
        let mut add = |path: &[Seg2]| path.iter().for_each(|seg| bounds = bounds.union(&seg.bounds()));
        match item {
            Item::Stroke { path, .. } => add(path),
            Item::Fill { region } => region.loops().for_each(|ring| add(&ring.segs)),
            Item::Text(run) => {
                if let Some(font) = font_in(fonts, &run.font) {
                    run.outlines(font).for_each(|ring| add(&ring.segs));
                }
            }
            Item::Picture(picture) => bounds = bounds.union(&picture.rect),
        }
    }
    bounds
}

/// `group` moved `by` on paper: each point plus `by`, exactly, so the same lines moved to the same
/// centre are the same bytes however they got there.
pub(crate) fn moved(group: &Group, by: Vec2) -> Group {
    let segs = |path: &[Seg2]| -> Vec<Seg2> {
        // A shift keeps an arc's radius and angles: no turn, no mirror.
        path.iter().map(|seg| seg.mapped(|p| p + by, ArcMap::turn(0.0, false))).collect()
    };
    let ring = |l: &Loop2| Loop2 { segs: segs(&l.segs) };
    let items = group.items.iter().map(|item| match item {
        Item::Stroke { path, closed, width_mm, pattern } => {
            Item::Stroke { path: segs(path), closed: *closed, width_mm: *width_mm, pattern: pattern.clone() }
        }
        Item::Fill { region } => Item::Fill { region: Region2 { outer: ring(&region.outer), holes: region.holes.iter().map(ring).collect() } },
        Item::Text(run) => Item::Text(TextRun { origin: run.origin + by, ..run.clone() }),
        Item::Picture(picture) => Item::Picture(Picture { rect: Aabb2 { min: picture.rect.min + by, max: picture.rect.max + by }, ..*picture }),
    });
    Group { object: group.object, kind: group.kind, items: items.collect() }
}

/// The text cells of sheet `sheet` of `d` as `s`'s design shows them ([`title_block_text::shown`]).
fn cell_texts(s: &Session, d: &Drawing, sheet: u64) -> Vec<CellText> {
    let Some(at) = d.sheets.iter().position(|x| x.id == sheet && x.title_block.is_some()) else { return Vec::new() };
    let state = s.model.state();
    title_block_text::shown(&s.doc, &mut Tree::new(&s.doc, &state), d, at)
}

/// Sheet `record`'s title block text, its cells as `cells` gives them, set on paper in `lettering`'s
/// font and language: nothing without a block. Also whether a text waits for a font to fall back
/// to, which only the window's sheet leaves to the search's thread (`wait` false).
fn block_text(record: &Sheet, lettering: &Lettering, cells: &[CellText], wait: bool) -> Result<(Set, bool)> {
    Ok(match (&record.title_block, record.title_block_layout().map_err(|e| bad(CMD, e.to_string()))?) {
        (Some(block), Some(placed)) => {
            let laid = title_block_text::lay(block, &placed, &PaperFonts::new(&lettering.font, &lettering.language, wait), cells);
            (laid.set(), laid.pending)
        }
        _ => (Set::default(), false),
    })
}

/// Sheet `sheet` of `d`.
fn sheet_of(d: &Drawing, sheet: u64) -> Result<&Sheet> {
    d.sheets.iter().map(|s| &**s).find(|s| s.id == sheet).ok_or_else(|| bad(CMD, format!("the drawing has no sheet {}", ids::encode(sheet))))
}

/// The views on sheet `sheet` of `d`, in id order, each as it draws and with its centre.
pub(crate) fn views_on(d: &Drawing, sheet: u64) -> Result<Vec<(u64, ResolvedView, Vec2)>> {
    let fail = |e: DocError| bad(CMD, e.to_string());
    let mut on: Vec<u64> = d.views.iter().filter(|v| v.sheet == sheet).map(|v| v.id).collect();
    on.sort_unstable();
    on.into_iter()
        .map(|id| -> Result<(u64, ResolvedView, Vec2)> { Ok((id, d.resolve_view(id).map_err(fail)?, d.view_centre(id).map_err(fail)?)) })
        .collect()
}

/// `e`'s message without the command's name: a view's reason in a sheet scene.
fn reason(e: EngineError) -> String {
    match e {
        EngineError::BadParams { msg, .. } => msg,
        e => e.to_string(),
    }
}

/// The provider's cache: per view, the lines it was last drawn with. The session holds one, for
/// the drawing open.
#[derive(Default)]
pub struct Cache {
    /// The drawing it is for.
    drawing: Option<u64>,
    /// The sheet last extracted: the one [`step`] brings up to date.
    sheet: Option<u64>,
    /// Each view's last drawing, by id.
    views: BTreeMap<u64, Entry>,
    /// Each dimension's last drawing, by id.
    dimensions: BTreeMap<u64, DimensionEntry>,
    /// The shown set of the world state views were last drawn from, worked out once for it.
    shown: Option<(Arc<ModelState>, std::result::Result<ShownSet, Unmeasured>)>,
    last: Option<Assembled>,
    /// The world state a deferred drawing may draw from: its last [`update`].
    updated: Option<Arc<ModelState>>,
    /// Numbers each view drawn and each read of dimensions, so a scene is assembled again only when
    /// one of its views or dimensions was.
    serial: u64,
    /// The views' lines of each drawing closed, for when it opens again. They outlive the rest.
    kept: KeptLines,
    /// The latest view asked of [`begin`]: the one result [`finish`] may take.
    requested: Option<u64>,
    /// Views drawn, for tests: a move draws none.
    #[cfg(test)]
    projections: usize,
}

/// What a view's lines show: each body of the world state by the occurrences that place it, its
/// definition's body name, whether it is a reference, and its geometry where it stands. A world state
/// with the same draws the same lines, so an appearance, or an edit taken back, draws nothing. It
/// keys the whole set, not a view's own bodies: which bodies share a mesh, so the lines' last bits,
/// hangs on all of them ([`crate::mesh_lines`]).
#[derive(Debug, PartialEq)]
pub(crate) struct SourceKey(Vec<(Vec<u64>, String, bool, u64)>);

impl SourceKey {
    /// None when there is nothing to show, or a body's geometry has no key: a view drawn from it
    /// always draws again.
    fn of(shown: &std::result::Result<ShownSet, Unmeasured>) -> Option<Arc<SourceKey>> {
        let set = shown.as_ref().ok()?;
        let body = |one: &Shown| Some((one.path.clone(), one.body.clone(), one.reference, set.body(one)?.geometry_key()?));
        set.bodies.iter().map(body).collect::<Option<Vec<_>>>().map(|bodies| Arc::new(SourceKey(bodies)))
    }
}

/// A view's last drawing, and what it was drawn from: the world state (held, so its address can't
/// be reused while it is compared), what that showed, and the view as it drew.
#[derive(Clone)]
struct Entry {
    state: Arc<ModelState>,
    shows: Option<Arc<SourceKey>>,
    view: ResolvedView,
    drawn: Drawn,
    serial: u64,
}

impl Entry {
    /// Drawn from another world state than `state` that showed `now`.
    fn shows_the_same(&self, state: &Arc<ModelState>, now: &SourceKey) -> bool {
        !Arc::ptr_eq(&self.state, state) && self.shows.as_deref() == Some(now)
    }
}

/// A dimension's last drawing, or a note's, and what it was drawn from: its record, the world state,
/// its view's drawing (by serial, which holds its frame), the drawing's lettering and, for a note,
/// the document's revision ([`Annotation::reads_fields`]).
#[derive(Clone)]
struct DimensionEntry {
    state: Arc<ModelState>,
    record: Annotation,
    view_serial: u64,
    lettering: Lettering,
    revision: u64,
    drawn: DrawnDimension,
    serial: u64,
}

/// What a scene is assembled from: the sheet's record, the drawing's projection (its title block's
/// symbol), the images and logos on the sheet, each view on it with its entry's serial (0: not drawn yet)
/// and its centre, and each dimension read so far with its entry's serial.
#[derive(PartialEq)]
struct Inputs {
    sheet: Sheet,
    projection: Projection,
    images: Vec<Group>,
    logos: Vec<Item>,
    views: Vec<(u64, u64, Vec2)>,
    dimensions: Vec<(u64, u64)>,
}

/// The last answer of [`extract`], for the sheet, drawing, design, world state, lettering and serial
/// it was for: the same six again return it as it is. Its title block's cells and their text set on
/// paper hold for the same first five ([`Assembled::reads`]), so views drawn set no text, and its
/// scene is used again for the same inputs, cells and faces.
struct Assembled {
    sheet: u64,
    drawing: Arc<Drawing>,
    doc: Arc<Document>,
    state: Arc<ModelState>,
    lettering: Lettering,
    cells: Vec<CellText>,
    text: Arc<Set>,
    /// The sheet's own notes, set with its block.
    notes: Arc<Vec<SheetNote>>,
    /// Whether a text of the block or of a note on the sheet waits for a font to fall back to: until it doesn't, the block is
    /// set again on each [`extract`], and the sheet put together with it is never reused.
    pending: bool,
    serial: u64,
    inputs: Inputs,
    answer: SheetView,
}

impl Assembled {
    /// Whether it was read from sheet `sheet` of `d`, `doc`, `state` and `lettering`, all as they are.
    fn reads(&self, sheet: u64, d: &Arc<Drawing>, doc: &Arc<Document>, state: &Arc<ModelState>, lettering: &Lettering) -> bool {
        self.sheet == sheet
            && self.lettering == *lettering
            && Arc::ptr_eq(&self.drawing, d)
            && Arc::ptr_eq(&self.doc, doc)
            && Arc::ptr_eq(&self.state, state)
    }
}

impl Cache {
    /// What [`as_shown`] reads of the drawing open, for a copy of the session that exports it
    /// ([`Session::scratch_as_shown`]): the world state its last [`update`] took, each view's and
    /// dimension's last drawing, and the serial they were numbered to, so the copy's numbers never
    /// repeat theirs. Also the lines kept for `open` when it closed, if nothing has read it since it
    /// opened again, so the copy takes them back as the session would ([`Self::take_kept`]). Not the lines
    /// kept for other drawings, the last answer or a request in flight, which only the session's own
    /// page uses; the drawing and sheet are set by the read.
    pub(crate) fn shown_copy(&self, open: Option<u64>) -> Cache {
        Cache {
            views: self.views.clone(),
            dimensions: self.dimensions.clone(),
            updated: self.updated.clone(),
            serial: self.serial,
            kept: self.kept.copy_of(open),
            ..Cache::default()
        }
    }

    /// Drawing `drawing`'s lines kept when it closed, as its views' and dimensions' entries: from a
    /// world state that has gone, so each view is out of date until [`keep_what_shows_the_same`] finds
    /// it shows the same, and each dimension is read again once its view is up to date. A held
    /// drawing draws neither, so it shows them as they were.
    fn take_kept(&mut self, drawing: u64) {
        let gone: Arc<ModelState> = Arc::default();
        let kept = self.kept.take(drawing);
        for (id, kept) in kept.views {
            self.serial += 1;
            let entry = Entry { state: gone.clone(), shows: kept.shows, view: kept.view, drawn: kept.drawn, serial: self.serial };
            self.views.insert(id, entry);
        }
        // From the same gone state, so read again: its `view_serial`, from before the close, never
        // matches its view's new one either.
        for (id, kept) in kept.dimensions {
            self.serial += 1;
            let KeptDimension { record, view_serial, lettering, drawn } = kept;
            // Its world state is gone, so it is read again whatever its revision.
            let entry = DimensionEntry { state: gone.clone(), record, view_serial, lettering, revision: 0, drawn, serial: self.serial };
            self.dimensions.insert(id, entry);
        }
    }

    /// `d` is deferred, and `state` isn't the one its last [`update`] took.
    fn holds(&self, d: &Drawing, state: &Arc<ModelState>) -> bool {
        d.updates == Updates::Deferred && !self.updated.as_ref().is_some_and(|updated| Arc::ptr_eq(updated, state))
    }

    /// Hands view `id` back to its job: it keeps its lines, drawn from a world state that has gone (as
    /// [`Self::take_kept`]'s are), so it is out of date until its job draws it and its records again.
    /// A change to the cache, so [`extract`] doesn't give its last answer again.
    fn redraw(&mut self, id: u64) {
        if let Some(entry) = self.views.get_mut(&id) {
            entry.state = Arc::default();
            self.serial += 1;
        }
    }

    /// Whether view `id`, drawing as `view` now, was last drawn so, from `state`.
    fn fresh(&self, id: u64, state: &Arc<ModelState>, view: &ResolvedView) -> bool {
        self.views.get(&id).is_some_and(|e| Arc::ptr_eq(&e.state, state) && e.view == *view)
    }

    /// What sheet `record` of `d`, holding the views `on` ([`views_on`]), is assembled from now.
    fn inputs(&self, d: &Drawing, record: &Sheet, on: &[(u64, ResolvedView, Vec2)]) -> Inputs {
        let serial = |id: &u64| self.views.get(id).map_or(0, |e| e.serial);
        let images = crate::drawing_pictures::groups(d, record.id);
        let logos = crate::drawing_pictures::logo_items(d, record.id);
        let views = on.iter().map(|(id, _, centre)| (*id, serial(id), *centre)).collect();
        let read = dimensions_on(d, on.iter().map(|(id, ..)| *id));
        let dimensions = read.iter().filter_map(|record| self.dimensions.get(&record.id()).map(|e| (record.id(), e.serial))).collect();
        Inputs { sheet: record.clone(), projection: d.projection, images, logos, views, dimensions }
    }

    /// The views of the last scene assembled, while it is still its sheet's scene: the sheet as `d`
    /// has it, holding the same views, none drawn again since. So a view in it is on that sheet.
    fn assembled(&self, d: &Drawing) -> Option<&[ViewState]> {
        let last = self.last.as_ref()?;
        let on = views_on(d, last.sheet).ok()?;
        (self.inputs(d, sheet_of(d, last.sheet).ok()?, &on) == last.inputs).then_some(last.answer.views.as_slice())
    }

    /// Keeps `records` as `lines` drew them, at the document's `revision`, numbered with the cache's
    /// serial.
    fn keep_dimensions(
        &mut self,
        records: Vec<Annotation>,
        lines: Vec<DrawnDimension>,
        state: &Arc<ModelState>,
        view_serial: u64,
        lettering: &Lettering,
        revision: u64,
    ) {
        let serial = self.serial;
        for (record, drawn) in records.into_iter().zip(lines) {
            let entry = DimensionEntry { state: state.clone(), record, view_serial, lettering: lettering.clone(), revision, drawn, serial };
            self.dimensions.insert(entry.record.id(), entry);
        }
    }

    /// The dimensions and notes of the views `on`, by view and id, to read again: each whose view is
    /// up to date, last drawn from another record, model, drawing of its view, or lettering, or a note
    /// read at another `revision`.
    fn stale_dimensions(
        &self,
        d: &Drawing,
        on: &[(u64, ResolvedView, Vec2)],
        state: &Arc<ModelState>,
        lettering: &Lettering,
        revision: u64,
    ) -> Vec<Annotation> {
        self.dimensions_where(d, on, state, lettering, revision, |current| current.is_none())
    }

    /// The dimensions of the views `on`, by view and id, read as they are now but with text that
    /// waits for its font: [`extract`] sets it, as it has no lines to draw again.
    fn waiting_dimensions(
        &self,
        d: &Drawing,
        on: &[(u64, ResolvedView, Vec2)],
        state: &Arc<ModelState>,
        lettering: &Lettering,
        revision: u64,
    ) -> Vec<Annotation> {
        self.dimensions_where(d, on, state, lettering, revision, |current| current.is_some_and(|e| e.drawn.pending))
    }

    /// The dimensions and notes of the up-to-date views `on` that `keep` takes, given each one's entry
    /// when it was read from this record, model, drawing of its view and lettering, and a note's at this
    /// `revision`.
    fn dimensions_where(
        &self,
        d: &Drawing,
        on: &[(u64, ResolvedView, Vec2)],
        state: &Arc<ModelState>,
        lettering: &Lettering,
        revision: u64,
        keep: impl Fn(Option<&DimensionEntry>) -> bool,
    ) -> Vec<Annotation> {
        let fresh = on.iter().filter(|(id, view, _)| self.fresh(*id, state, view));
        let serials: BTreeMap<u64, u64> = fresh.filter_map(|(id, ..)| self.views.get(id).map(|e| (*id, e.serial))).collect();
        let current = |record: &Annotation, serial: u64| {
            let read_now =
                |e: &&DimensionEntry| e.record.is(record) && Arc::ptr_eq(&e.state, state) && e.view_serial == serial && e.lettering == *lettering;
            self.dimensions.get(&record.id()).filter(read_now).filter(|e| !record.reads_fields() || e.revision == revision)
        };
        let mut found = dimensions_on(d, serials.keys().copied());
        found.retain(|record| serials.get(&record.view()).is_some_and(|serial| keep(current(record, *serial))));
        found
    }
}

/// Where one view of a drawing stands with the provider ([`standing`]).
#[derive(Clone, Debug, PartialEq)]
pub struct ViewStanding {
    pub view: u64,
    pub sheet: u64,
    /// `current`, `out_of_date` (not drawn yet, or drawn from another model or with other settings
    /// than it has now), `placeholder` or `failed`.
    pub status: &'static str,
    /// Why, for a placeholder or a failed view.
    pub reason: Option<String>,
}

/// Where the drawing open stands with the provider ([`standing`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Standing {
    /// Each view, in id order.
    pub views: Vec<ViewStanding>,
    /// The dimensions and notes its sheets show as they last were ([`out_of_date_annotations`]).
    pub annotations: Vec<u64>,
}

/// Each view of `d`, the drawing open, in id order, as the provider last drew it, whichever sheet it
/// is on, and the dimensions and notes shown as they last were. It reads the cache, caught up first
/// ([`catch_up`]): nothing is drawn. A view that drew but didn't fit its sheet's scene is as the last
/// assembly left it, while that is still its sheet's scene ([`Cache::assembled`]).
pub fn standing(s: &mut Session, d: &Drawing) -> Standing {
    forget_closed(s);
    let state = s.world_state().ok();
    if let Some(state) = &state {
        catch_up(s, d, d.views.iter().map(|v| v.id), state);
    }
    let lettering = Lettering::of(s, d);
    let revision = s.revision;
    let cache = &s.drawing_scenes;
    let assembled = cache.assembled(d).unwrap_or_default();
    let in_assembly = |id: u64| assembled.binary_search_by_key(&id, |v| v.view).ok().and_then(|at| assembled.get(at));
    let one = |view: &Arc<View>| {
        let fresh = d.resolve_view(view.id).is_ok_and(|resolved| state.as_ref().is_some_and(|state| cache.fresh(view.id, state, &resolved)));
        let (status, reason) = match cache.views.get(&view.id).filter(|_| fresh) {
            Some(entry) => {
                let status = in_assembly(view.id).map_or(&entry.drawn.status, |v| &v.status);
                (if *status == Status::Drawn { "current" } else { status.name() }, status.reason())
            }
            None => ("out_of_date", None),
        };
        ViewStanding { view: view.id, sheet: view.sheet, status, reason }
    };
    let views: Vec<ViewStanding> = d.views.iter().map(one).collect();
    let stale: Vec<u64> = views.iter().filter(|v| v.status == "out_of_date").map(|v| v.view).collect();
    // Every sheet's views as `extract` takes one sheet's (`views_on`), but each on its own, so one
    // that can't be placed leaves out only its own dimensions and notes.
    let on: Vec<(u64, ResolvedView, Vec2)> =
        d.views.iter().filter_map(|v| Some((v.id, d.resolve_view(v.id).ok()?, d.view_centre(v.id).ok()?))).collect();
    let stale_dimensions = state.as_ref().map_or_else(Vec::new, |state| cache.stale_dimensions(d, &on, state, &lettering, revision));
    let annotations = out_of_date_annotations(d, &stale, stale_dimensions.iter().map(Annotation::id));
    Standing { views, annotations }
}

/// Sheet `sheet` of the drawing open, as the provider last drew it: at once, as nothing is drawn
/// here. The first call for a sheet gives its border, with every view out of date.
pub fn extract(s: &mut Session, sheet: u64) -> Result<SheetView> {
    forget_closed(s);
    let d = s.drawing(CMD)?.clone();
    let state = s.world_state()?;
    let lettering = Lettering::of(s, &d);
    let on = views_on(&d, sheet)?;
    let handed_back = set_waiting_dimensions(s, &d, &on, &state, &lettering);
    let revision = s.revision;
    let last = s.drawing_scenes.last.as_ref().filter(|last| !last.pending && last.reads(sheet, &d, &s.doc, &state, &lettering));
    if let Some(last) = last
        && last.serial == s.drawing_scenes.serial
    {
        return Ok(last.answer.clone());
    }
    // The title block's cells and the sheet's notes are read and set again only for another sheet,
    // drawing, design, model or lettering (a name or a mass can change them), not for each view drawn.
    let (cells, text, notes, pending) = match last {
        Some(last) => (last.cells.clone(), last.text.clone(), last.notes.clone(), false),
        None => {
            let cells = cell_texts(s, &d, sheet);
            let (text, block_pending) = block_text(sheet_of(&d, sheet)?, &lettering, &cells, false)?;
            let (notes, notes_pending) = sheet_notes(s, &d, sheet, &lettering, false);
            (cells, Arc::new(text), Arc::new(notes), block_pending || notes_pending)
        }
    };
    let doc = s.doc.clone();
    let cache = &mut s.drawing_scenes;
    let record = sheet_of(&d, sheet)?;
    cache.sheet = Some(sheet);
    catch_up(s, &d, on.iter().map(|(id, ..)| *id), &state);
    let cache = &mut s.drawing_scenes;
    let stale = on.iter().filter(|(id, view, _)| !cache.fresh(*id, &state, view)).map(|(id, ..)| *id).collect();
    let (stale_notes, stale_dimensions): (Vec<Annotation>, Vec<Annotation>) =
        cache.stale_dimensions(&d, &on, &state, &lettering, revision).into_iter().partition(Annotation::is_note);
    let (stale_dimensions, stale_notes) = (stale_dimensions.iter().map(Annotation::id).collect(), stale_notes.iter().map(Annotation::id).collect());
    let waiting_for_fonts = pending || handed_back || !cache.waiting_dimensions(&d, &on, &state, &lettering, revision).is_empty();
    let inputs = cache.inputs(&d, record, &on);
    let (scene, views, objects, dimensions, notes_drawn, generation) = match cache.last.take() {
        // Text set while a font was looked for can differ with the same cells and faces.
        Some(last)
            if !last.pending
                && !pending
                && last.inputs == inputs
                && last.cells == cells
                && last.text.faces() == text.faces()
                && last.notes == notes =>
        {
            let answer = last.answer;
            (answer.scene, answer.views, answer.objects, answer.dimensions, answer.notes, answer.generation)
        }
        _ => {
            // Views, dimensions and notes no longer in the drawing are forgotten: each holds a world state.
            let kept: BTreeSet<u64> = d.views.iter().map(|v| v.id).collect();
            cache.views.retain(|id, _| kept.contains(id));
            cache.dimensions.retain(|id, _| d.dimension(*id).is_some() || d.notes.by_id(*id).is_some());
            let drawn = on.iter().filter_map(|(id, _, centre)| cache.views.get(id).map(|e| (*id, *centre, &e.drawn)));
            let mut assembly = assemble(record, d.projection, &text, &inputs.images, &inputs.logos, drawn)?;
            assembly.add_dimensions(inputs.dimensions.iter().filter_map(|(id, _)| cache.dimensions.get(id).map(|e| &e.drawn)));
            assembly.add_sheet_notes(&notes);
            let (scene, views, objects, dimensions, notes) = assembly.finish()?;
            (Arc::new(scene), views, objects.into(), dimensions, notes, GENERATION.fetch_add(1, Ordering::Relaxed).wrapping_add(1))
        }
    };
    let answer =
        SheetView { scene, generation, views, stale, objects, dimensions, stale_dimensions, notes: notes_drawn, stale_notes, waiting_for_fonts };
    let serial = cache.serial;
    cache.last = Some(Assembled { sheet, drawing: d, doc, state, lettering, cells, text, notes, pending, serial, inputs, answer: answer.clone() });
    Ok(answer)
}

/// Draws the first view out of date on the sheet last extracted or, with none, reads the dimensions
/// out of date of one view, and says whether more are waiting: a short piece of work for the UI's
/// tick, one view's projection or one read of a view's dimensions. Text that waits for its font is
/// no work for it: [`extract`] sets it, and its answer says so ([`SheetView::waiting_for_fonts`]).
pub fn step(s: &mut Session) -> bool {
    let Some((d, on, state)) = drawable(s) else { return false };
    let lettering = Lettering::of(s, &d);
    let waiting: Vec<(u64, ResolvedView)> =
        on.iter().filter(|(id, view, _)| !s.drawing_scenes.fresh(*id, &state, view)).map(|(id, view, _)| (*id, view.clone())).collect();
    let Some((id, view)) = waiting.first().cloned() else { return step_dimensions(s, &d, &on, &state, lettering) };
    let done = job_for(s, &d, id, view, state.clone()).run();
    finish(s, done);
    waiting.len() > 1 || !s.drawing_scenes.stale_dimensions(&d, &on, &state, &lettering, s.revision).is_empty()
}

/// What [`step`] and [`begin`] draw from: the drawing open, the views of the sheet last extracted
/// and the world state, once the cache has caught up with it ([`catch_up`]). None while a deferred
/// drawing holds, or with nothing to draw from.
fn drawable(s: &mut Session) -> Option<(Arc<Drawing>, Vec<(u64, ResolvedView, Vec2)>, Arc<ModelState>)> {
    forget_closed(s);
    let (Ok(d), Some(sheet)) = (s.drawing(CMD).cloned(), s.drawing_scenes.sheet) else { return None };
    let on = views_on(&d, sheet).ok()?;
    let state = s.world_state().ok()?;
    catch_up(s, &d, on.iter().map(|(id, ..)| *id), &state);
    (!s.drawing_scenes.holds(&d, &state)).then_some((d, on, state))
}

/// A shown set, and the world state it was worked out for.
type ShownFor = (Arc<ModelState>, std::result::Result<ShownSet, Unmeasured>);

/// One view's lines, worked out beside the window: [`begin`] asks for them, [`Job::run`] works them
/// out anywhere, and [`finish`] takes them.
pub struct Job {
    /// The session as its view draws the design: as it places its parts, or in another state of it.
    /// For a view of another configuration, the design its copy is made from.
    copy: Session,
    /// Why the view can't be drawn, when the copy it draws from can't be made ([`copy_for`]).
    refused: Option<Unmeasured>,
    /// What a view of another configuration draws ([`ViewSessions::choice`]): its copy evaluates the
    /// design, so [`Job::run`] makes it, off the window's thread.
    later: Option<Choice>,
    /// Whether its view draws the design as it places its parts, whose shown set the cache keeps.
    placed: bool,
    /// The drawing open when the job was asked for: its records of the view's dimensions and notes.
    drawing: Arc<Drawing>,
    ticket: Ticket,
    /// The cache's shown set, reused when it is for the copy's world state.
    shown: Option<ShownFor>,
}

/// What a job was asked from: its request, the view as it resolved, and the session's world state,
/// revision and the drawing's lettering then.
struct Ticket {
    request: u64,
    view: u64,
    resolved: ResolvedView,
    state: Arc<ModelState>,
    revision: u64,
    lettering: Lettering,
}

/// A job's work, for [`finish`]: the view's lines and what they show, its dimensions and notes through
/// its new frame, and the shown set they were drawn from.
pub struct Done {
    ticket: Ticket,
    drawn: Drawn,
    shows: Option<Arc<SourceKey>>,
    dimensions: Vec<Annotation>,
    lines: Vec<DrawnDimension>,
    /// The shown set of the design as it places its parts, for the cache; none from another state.
    shown: Option<ShownFor>,
}

impl Job {
    /// Works the view's lines, dimensions and notes out: the one long piece of work, on any thread.
    pub fn run(self) -> Done {
        let Job { copy, refused, later, placed, drawing, ticket, shown } = self;
        // A view of another configuration: its copy, made here ([`copy_for`]). A panic while it is made
        // fails the view, as `Session::execute` fails a command, and the view is then up to date: its
        // job isn't asked for again until the design changes.
        let made = later.map(|choice| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let made = copy_for(&copy, &choice);
                // Planted after the copy is evaluated, so its results are already in the shared cache.
                #[cfg(test)]
                copy_panics::check();
                made
            }))
        });
        let panicked = matches!(made, Some(Err(_)));
        let (copy, refused) = match made {
            Some(Ok(Ok(made))) => (made, None),
            Some(Ok(Err(why))) => (copy, Some(why)),
            // Drawn as its placeholder, then failed, below.
            Some(Err(_)) => (copy, Some(Unmeasured::NotConfigured)),
            None => (copy, refused),
        };
        // A design whose parts can't be placed draws its views as placeholders (`shown` says why).
        let state = copy.world_state().unwrap_or_else(|_| copy.model.state());
        let shown = match (refused, shown) {
            (Some(why), _) => Err(why),
            (None, Some((was, shown))) if Arc::ptr_eq(&was, &state) => shown,
            _ => crate::drawing_view::shown(&copy),
        };
        let mut drawn = draw(&copy, &shown, ticket.view, &ticket.resolved);
        if panicked {
            drawn.status = Status::Failed(COPY_PANICKED.into());
        }
        let shows = SourceKey::of(&shown);
        // The view's dimensions and notes through its new frame, taken in the same swap as its lines.
        let frame = drawn.frame.map(|frame| (frame, ticket.resolved.line_group.value));
        let dimensions = dimensions_on(&drawing, std::iter::once(ticket.view));
        // `copy` is the session the view draws from (its row's copy, or the design when the row can't be
        // put on it), so its records are read on it.
        let lines = draw_dimensions(&copy, None, &drawing, &dimensions, |_| frame, false);
        let shown = placed.then_some((state, shown));
        Done { ticket, drawn, shows, dimensions, lines, shown }
    }
}

/// Why a view whose job panicked while making its copy is failed ([`Job::run`]).
const COPY_PANICKED: &str = "internal error while making the copy of the design it draws";

/// Makes the copies [`Job::run`] makes panic on this thread, for tests of what a panic there does.
#[cfg(test)]
pub(crate) mod copy_panics {
    use std::cell::Cell;

    thread_local! {
        static ON: Cell<bool> = const { Cell::new(false) };
    }

    pub(crate) fn check() {
        if ON.with(Cell::get) {
            panic!("a planted panic in a view's copy");
        }
    }

    /// Puts it back when dropped, also when the test panics.
    struct Restore(bool);

    impl Drop for Restore {
        fn drop(&mut self) {
            ON.with(|on| on.set(self.0));
        }
    }

    /// Runs `f` with every copy a job makes panicking.
    pub(crate) fn with<R>(f: impl FnOnce() -> R) -> R {
        let _restore = Restore(ON.with(|on| on.replace(true)));
        f()
    }
}

/// Asks for the first view out of date on the sheet last extracted, to draw beside the window, as
/// [`step`] would draw it: none when nothing is waiting, or while a deferred drawing holds. One at a
/// time: asking again replaces the earlier request.
pub fn begin(s: &mut Session) -> Option<Job> {
    let (d, on, state) = drawable(s)?;
    let (id, view, _) = on.into_iter().find(|(id, view, _)| !s.drawing_scenes.fresh(*id, &state, view))?;
    Some(job_for(s, &d, id, view, state))
}

/// Asks for view `id` of drawing `d`, resolved as `view`, of the world `state`: the latest request
/// from now on.
fn job_for(s: &mut Session, d: &Arc<Drawing>, id: u64, view: ResolvedView, state: Arc<ModelState>) -> Job {
    let request = REQUEST.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    s.drawing_scenes.requested = Some(request);
    let lettering = Lettering::of(s, d);
    let choice = ViewSessions::new(s).choice(&view);
    let placed = choice.is_placed();
    let (copy, refused, later, shown) = if placed {
        (designed(s), None, None, s.drawing_scenes.shown.take())
    } else if choice.evaluates() {
        (designed(s), None, Some(choice), None)
    } else {
        match copy_for(s, &choice) {
            Ok(copy) => (copy, None, None, None),
            Err(why) => (designed(s), Some(why), None, None),
        }
    };
    let ticket = Ticket { request, view: id, resolved: view, state, revision: s.revision, lettering };
    Job { copy, refused, later, placed, drawing: d.clone(), ticket, shown }
}

/// Takes a job's work into the cache when it is for its latest request and nothing it was asked from
/// has changed since: the drawing, the model and the view as it resolves. Otherwise it is dropped and
/// nothing changes. Says whether it was taken.
pub fn finish(s: &mut Session, done: Done) -> bool {
    forget_closed(s);
    let Done { ticket, drawn, shows, dimensions, lines, shown } = done;
    if s.drawing_scenes.requested != Some(ticket.request) {
        return false;
    }
    s.drawing_scenes.requested = None;
    // Any change to the document since, even one undone, moves the revision: so the drawing, its
    // dimensions' records and its lettering (units, font, language) are as they were.
    let holds = s.revision == ticket.revision
        && s.world_state().is_ok_and(|state| Arc::ptr_eq(&state, &ticket.state))
        && s.drawing(CMD).ok().and_then(|d| d.resolve_view(ticket.view).ok()).is_some_and(|view| view == ticket.resolved);
    if !holds {
        return false;
    }
    let cache = &mut s.drawing_scenes;
    if shown.is_some() {
        cache.shown = shown;
    }
    cache.serial += 1;
    let serial = cache.serial;
    let entry = Entry { state: ticket.state.clone(), shows, view: ticket.resolved, drawn, serial };
    cache.views.insert(ticket.view, entry);
    cache.keep_dimensions(dimensions, lines, &ticket.state, serial, &ticket.lettering, ticket.revision);
    #[cfg(test)]
    {
        cache.projections += 1;
    }
    true
}

/// Forgets the request [`begin`] made: its lines, when they come, are dropped.
pub fn cancel(s: &mut Session) {
    s.drawing_scenes.requested = None;
}

/// A copy of `s` as the design places its parts: an occurrence move not captured yet is left out.
pub(crate) fn designed(s: &Session) -> Session {
    let mut copy = s.scratch();
    copy.pending_moves.clear();
    copy
}

/// Reads the dimensions and notes out of date of the first view that has any, through its frame,
/// and says whether another view's are waiting.
fn step_dimensions(s: &mut Session, d: &Drawing, on: &[(u64, ResolvedView, Vec2)], state: &Arc<ModelState>, lettering: Lettering) -> bool {
    let revision = s.revision;
    let stale = s.drawing_scenes.stale_dimensions(d, on, state, &lettering, revision);
    let Some(view) = stale.first().map(Annotation::view) else { return false };
    // Its copy evaluates the design, which only its job does: its job reads them again with its lines.
    if by_their_jobs(s, on).contains(&view) {
        s.drawing_scenes.redraw(view);
        return true;
    }
    let (these, others): (Vec<_>, Vec<_>) = stale.into_iter().partition(|record| record.view() == view);
    let entry = s.drawing_scenes.views.get(&view);
    let frame = entry.and_then(|e| Some((e.drawn.frame?, e.view.line_group.value)));
    let view_serial = entry.map_or(0, |e| e.serial);
    let lines = draw_dimensions(s, Some(&mut ViewSessions::new(s)), d, &these, |_| frame, false);
    let cache = &mut s.drawing_scenes;
    cache.serial += 1;
    cache.keep_dimensions(these, lines, state, view_serial, &lettering, revision);
    !others.is_empty()
}

/// The views of `on` whose copy evaluates the design ([`Choice::evaluates`]): the window's frame paths
/// never read their records, as that would make the copy; their jobs do.
fn by_their_jobs(s: &Session, on: &[(u64, ResolvedView, Vec2)]) -> BTreeSet<u64> {
    let sessions = ViewSessions::new(s);
    on.iter().filter(|(_, view, _)| sessions.choice(view).evaluates()).map(|(id, ..)| *id).collect()
}

/// Brings the cache to `d`, the drawing open, and `state`, before anything reads or draws from it:
/// the lines kept when `d` closed come back, and each of the views `on` that shows the same takes
/// `state` ([`keep_what_shows_the_same`]). [`extract`], [`step`], [`begin`] and [`standing`] all start
/// here, so each says the same of a view.
fn catch_up(s: &mut Session, d: &Drawing, on: impl Iterator<Item = u64> + Clone, state: &Arc<ModelState>) {
    let cache = &mut s.drawing_scenes;
    if cache.drawing != Some(d.id) {
        cache.take_kept(d.id);
        cache.drawing = Some(d.id);
    }
    keep_what_shows_the_same(s, on, state);
}

/// Sets the text of each dimension and note on `on` that waited for its font, once the search has
/// answered: one read of those alone, kept only when its text is set, so a text still waiting
/// changes nothing. Says whether it handed any back to its view's job: that text still waits, though
/// its view, now out of date, no longer counts it.
fn set_waiting_dimensions(s: &mut Session, d: &Drawing, on: &[(u64, ResolvedView, Vec2)], state: &Arc<ModelState>, lettering: &Lettering) -> bool {
    let revision = s.revision;
    let waiting = s.drawing_scenes.waiting_dimensions(d, on, state, lettering, revision);
    // Those of a view whose copy evaluates the design are read again by its job, with its lines.
    let jobs = by_their_jobs(s, on);
    let (waiting, theirs): (Vec<Annotation>, Vec<Annotation>) = waiting.into_iter().partition(|record| !jobs.contains(&record.view()));
    for record in &theirs {
        s.drawing_scenes.redraw(record.view());
    }
    let handed_back = !theirs.is_empty();
    if waiting.is_empty() {
        return handed_back;
    }
    let cache = &s.drawing_scenes;
    let frame = |view: u64| cache.views.get(&view).and_then(|e| Some((e.drawn.frame?, e.view.line_group.value)));
    let lines = draw_dimensions(s, Some(&mut ViewSessions::new(s)), d, &waiting, frame, false);
    let set: Vec<(Annotation, DrawnDimension)> = waiting.into_iter().zip(lines).filter(|(_, drawn)| !drawn.pending).collect();
    let cache = &mut s.drawing_scenes;
    for (record, drawn) in set {
        let Some(view_serial) = cache.dimensions.get(&record.id()).map(|e| e.view_serial) else { continue };
        cache.serial += 1;
        cache.keep_dimensions(vec![record], vec![drawn], state, view_serial, lettering, revision);
    }
    handed_back
}

/// Each view of `on` drawn from another world state that showed what `state` shows takes `state`:
/// drawn as it draws now, it is then up to date ([`Cache::fresh`]) and draws nothing. What a view
/// shows is what it draws: the design as it places its parts, whose shown set is worked out once and
/// kept for [`step`] to draw from, or its exploded view's (#13048), one for each exploded view behind.
/// Each is worked out only when a view drawing it is behind. A view of another configuration is left
/// behind: its copy evaluates the design, which only its job does ([`Job::run`]).
fn keep_what_shows_the_same(s: &mut Session, on: impl Iterator<Item = u64> + Clone, state: &Arc<ModelState>) {
    // Each with what it says now: a joint deleted may leave its picture as it was.
    let says = |view: &ResolvedView| crate::drawing_view::position_reason(s, view);
    // Each by the key of the session it draws from, so a view of the active row is the design's.
    let behind: Vec<(u64, Choice, Option<String>)> = {
        let sessions = ViewSessions::new(s);
        on.filter_map(|id| {
            let entry = s.drawing_scenes.views.get(&id).filter(|e| e.shows.is_some() && !Arc::ptr_eq(&e.state, state))?;
            Some((id, sessions.choice(&entry.view), says(&entry.view)))
        })
        .collect()
    };
    let mut now: BTreeMap<Choice, Option<Arc<SourceKey>>> = BTreeMap::new();
    if behind.iter().any(|(_, choice, _)| choice.is_placed()) {
        let shown = match s.drawing_scenes.shown.take() {
            Some((was, shown)) if Arc::ptr_eq(&was, state) => shown,
            _ => crate::drawing_view::shown(s),
        };
        now.insert(Choice::default(), SourceKey::of(&shown));
        s.drawing_scenes.shown = Some((state.clone(), shown));
    }
    {
        let mut sessions = ViewSessions::new(s);
        for (_, choice, _) in &behind {
            if !now.contains_key(choice) && !choice.evaluates() {
                let shown = sessions.drawing(choice).and_then(crate::drawing_view::shown);
                now.insert(choice.clone(), SourceKey::of(&shown));
            }
        }
    }
    let cache = &mut s.drawing_scenes;
    for (id, choice, why) in behind {
        let Some(Some(now)) = now.get(&choice) else { continue };
        if let Some(entry) = cache.views.get_mut(&id).filter(|e| e.shows_the_same(state, now)) {
            entry.state = state.clone();
            // A new reason is a new drawing of the view, so the sheet is assembled again.
            if entry.drawn.why != why {
                cache.serial += 1;
                (entry.serial, entry.drawn.why) = (cache.serial, why);
            }
        }
    }
}

/// Drops the cache once its drawing isn't the one open: closing a drawing (or opening another) ends
/// its scenes, and the world states they hold. Its views' and dimensions' lines are kept, for when
/// it opens again, and forgotten with the drawing. [`Session::execute`] calls it after every command.
pub(crate) fn forget_closed(s: &mut Session) {
    let cache = &mut s.drawing_scenes;
    if let Some(drawing) = cache.drawing.filter(|d| s.open_drawing != Some(*d)) {
        let mut kept = std::mem::take(&mut cache.kept);
        let views = std::mem::take(&mut cache.views).into_iter().map(|(id, e)| (id, KeptView { view: e.view, drawn: e.drawn, shows: e.shows }));
        let dimensions = std::mem::take(&mut cache.dimensions)
            .into_iter()
            .map(|(id, e)| (id, KeptDimension { record: e.record, view_serial: e.view_serial, lettering: e.lettering, drawn: e.drawn }));
        kept.keep(drawing, Kept { views: views.collect(), dimensions: dimensions.collect() });
        *cache = Cache { kept, ..Cache::default() };
    }
    cache.kept.retain(|drawing| s.doc.drawings.iter().any(|d| d.id == drawing));
}

#[cfg(test)]
#[path = "drawing_scene_tests.rs"]
pub(crate) mod tests;
