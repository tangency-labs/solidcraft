//! The sheet scene (#4890 W10, #3890 W15): everything drawn on one sheet of a drawing as one paper
//! scene ([`Scene`]), for the sheet page and every export, so the screen and the files draw the
//! same thing. Each group paints in its kind's band ([`ObjectKind::band`]), each band over the ones
//! before it: the sheet's drawing border ([`SheetLayout::paper`]); the images placed on it
//! ([`groups`]), so a view's lines draw over a photo; each view on the sheet, its lines as
//! [`paper_group`] makes them; the labels of the views placed apart from their projection place
//! ([`Drawing::label_text`]), and the dimensions, notes and balloons of those views
//! ([`crate::drawing_dimension_paper`], [`crate::drawing_note_paper`], [`crate::drawing_balloon_paper`]); the sheet's parts lists and revision
//! tables, each where its record places it, with its words, read with the title block's text
//! ([`crate::drawing_table_paper`]); the notes and revision tags on the sheet itself
//! ([`crate::drawing_revision_paper`]); then its title block's logos, lines and projection symbol, in
//! the drawing's projection ([`title_block_paper`]), with its labels and values set on them
//! ([`title_block_text`]). Within a band, groups paint in the order they join: views by id, their
//! dimensions, notes and balloons by view and id. A shaded view's group holds its pictures under its lines;
//! [`build`] draws no pixels: each writer asks for them ([`crate::drawing_picture`]).
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
//! reads the drawing's and the design's fields. A note or a revision tag on a sheet reads no model: it
//! is set with the sheet's title block ([`sheet_texts`]).
//! The provider also draws each shaded view's pictures for the page, once its lines and dimensions
//! are up to date: [`step`] draws one band of one picture at a time, on the grid the sheet's dpi
//! gives it, and keeps it in the view's entry, centred on the view as its lines are. So a move draws
//! nothing again. A picture is current while its view's drawing and the bodies' geometry and
//! appearance are as they were (`Looks`). Otherwise [`extract`] hands the last one on, marked stale,
//! until the new one is done: never a half-drawn one ([`SheetView::pictures`]).
//!
//! A balloon (#3874) rides the same path as one more kind: read with its view, kept and read again
//! by the same rules, and also after any change to the document, as its number reads a parts list and
//! the design's parts ([`balloon_rows`]). Its circle, number and leader are
//! [`crate::drawing_balloon_paper`]'s.
//!
//! A deferred drawing ([`Updates::Deferred`]) draws nothing once the model has moved on from its last
//! `drawing.update`, which isn't saved: its last lines show, its views listed in `stale`, and every command
//! that reads a sheet ([`as_shown`]) reads those and lists the views and their dimensions out of date.
//!
//! [`hit`] says which of a scene's objects is under a point of the sheet, the one rule for picking
//! there: the sheet page's hover and `drawing.scene.hit`.
//!
//! The provider's work comes in one order (`Next`): a view out of date, then dimensions, then the
//! sheet's fills and sketches, then a picture's setup, then its bands. The long kinds can also be
//! done beside the window (#4892 U3): [`begin`] asks for a view's lines or a picture's setup when
//! one is next, [`Job::run`] works it out on a copy of the session (on a worker thread), and
//! [`finish`] takes it only for the latest request, while nothing it was asked from has changed. A
//! result that comes too late changes nothing. [`step`] does whichever comes next, a long one run
//! at once.
//!
//! Lines stand where the design places its parts: an occurrence move not captured yet is left out,
//! as dimensions leave it out.
//!
//! [`SheetLayout::paper`]: solidcraft_geom::drawing_sheet::SheetLayout::paper
//! [`groups`]: crate::drawing_pictures::groups
//! [`title_block_paper`]: solidcraft_doc::drawing::projection_symbol::title_block_paper

use std::borrow::Borrow;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use solidcore_record::{Loop2, Region2, Seg2, Vec2};
use solidcraft_doc::bom::{Tree, Unlisted};
use solidcraft_doc::drawing::balloon::{Balloon, RowRead, balloon_rows};
use solidcraft_doc::drawing::dimension::{self, Dimension};
use solidcraft_doc::drawing::note::{Anchor, Note};
use solidcraft_doc::drawing::view::{FlatOf, ResolvedView, View, ViewKind, ViewPlace};
use solidcraft_doc::drawing::{Drawing, FillRegionStatus, ResolvedSketchFill, Sheet, SheetSketch, SketchFill, Updates};
use solidcraft_doc::drawing_view::TangentEdges;
use solidcraft_doc::{DocError, Document, ModelState, ThreadInfo, expr::ParamDef, ids};
use solidcraft_geom::cell::{Part, Projection};
use solidcraft_geom::drawing_detail::{Boundary, Shape};
use solidcraft_geom::drawing_view::{Scale, ViewFrame, is_paper_coordinate};
use solidcraft_geom::hlr::Source;
use solidcraft_geom::paper::line::{LineGroup, LineKind};
use solidcraft_geom::paper::text::{Font, TextRun, font_in};
use solidcraft_geom::paper::{Clip, DrawnFor, Group, Item, MAX_RECORDS, ObjectKind, Picture, Scene};
use solidcraft_geom::pixel_grid::{GridWindow, PixelGrid, PixelGridError};
use solidcraft_geom::prelude::*;
use solidcraft_geom::{Aabb2, ArcMap};
use solidcraft_render::job::MAX_PICTURE_PIXEL_TESTS;
use solidcraft_render::transmission::Transmission;
use solidcraft_render::{FaceAppearance, ProductScene};
use solidcraft_sketch::PaperFonts;
use solidcraft_viewport::surface::OpaqueSurface;

use crate::drawing_picture::{PictureRows, over_budget, picture_items, view_parts, view_pictures};
use crate::drawing_table_paper::SheetTables;
use crate::drawing_view::{
    CHORD_MM, Choice, LineRequest, MIN_MODEL_CHORD_MM, PreparedBodies, Shown, ShownSet, Stats, Styles, Unmeasured, ViewSessions, copy_for,
    paper_lines,
};
use crate::params::bad;
use crate::title_block_text::{self, CellText, Set};
use crate::{EngineError, Result, Session};

pub mod fill;
#[path = "drawing_scene_kept.rs"]
mod kept;
use kept::{Kept, KeptFill, KeptLines, KeptSketch};
pub mod sketch;

/// The command the sheet scene answers for, in its errors.
pub const CMD: &str = "drawing.scene";

/// Numbers every scene the provider assembles, in every session, so a page never sees one number
/// for two scenes, not even across closing and opening its drawing.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Numbers every view asked of [`begin`], in every session, so a result is only ever taken by the
/// cache that asked for it: not by the one a drawing gets when it is closed and opened again.
static REQUEST: AtomicU64 = AtomicU64::new(0);

/// The most pixels the page draws one picture with: 64 MB of rows. A finer one is drawn at the
/// sheet's dpi divided by the least whole number that keeps it within this, so it still lies on its
/// lines.
const WINDOW_PICTURE_PIXELS: u64 = 16 << 20;

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
    /// What it says beside its lines, when its status doesn't say why, as a note does: the features its
    /// configuration row fails to build, and the joints or motion study its position names that are
    /// gone ([`crate::drawing_view::state_reason`]).
    pub why: Option<String>,
    /// Its label, when it draws one ([`Drawing::label_text`]).
    pub label: Option<LabelState>,
    /// A section view's hatching, per body it hatches ([`crate::drawing_view::hatches`]).
    pub hatches: Vec<HatchState>,
}

/// One body's hatching in a section view (#13046).
#[derive(Clone, Debug, PartialEq)]
pub struct HatchState {
    /// The body's name.
    pub body: String,
    /// Its lines' angle on paper, in radians.
    pub angle: f64,
    /// Its lines' spacing on paper, in millimetres.
    pub spacing_mm: f64,
    /// Painted solid: too thin to hatch.
    pub filled: bool,
    /// How many hatch lines it drew.
    pub strokes: usize,
}

/// A view's label in a sheet scene (#13044).
#[derive(Clone, Debug, PartialEq)]
pub struct LabelState {
    /// Whether it is in the scene.
    pub drawn: bool,
    /// Why not, or why its text is left out.
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

/// One table on the sheet: drawn, or why not.
#[derive(Clone, Debug, PartialEq)]
pub struct TableState {
    pub table: u64,
    /// Whether its lines are in the scene.
    pub drawn: bool,
    /// Whether it can't be read or laid out, rather than only left out for room.
    pub failed: bool,
    /// Why it isn't drawn.
    pub why: Option<String>,
}

/// One balloon of a view on a sheet.
#[derive(Clone, Debug, PartialEq)]
pub struct BalloonState {
    pub balloon: u64,
    pub view: u64,
    /// Where the edge its arrow ends on stands with the model.
    pub status: dimension::Status,
    /// What it reads of its parts list: its number, or why it has none.
    pub row: RowRead,
    /// Whether it is in the scene. A lost balloon keeps its circle and number, without a leader.
    pub drawn: bool,
    /// Why not, or why its number or leader is left out.
    pub why: Option<String>,
}

/// One revision tag on a sheet: whether it is in the scene, with why not, or why its text is left out.
#[derive(Clone, Debug, PartialEq)]
pub struct TagState {
    pub tag: u64,
    pub drawn: bool,
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

/// What became of one authored sheet sketch. Writer support is checked separately.
#[derive(Clone, Debug, PartialEq)]
pub enum SketchStatus {
    Drawn,
    Failed(String),
}

impl SketchStatus {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Drawn => "drawn",
            Self::Failed(_) => "failed",
        }
    }
}

/// One sheet sketch, independently of projected-view status.
#[derive(Clone, Debug)]
pub struct SketchState {
    pub sketch: u64,
    pub status: SketchStatus,
}

/// What became of a drawing-owned fill. A lost association retains visible
/// geometry but is refused by exports, independently of its sketch's strokes.
#[derive(Clone, Debug, PartialEq)]
pub enum FillStatus {
    Drawn,
    LostBoundary(String),
    Failed(String),
}

impl FillStatus {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Drawn => "drawn",
            Self::LostBoundary(_) => "lost_boundary",
            Self::Failed(_) => "failed",
        }
    }
}

#[derive(Clone, Debug)]
pub struct FillState {
    pub fill: u64,
    pub sketch: u64,
    pub status: FillStatus,
}

/// One sheet drawn at once ([`build`]).
#[derive(Clone, Debug)]
pub struct SheetScene {
    pub scene: Scene,
    /// Each view on the sheet, in id order.
    pub views: Vec<ViewState>,
    /// The sheet's border, then each view on it, then each dimension, note, balloon and table drawn, in paint order:
    /// each that is on paper ([`SheetObject::on_paper`]).
    pub objects: Vec<SheetObject>,
    /// Each dimension of a view on the sheet, by view and id.
    pub dimensions: Vec<DimensionState>,
    /// Each sketch on the sheet, in id order.
    pub sketches: Vec<SketchState>,
    /// Sheet-owned fills, by drawing-global id.
    pub fills: Vec<FillState>,
    /// Each note of a view on the sheet, by view and id, then each note on the sheet, by id.
    pub notes: Vec<NoteState>,
    /// Each balloon of a view on the sheet, by view and id.
    pub balloons: Vec<BalloonState>,
    /// Each table on the sheet, by id.
    pub tables: Vec<TableState>,
    /// Each revision tag on the sheet, in the drawing's order.
    pub tags: Vec<TagState>,
    /// The views, dimensions, notes, balloons, sketches and fills a deferred drawing shows as they last
    /// were ([`as_shown`]), each with its kind as the scene knew it, so no reader works the kind out again
    /// from the id.
    pub out_of_date: Vec<(u64, ObjectKind)>,
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
    /// Each sketch drawn so far, in id order.
    pub sketches: Vec<SketchState>,
    /// Sketches not solved from their current records and source definitions yet.
    pub stale_sketches: Vec<u64>,
    /// Sheet-owned fills, by drawing-global id.
    pub fills: Vec<FillState>,
    pub stale_fills: Vec<u64>,
    /// Each note of a view on the sheet read so far, then each note on the sheet, as it last was.
    pub notes: Vec<NoteState>,
    /// The notes of views up to date that are out of date: [`step`] reads them with the dimensions.
    pub stale_notes: Vec<u64>,
    /// Each balloon of a view on the sheet read so far, by view and id, as it last was.
    pub balloons: Vec<BalloonState>,
    /// The balloons of views up to date that are out of date: [`step`] reads them with the dimensions.
    pub stale_balloons: Vec<u64>,
    /// Each table on the sheet, by id, as it last was.
    pub tables: Vec<TableState>,
    /// Each revision tag on the sheet, in the drawing's order, as it last was.
    pub tags: Vec<TagState>,
    /// A title block's, a dimension's, a note's, a balloon's or a table's text waits for the search for a font to fall back to: the
    /// page extracts again on its next tick, and the text is set once the search has answered.
    pub waiting_for_fonts: bool,
    /// The pixels of each picture in `scene` drawn so far ([`SheetPicture`]), by view and bottom first.
    /// Made again for each answer, so a picture done shows without the scene changing.
    pub pictures: Arc<[SheetPicture]>,
    /// A picture is out of date: [`step`] is drawing it, so the page keeps ticking.
    pub drawing_pictures: bool,
}

/// The pixels of one picture of a sheet's scene, as the page last drew them ([`step`]). The page
/// finds it by the `source` of an [`Item::Picture`] and draws `rows` over `grid`'s rectangle at the
/// item's opacity. A new picture is new `rows`, so the page can key its texture by their address.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetPicture {
    pub source: u64,
    pub view: u64,
    /// Where on the sheet its pixels lie: its rectangle at the sheet's dpi, or coarser for a large one.
    pub grid: PixelGrid,
    /// Its pixels, `grid.width` by `grid.height`: straight alpha sRGB RGBA, top row first. None
    /// before it is first drawn, or when it can't be drawn (`why`).
    pub rows: Option<Arc<[u8]>>,
    /// The rows are its last picture, from before its view or its bodies changed: a new one is
    /// being drawn.
    pub stale: bool,
    /// Why it can't be drawn.
    pub why: Option<String>,
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
    /// The stroke as the view draws it, in paper millimetres from the sheet's lower-left corner: the
    /// edge projected, which a Dimension tool reads to tell a straight edge from a round one.
    pub path: Vec<Seg2>,
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
    let path = match group.items.get(at) {
        // A shift keeps an arc's radius and angles, as [`moved`] has it.
        Some(Item::Stroke { path, .. }) => path.iter().map(|seg| seg.mapped(|p| p + centre, ArcMap::turn(0.0, false))).collect(),
        _ => Vec::new(),
    };
    Ok(Some(LineHit { view, body, source, distance_mm, path }))
}

/// The id of the sheet `named` names in `drawing` (its id or its one name), refused as `cmd`: the
/// sheet a command asks to draw.
pub fn sheet_id(drawing: &Drawing, named: &str, cmd: &str) -> Result<u64> {
    let at = drawing.sheet_index(named).map_err(|e| bad(cmd, e.to_string()))?;
    drawing.sheets.get(at).map(|sheet| sheet.id).ok_or_else(|| bad(cmd, format!("no sheet `{named}`")))
}

/// The last drawn boundary relative to the view centre, when available for a thumbnail.
pub fn cached_view_boundary(s: &Session, drawing: u64, view: u64) -> Option<Aabb2> {
    let cache = &s.drawing_scenes;
    if cache.drawing != Some(drawing) {
        return None;
    }
    cache.views.get(&view)?.drawn.boundary
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
    // view of that state draws from the same one, and lends the next the bodies it prepared for exact
    // lines (#13218). Each view's lines are placed as they are drawn, so at most one view's are held
    // beside the scene.
    let mut sessions = ViewSessions::new(s);
    let mut shown: BTreeMap<Choice, std::result::Result<ShownSet, Unmeasured>> = BTreeMap::new();
    let mut prepared: BTreeMap<Choice, PreparedBodies> = BTreeMap::new();
    // What each view drew its lines from, kept as it is drawn (the lines themselves aren't): the
    // state of the assembly it draws, and its bodies.
    let mut owners = Vec::new();
    let drawn = on.iter().map(|(id, view, centre)| {
        let choice = sessions.choice(view);
        let drawn = match sessions.drawing(&choice) {
            Ok((from, failing)) => {
                let shown = shown.entry(choice.clone()).or_insert_with(|| crate::drawing_view::shown(from));
                draw(from, failing, shown, prepared.entry(choice.clone()).or_default(), *id, view)
            }
            Err(why) => draw(s, &[], &Err(why), &mut PreparedBodies::default(), *id, view),
        };
        owners.extend(drawn.owners.clone().map(|of| (*id, choice, of)));
        (*id, *centre, drawn)
    });
    let lettering = Lettering::of(s, drawing);
    let (text, _) = block_text(record, &lettering, &cell_texts(s, drawing, sheet), true)?;
    let (texts, _) = sheet_texts(s, drawing, sheet, &lettering, true);
    let (labels, _) = view_labels(drawing, drawing.views.iter().map(|v| &**v).filter(|v| v.sheet == sheet), &lettering, true);
    let tables = sheet_tables(s, drawing, record, &lettering, true);
    let (images, logos) = (crate::drawing_pictures::groups(drawing, sheet), crate::drawing_pictures::logo_items(drawing, sheet));
    let mut assembly = assemble(record, drawing.projection, &text, &images, &logos, drawn)?;
    // After the views, as they need the views' frames: one read for the sheet's dimensions, each on
    // the session its view drew from.
    let frames: BTreeMap<u64, ViewFrame> = assembly.placed.iter().map(|(view, (frame, _))| (*view, *frame)).collect();
    let wanted = dimensions_on(drawing, on.iter().map(|(id, ..)| *id));
    let group = |view: u64| on.binary_search_by_key(&view, |(id, ..)| *id).ok().and_then(|at| on.get(at)).map(|(_, v, _)| v.line_group.value);
    let dimensions = draw_dimensions(s, Some(&mut sessions), drawing, &wanted, |view| Some((*frames.get(&view)?, group(view)?)), true);
    assembly.add_dimensions(dimensions.iter());
    assembly.add_labels(&labels);
    // Tables paint in their own band, under the sheet's own notes, so a note placed on one stays readable.
    let tables = assembly.add_tables(&tables);
    assembly.add_sheet_texts(&texts);
    let records = sketches_on(drawing, sheet);
    let sketched = records.iter().map(|record| (record.id, draw_sketch(s, drawing, record)));
    let fills = fills_on(drawing, sheet);
    let mut regions = BTreeMap::new();
    for record in &fills {
        regions.entry(record.sketch).or_insert_with(|| drawing.resolve_sheet_fill_regions(&s.doc, record.sketch).map_err(|e| e.to_string()));
    }
    let filled = fills.iter().map(|record| (record.id, draw_fill(drawing, record, regions.get(&record.sketch))));
    assembly.add_fills(filled);
    assembly.add_sketches(sketched);
    let fills = std::mem::take(&mut assembly.fills);
    let sketches = std::mem::take(&mut assembly.sketches);
    let tags = std::mem::take(&mut assembly.tags);
    let (scene, views, objects, dimensions, notes, balloons) = assembly.finish()?;
    // Drawn at once, so nothing is as it last was.
    let out_of_date = Vec::new();
    // Each drawn view's bodies with the world state of the session it drew from: a view that draws its
    // parts apart has its copy's, as the sheet shows them.
    let state_of = |choice: &Choice| Some(shown.get(choice)?.as_ref().ok()?.state.clone());
    let drawn = owners.into_iter().filter(|(id, ..)| in_scene(&views, *id)).filter_map(|(_, choice, of)| Some((state_of(&choice)?, of))).collect();
    Ok(SheetScene { scene, views, objects, dimensions, notes, balloons, tables, tags, out_of_date, drawn, sketches, fills })
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
    // A held view's dimensions, notes and balloons are out of date with it: listed after the views,
    // never read again here against its old lines. `stale_dimensions`, `stale_notes` and
    // `stale_balloons` hold those of the views up to date, each list one kind, as do the sheet's
    // `stale_sketches` and `stale_fills`.
    let of = |listed: Vec<u64>, kind: ObjectKind| listed.into_iter().map(move |id| (id, kind));
    let notes_and_balloons = of(shown.stale_notes, ObjectKind::Note).chain(of(shown.stale_balloons, ObjectKind::Balloon));
    let up_to_date = of(shown.stale_dimensions, ObjectKind::Dimension).chain(notes_and_balloons);
    let annotations = out_of_date_annotations(d, &shown.stale, up_to_date);
    let on_the_sheet = of(shown.stale_sketches, ObjectKind::Sketch).chain(of(shown.stale_fills, ObjectKind::Fill));
    let out_of_date = shown.stale.iter().map(|&view| (view, ObjectKind::View)).chain(annotations).chain(on_the_sheet).collect();
    let (scene, objects) = (Arc::unwrap_or_clone(shown.scene), shown.objects.to_vec());
    // Each view in the scene as it was last drawn, from the world state it was drawn from.
    let entries = &s.drawing_scenes.views;
    let drawn_from = |view: &ViewState| entries.get(&view.view).and_then(|entry| Some((entry.state.clone(), entry.drawn.owners.clone()?)));
    let drawn = shown.views.iter().filter(|view| view.status == Status::Drawn).filter_map(drawn_from).collect();
    let (dimensions, notes, balloons, tables) = (shown.dimensions, shown.notes, shown.balloons, shown.tables);
    Ok(SheetScene {
        scene,
        views: shown.views,
        objects,
        dimensions,
        notes,
        balloons,
        tables,
        tags: shown.tags,
        out_of_date,
        drawn,
        sketches: shown.sketches,
        fills: shown.fills,
    })
}

/// Draws what is waiting on the sheet last extracted, as the sheet page would: each view once, then
/// each view's dimensions once, so a script reads what the page shows.
pub(crate) fn settle(s: &mut Session, d: &Drawing) {
    for _ in 0..=d.views.len().saturating_mul(2).saturating_add(d.sketches.len()).saturating_add(d.fills.len()) {
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

#[derive(Clone, Debug)]
struct DrawnSketch {
    group: Option<Group>,
    status: SketchStatus,
    /// The view it is drawn through: its strokes are in that view's plane until the sheet is put
    /// together ([`Assembly::add_sketches`]). None for a sheet's own, already on paper.
    view: Option<u64>,
}

/// Solve a read-only copy, then convert exact authored geometry: a sheet's sketch in physical paper
/// mm, a view's in its view's plane, with its view's line width.
fn draw_sketch(s: &Session, drawing: &Drawing, record: &SheetSketch) -> DrawnSketch {
    let id = record.id;
    let sheet = drawing.sketch_sheet(record);
    let view = record.owner.view();
    let line_group = match view {
        Some(view) => drawing.resolve_view(view).ok().map(|resolved| resolved.line_group.value),
        None => drawing.sheets.iter().find(|on| Some(on.id) == sheet).and_then(|on| on.line_group),
    };
    let width_mm = LineKind::Visible.width_mm(line_group.unwrap_or(drawing.line_group));
    let result =
        drawing.solve_sheet_sketch(&s.doc, id).map_err(|e| bad(CMD, e.to_string())).and_then(|solved| sketch::paper_group(&solved, width_mm));
    match result {
        Ok(group) => DrawnSketch { group: Some(group), status: SketchStatus::Drawn, view },
        Err(e) => DrawnSketch {
            view,
            group: None,
            status: SketchStatus::Failed(format!(
                "drawing {} sheet {} sketch {}: {}",
                ids::encode(drawing.id),
                sheet.map(ids::encode).unwrap_or_default(),
                ids::encode(id),
                reason(e)
            )),
        },
    }
}

#[derive(Clone, Debug)]
struct DrawnFill {
    sketch: u64,
    group: Option<Group>,
    status: FillStatus,
}

type FillRegions = std::result::Result<Vec<ResolvedSketchFill>, String>;

fn draw_fill(drawing: &Drawing, record: &SketchFill, regions: Option<&FillRegions>) -> DrawnFill {
    let result = regions
        .ok_or_else(|| "fill region result is absent".to_string())
        .and_then(|regions| regions.as_ref().map_err(Clone::clone))
        .and_then(|regions| regions.iter().find(|region| region.id == record.id).ok_or_else(|| "selected fill result is absent".to_string()))
        .and_then(|resolved| fill::paper_group(drawing, resolved).map(|group| (group, &resolved.status)).map_err(reason));
    match result {
        Ok((group, status)) => DrawnFill {
            sketch: record.sketch,
            group: Some(group),
            status: match status {
                FillRegionStatus::Current => FillStatus::Drawn,
                FillRegionStatus::LostBoundary(why) => FillStatus::LostBoundary(why.clone()),
            },
        },
        Err(why) => DrawnFill {
            sketch: record.sketch,
            group: None,
            status: FillStatus::Failed(format!(
                "drawing {} sketch {} fill {}: {why}",
                ids::encode(drawing.id),
                ids::encode(record.sketch),
                ids::encode(record.id)
            )),
        },
    }
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
    /// A section view's hatching ([`ViewState::hatches`]).
    hatches: Vec<HatchState>,
}

/// What drew each item of a view's group: for a stroke, the body (its index among `bodies`) and what
/// of it; nothing for a picture.
#[derive(Debug)]
struct Owners {
    bodies: Vec<Shown>,
    items: Vec<Option<(usize, Source)>>,
}

/// A dimension's lines for a sheet scene, centred on its view, and how it read; or a note's or a
/// balloon's.
#[derive(Clone, Debug)]
struct DrawnDimension {
    dimension: u64,
    view: u64,
    /// A dimension, a note or a balloon.
    kind: ObjectKind,
    /// A balloon's read of its parts list; none for the others.
    row: Option<RowRead>,
    status: dimension::Status,
    group: Option<Arc<Group>>,
    /// The fonts its group's text uses.
    fonts: Vec<Font>,
    why: Option<String>,
    /// Whether its text waits for a font to fall back to: [`extract`] reads it again until it doesn't.
    pending: bool,
}

/// What a view's dimensions path reads: a dimension, a note that moves with the view (and the view),
/// or a balloon.
#[derive(Clone, Debug)]
enum Annotation {
    Dimension(Arc<Dimension>),
    Note(Arc<Note>, u64),
    Balloon(Arc<Balloon>),
}

impl Annotation {
    fn id(&self) -> u64 {
        match self {
            Annotation::Dimension(record) => record.id,
            Annotation::Note(note, _) => note.id,
            Annotation::Balloon(balloon) => balloon.id,
        }
    }

    fn view(&self) -> u64 {
        match self {
            Annotation::Dimension(record) => record.view,
            Annotation::Note(_, view) => *view,
            Annotation::Balloon(balloon) => balloon.view,
        }
    }

    /// The same record as `other`, not only an equal one.
    fn is(&self, other: &Annotation) -> bool {
        match (self, other) {
            (Annotation::Dimension(a), Annotation::Dimension(b)) => Arc::ptr_eq(a, b),
            (Annotation::Note(a, _), Annotation::Note(b, _)) => Arc::ptr_eq(a, b),
            (Annotation::Balloon(a), Annotation::Balloon(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    fn kind(&self) -> ObjectKind {
        match self {
            Annotation::Dimension(_) => ObjectKind::Dimension,
            Annotation::Note(..) => ObjectKind::Note,
            Annotation::Balloon(_) => ObjectKind::Balloon,
        }
    }

    /// Whether what it shows reads the document as well as the model, so it is read again after any
    /// change to it: a note's text with a field reads the drawing's and the design's fields, and a
    /// balloon's number its parts list and the design's parts.
    fn reads_document(&self) -> bool {
        match self {
            Annotation::Dimension(_) => false,
            Annotation::Note(note, _) => note.content.0.iter().any(|part| matches!(part, Part::Field { .. })),
            Annotation::Balloon(_) => true,
        }
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

/// View `id`, drawing as `view` does, of the bodies `shown` holds, from `s`, where its configuration
/// row fails to build `failing` features the design builds: its lines, the hidden ones too
/// when its style draws them, sampled at
/// [`CHORD_MM`] on paper, or coarser past 100:1, where that would ask the mesh for chords finer than
/// [`MIN_MODEL_CHORD_MM`]; centred on the view ([`assemble`] moves them to its centre). Lines past
/// the paper scene's [`MAX_RECORDS`] are refused here, so no view holds more than a scene takes.
/// The bodies its exact lines need come from `prepared`, as the sheet's other views left them.
fn draw(
    s: &Session,
    failing: &[String],
    shown: &std::result::Result<ShownSet, Unmeasured>,
    prepared: &mut PreparedBodies,
    id: u64,
    view: &ResolvedView,
) -> Drawn {
    if let Some(of) = &view.flat {
        return draw_flat(s, id, view, of);
    }
    // What the view chooses to show ([`crate::drawing_view::measured`]), and its reference parts' reach.
    let measured = crate::drawing_view::measured(shown, view).map(|(set, extents)| {
        let around = crate::drawing_view::shows(&set, &view.basis, &extents, view.reference_shown_percent);
        (set, extents, around)
    });
    let placing = measured.as_ref().map(|(_, extents, around)| (*extents, *around)).map_err(|why| *why);
    let boundary = crate::drawing_view::placed(view, placing, Vec2::ZERO, CMD).ok().map(|placed| placed.boundary);
    // A section view also says which of its parts it draws uncut.
    let uncut = measured.as_ref().ok().and_then(|(set, ..)| set.uncut.clone());
    let why = match (crate::drawing_view::state_reason(s, view, failing), uncut) {
        (Some(state), Some(uncut)) => Some(format!("{state}; {uncut}")),
        (state, uncut) => state.or(uncut),
    };
    let none = |status| Drawn { group: None, status, stats: None, boundary, frame: None, owners: None, why: why.clone(), hatches: Vec::new() };
    let styles = Styles::from(view);
    let not_drawable = crate::drawing_view::not_cut_yet(view).or_else(|| crate::drawing_view::not_detailed_yet(view));
    if let Some(why) = not_drawable.or_else(|| crate::drawing_view::not_drawable_apart(view)) {
        return none(Status::Failed(why.into()));
    }
    // Its reference parts draw too, around what the extents measure.
    let (set, extents, around) = match measured {
        Ok(measured) => measured,
        Err(why) => return none(Status::Placeholder(why)),
    };
    // A section's cut bodies are its own: the sheet's other views keep sharing theirs.
    let mut own = PreparedBodies::default();
    let prepared = if view.cut.is_some() { &mut own } else { prepared };
    let tangent_edges = view.tangent_edges.value == TangentEdges::Show;
    // The margin keeps the mesh's chord at or above its floor once divided back by the factor.
    let chord_mm = CHORD_MM.max(MIN_MODEL_CHORD_MM * view.scale.value.factor() * (1.0 + 1e-9));
    let reference_hides_model = view.reference_hides_model.value;
    let (hidden, reference_hidden) = (view.visual_style.value.draws_hidden(), view.reference_style.value.draws_hidden());
    let request = LineRequest { shows: around, hidden, reference_hidden, reference_hides_model, tangent_edges, chord_mm };
    let frame = match crate::drawing_view::view_frame(view, &extents.model, Vec2::ZERO, CMD) {
        Ok(frame) => frame,
        Err(e) => return none(Status::Failed(reason(e))),
    };
    // A detail view shows only its region: its boundary is the region's.
    let around = view.detail.map_or(around, |region| region.shape().bounds());
    let lines = frame.boundary(&around).map_err(|e| bad(CMD, e.to_string())).and_then(|rect| {
        let mut lines = crate::drawing_view::lines(s, &set, &frame, &request, prepared, CMD)?;
        styles.leave_shaded_to_pictures(&mut lines);
        let hatched = match &view.cut {
            Some(cut) => crate::drawing_view::hatches(&set, cut, &frame, view.line_group.value),
            None => crate::drawing_view::Hatching::default(),
        };
        Ok((lines, rect, hatched))
    });
    match lines {
        Ok((lines, rect, hatched)) => {
            let (group, drew) = paper_lines(&lines, id, view.line_group.value);
            // A detail view's lines are cut to its window, each line on its own, so each part keeps
            // the line's owner.
            let (mut group, drew) = match view.detail {
                Some(region) => match cut_to_window(&group, &drew, &crate::drawing_view::detail_window(&region, &frame)) {
                    Ok(cut) => cut,
                    Err(e) => return none(Status::Failed(e)),
                },
                None => (group, drew),
            };
            // The pictures, then the hatching, go under the strokes, so the owners start with one empty
            // entry for each.
            let mut under = picture_items(id, &view_parts(&set, styles), rect);
            under.extend(hatch_items(&hatched.bodies, view.line_group.value));
            let owner = |at: usize| lines.lines.get(at).map(|line| (line.shown, line.source));
            let items = std::iter::repeat_n(None, under.len()).chain(drew.into_iter().map(|at| at.and_then(owner))).collect();
            group.items.splice(0..0, under);
            // A body the hatcher refused is drawn unhatched, and the view says so.
            let refused = hatched.refused.iter().map(|(at, why)| {
                let name = set.bodies.get(*at).map_or("a body", |b| b.body.as_str());
                format!("{name} isn't hatched: {why}")
            });
            // A body hatched in doubt, too.
            let doubted = hatched.doubted.iter().map(|(at, why)| {
                let name = set.bodies.get(*at).map_or("a body", |b| b.body.as_str());
                format!("{name}: {why}")
            });
            let says = why.clone().into_iter().chain(refused).chain(doubted).reduce(|a, b| format!("{a}; {b}"));
            let hatches = hatched
                .bodies
                .iter()
                .map(|h| HatchState {
                    body: set.bodies.get(h.shown).map(|b| b.body.clone()).unwrap_or_default(),
                    angle: h.angle,
                    spacing_mm: h.spacing_mm,
                    filled: h.filled,
                    strokes: h.strokes.len(),
                })
                .collect();
            if group.records() > MAX_RECORDS {
                return none(Status::Failed(format!("its lines are more than a paper scene's {MAX_RECORDS} records: choose a smaller scale")));
            }
            let owners = Some(Arc::new(Owners { bodies: set.into_owned().bodies, items }));
            Drawn {
                group: Some(Arc::new(group)),
                status: Status::Drawn,
                stats: Some(lines.stats),
                boundary,
                frame: Some(frame),
                owners,
                why: says,
                hatches,
            }
        }
        Err(e) => none(Status::Failed(reason(e))),
    }
}

/// `group`'s items cut to `window`, one at a time ([`solidcraft_geom::paper::clip::clipped`]), and
/// for each part the line of `drew` its item drew.
fn cut_to_window(group: &Group, drew: &[Option<usize>], window: &Shape) -> std::result::Result<(Group, Vec<Option<usize>>), String> {
    let clip = Clip::from(*window);
    let mut items = Vec::with_capacity(group.items.len());
    let mut owners = Vec::with_capacity(drew.len());
    for (at, item) in group.items.iter().enumerate() {
        let one = Group { object: group.object, kind: group.kind, items: vec![item.clone()] };
        let cut = solidcraft_geom::paper::clip::clipped(&one, &clip).map_err(|e| format!("its region can't be cut: {e}"))?;
        owners.extend(std::iter::repeat_n(drew.get(at).copied().flatten(), cut.items.len()));
        items.extend(cut.items);
    }
    Ok((Group { items, ..group.clone() }, owners))
}

/// A section view's hatching as paper items: each filled body's cut faces painted solid, then each
/// other body's hatch lines, thin, each its own stroke.
fn hatch_items(hatched: &[crate::drawing_view::BodyHatch], group: LineGroup) -> Vec<Item> {
    let width_mm = LineKind::Thin.width_mm(group);
    let fills = hatched.iter().filter(|h| h.filled).flat_map(|h| h.regions.iter().cloned().map(|region| Item::Fill { region }));
    let strokes = hatched.iter().flat_map(|h| &h.strokes).map(|seg| Item::Stroke { path: vec![*seg], closed: false, width_mm, pattern: None });
    fills.chain(strokes).collect()
}

/// Flat pattern view `id`, drawing as `view` does, of its body's flat `of` ([`crate::drawing_view::flat`]):
/// centred on the view as [`draw`]'s lines are. Its lines name no body, so nothing is picked on them.
fn draw_flat(s: &Session, id: u64, view: &ResolvedView, of: &FlatOf) -> Drawn {
    let flat = crate::drawing_view::flat(s, of, CMD);
    let placing = crate::drawing_view::flat_measured(&flat);
    let boundary = crate::drawing_view::placed(view, placing, Vec2::ZERO, CMD).ok().map(|placed| placed.boundary);
    let none = |status| Drawn { group: None, status, stats: None, boundary, frame: None, owners: None, why: None, hatches: Vec::new() };
    if let Some(why) = crate::drawing_view::not_drawable_flat(view) {
        return none(Status::Failed(why.into()));
    }
    let flat = match flat {
        Ok(flat) => flat,
        Err(e) => return none(Status::Failed(reason(e))),
    };
    let frame = match crate::drawing_view::frame(view.basis, view.scale.value, &flat.model, Vec2::ZERO, CMD) {
        Ok(frame) => frame,
        Err(e) => return none(Status::Failed(reason(e))),
    };
    let group = crate::drawing_view::flat_group(&flat, &frame, id, view.line_group.value);
    if group.records() > MAX_RECORDS {
        return none(Status::Failed(format!("its lines are more than a paper scene's {MAX_RECORDS} records: choose a smaller scale")));
    }
    Drawn {
        group: Some(Arc::new(group)),
        status: Status::Drawn,
        stats: None,
        boundary,
        frame: Some(frame),
        owners: None,
        why: None,
        hatches: Vec::new(),
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
    let balloons: Vec<&Arc<Balloon>> = wanted.iter().filter_map(|a| if let Annotation::Balloon(b) = a { Some(b) } else { None }).collect();
    let on: Vec<(u64, u64)> = records.iter().map(|record| (record.id, record.view)).collect();
    let read = read_with(s, sessions.as_deref_mut(), d, &on, solidcraft_doc::Document::resolve_dimensions);
    let lettering = PaperFonts::new(&d.font, &d.language, wait);
    let mut noted = drawn_notes(s, sessions.as_deref_mut(), d, &notes, &frame, &lettering).into_iter();
    let mut ballooned = drawn_balloons(s, sessions, d, &balloons, &frame, &lettering).into_iter();
    let one = |at: usize, record: &Arc<Dimension>| match read.as_ref().map(|all| all.get(at)) {
        Ok(Some(resolved)) => {
            // On a view that draws none of the model's edges, it has no place to draw.
            let placed = !d.view(record.view).is_some_and(|view| crate::drawing_view::no_model_edges(view).is_some());
            let paper = frame(record.view)
                .filter(|_| placed)
                .and_then(|(frame, lines)| crate::drawing_dimension_paper::paper_group(record, resolved, &frame, lines, &lettering));
            let drawn = |group, fonts, why, pending| DrawnDimension {
                dimension: record.id,
                view: record.view,
                kind: ObjectKind::Dimension,
                row: None,
                status: resolved.status.clone(),
                group,
                fonts,
                why,
                pending,
            };
            match paper {
                Some(paper) => drawn(Some(Arc::new(paper.group)), paper.fonts, paper.why, paper.pending),
                None => {
                    let nothing = !placed || (!resolved.points.is_empty() && frame(record.view).is_some());
                    drawn(None, Vec::new(), nothing.then(|| NO_PLACE.to_string()), false)
                }
            }
        }
        Ok(None) | Err(_) => {
            let status = dimension::Status::Unmeasurable(format!("dimension {} can't be read", ids::encode(record.id)));
            let kind = ObjectKind::Dimension;
            DrawnDimension {
                dimension: record.id,
                view: record.view,
                kind,
                row: None,
                status,
                group: None,
                fonts: Vec::new(),
                why: None,
                pending: false,
            }
        }
    };
    // Each dimension by its place among the dimensions read, each note and balloon in turn.
    let (mut lines, mut at) = (Vec::with_capacity(wanted.len()), 0);
    for record in wanted {
        match record {
            Annotation::Dimension(record) => {
                lines.push(one(at, record));
                at += 1;
            }
            Annotation::Note(..) => lines.extend(noted.next()),
            Annotation::Balloon(_) => lines.extend(ballooned.next()),
        }
    }
    lines
}

/// The dimensions, notes and balloons of `d` a sheet shows as they last were, by id with their kind:
/// those of each view out of date (`stale_views`), as their view is, then `up_to_date`, those of the
/// views up to date that were read from another record, model, drawing of their view, lettering or
/// revision ([`Cache::stale_dimensions`]).
fn out_of_date_annotations(d: &Drawing, stale_views: &[u64], up_to_date: impl IntoIterator<Item = (u64, ObjectKind)>) -> Vec<(u64, ObjectKind)> {
    let held = dimensions_on(d, stale_views.iter().copied()).into_iter().map(|record| (record.id(), record.kind()));
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
        let base = DrawnDimension {
            dimension: note.id,
            view,
            kind: ObjectKind::Note,
            row: None,
            status,
            group: None,
            fonts: Vec::new(),
            why: None,
            pending: false,
        };
        // A leader on a view that draws none of the model's edges has no place to end.
        if note.paths().next().is_some() && d.view(view).and_then(crate::drawing_view::no_model_edges).is_some() {
            return DrawnDimension { why: Some(NO_PLACE.to_string()), ..base };
        }
        let (Some(resolved), Some((frame, group))) = (resolved, frame(view)) else { return base };
        let text =
            d.sheet_of(&note.anchor).and_then(|at| title_block_text::read(s, tree, d, at, std::iter::once(&note.content)).pop()).unwrap_or_default();
        let tips: Vec<Vec2> =
            std::iter::once(&resolved.anchor).chain(&resolved.branches).filter_map(|end| end.point).map(|p| frame.to_paper(p)).collect();
        // Centred on the view, as its frame is: the scene moves it to the view's centre.
        let paper = crate::drawing_note_paper::note_group(note, &text, &tips, LineKind::Thin.width_mm(group), lettering);
        DrawnDimension { group: Some(Arc::new(paper.group)), fonts: paper.fonts, why: paper.why, pending: paper.pending, ..base }
    };
    notes.iter().enumerate().map(|(at, note)| one(at, note, &mut tree)).collect()
}

/// `balloons`, each on its view (`frame`, as [`draw_dimensions`] gives it), read once, their edges
/// as dimensions are ([`read_with`]): their rows by the one rule `drawing.balloon.inspect` reads them
/// by ([`balloon_rows`]), and each one's leader to its edge's middle now, through the view's
/// frame. One with no row has an empty circle and says why; one whose view isn't drawn has no lines;
/// one that can't be read says so.
fn drawn_balloons(
    s: &Session,
    sessions: Option<&mut ViewSessions>,
    d: &Drawing,
    balloons: &[&Arc<Balloon>],
    frame: &impl Fn(u64) -> Option<(ViewFrame, LineGroup)>,
    lettering: &PaperFonts,
) -> Vec<DrawnDimension> {
    if balloons.is_empty() {
        return Vec::new();
    }
    let on: Vec<(u64, u64)> = balloons.iter().map(|balloon| (balloon.id, balloon.view)).collect();
    let read = read_with(s, sessions, d, &on, solidcraft_doc::Document::resolve_balloons);
    let records: Vec<&Balloon> = balloons.iter().map(|balloon| balloon.as_ref()).collect();
    let rows = balloon_rows(d, &records, |source| s.source_design(d, source));
    let one = |(at, (balloon, row)): (usize, (&Balloon, RowRead))| {
        let unread = || dimension::Status::Unmeasurable(format!("balloon {} can't be read", ids::encode(balloon.id)));
        let resolved = read.as_ref().ok().and_then(|all| all.get(at));
        let status = resolved.map_or_else(unread, |r| r.status.clone());
        let (view, why) = (balloon.view, row_why(&row));
        let kind = ObjectKind::Balloon;
        let base = DrawnDimension { dimension: balloon.id, view, kind, row: None, status, group: None, fonts: Vec::new(), why, pending: false };
        // Its leader ends on the model, which a view that draws none of its edges has no place for.
        if d.view(view).and_then(crate::drawing_view::no_model_edges).is_some() {
            return DrawnDimension { row: Some(row), why: Some(NO_PLACE.to_string()), ..base };
        }
        let (Some(resolved), Some((frame, group))) = (resolved, frame(view)) else { return DrawnDimension { row: Some(row), ..base } };
        let tip = resolved.point.map(|p| frame.to_paper(p));
        // Centred on the view, as its frame is: the scene moves it to the view's centre.
        let paper = crate::drawing_balloon_paper::balloon_group(balloon, row.item(), tip, LineKind::Thin.width_mm(group), lettering);
        let why = base.why.clone().or(paper.why);
        DrawnDimension { row: Some(row), group: Some(Arc::new(paper.group)), fonts: paper.fonts, why, pending: paper.pending, ..base }
    };
    records.into_iter().zip(rows).enumerate().map(one).collect()
}

/// Why a balloon shows no number, in a sentence: none when it has one.
pub(crate) fn row_why(row: &RowRead) -> Option<String> {
    let why = match row {
        RowRead::Item { .. } => return None,
        RowRead::NoList(choices) if choices.is_empty() => "it has no parts list to read",
        RowRead::NoList(_) => "it names no parts list, and its sheet and drawing have more than one",
        RowRead::Unlisted { why: Unlisted::NotFound, .. } => "its parts list can't be read for its part",
        RowRead::Unlisted { why: Unlisted::Reference, .. } => "it points at a reference, which no parts list counts",
        RowRead::Unlisted { why: Unlisted::NoRow, .. } => "its parts list has no row for its part",
        RowRead::Unread { why, .. } => return Some(format!("its parts list can't be read: {why}")),
    };
    Some(why.to_string())
}

/// A note or a revision tag on a sheet (its group's kind says which), set with the sheet's title
/// block: it reads no model, so it is set again only when the sheet's text is ([`Assembled::reads`]).
#[derive(Clone, Debug, PartialEq)]
struct SheetText {
    object: u64,
    group: Group,
    fonts: Vec<Font>,
    why: Option<String>,
}

/// The note `params` (as `drawing.note.add` takes them, on a sheet) would add, alone on its sheet
/// and drawn exactly as the sheet will draw it once placed: the same group, in the drawing's
/// lettering, at its paper size. Added on a copy of the session, so nothing is added and no undo
/// step made. What the Note tool previews while the note follows the pointer.
pub fn note_preview_scene(s: &Session, params: &serde_json::Value) -> Result<Scene> {
    const ADD: &str = "drawing.note.add";
    let mut copy = s.scratch();
    let reply = copy.execute(ADD, params)?;
    let id = reply["note"].as_str().and_then(ids::decode).ok_or_else(|| bad(ADD, "the note wasn't added"))?;
    let d = copy.drawing(ADD)?.clone();
    let Some(Anchor::Sheet(sheet)) = d.notes.by_id(id).map(|note| note.anchor.clone()) else {
        return Err(bad(ADD, "only a note on a sheet is previewed"));
    };
    let size = sheet_of(&d, sheet)?.sheet.size_mm().map_err(|e| bad(ADD, e.to_string()))?;
    let lettering = Lettering::of(&copy, &d);
    let (texts, _) = sheet_texts(&copy, &d, sheet, &lettering, false);
    let text = texts.into_iter().find(|text| text.object == id).ok_or_else(|| bad(ADD, "the note wasn't drawn"))?;
    let mut scene = Scene::new(size, vec![text.group]);
    scene.add_fonts(text.fonts);
    Ok(scene)
}

/// The notes on sheet `sheet` of `d`, by id, each with its fields read for that sheet, then its
/// revision tags, in the drawing's order, all set in `lettering` in its line group's thin lines; and
/// whether a text waits for a font to fall back to, which only the window's sheet leaves to the
/// search's thread (`wait` false).
fn sheet_texts(s: &Session, d: &Drawing, sheet: u64, lettering: &Lettering, wait: bool) -> (Vec<SheetText>, bool) {
    let on: Vec<&Arc<Note>> = d.notes.iter().filter(|note| note.anchor == Anchor::Sheet(sheet)).collect();
    let tags: Vec<_> = d.revisions.tags().iter().filter(|tag| tag.sheet == sheet).collect();
    let Some(at) = d.sheets.iter().position(|one| one.id == sheet).filter(|_| !on.is_empty() || !tags.is_empty()) else {
        return (Vec::new(), false);
    };
    let line_mm = LineKind::Thin.width_mm(d.sheets.get(at).and_then(|one| one.line_group).unwrap_or(d.line_group));
    let fonts = PaperFonts::new(&lettering.font, &lettering.language, wait);
    let mut pending = false;
    let mut set = |object: u64, paper: crate::drawing_dimension_paper::OnPaper| {
        pending |= paper.pending;
        SheetText { object, group: paper.group, fonts: paper.fonts, why: paper.why }
    };
    let mut texts = Vec::with_capacity(on.len() + tags.len());
    if !on.is_empty() {
        let state = s.model.state();
        let read = title_block_text::read(s, &mut Tree::new(&s.doc, &state), d, at, on.iter().map(|note| &note.content));
        texts.extend(on.iter().zip(read).map(|(note, text)| set(note.id, crate::drawing_note_paper::note_group(note, &text, &[], line_mm, &fonts))));
    }
    for tag in tags {
        let shows = crate::drawing_revision_paper::shows(&d.revisions, tag);
        texts.push(set(tag.id, crate::drawing_revision_paper::tag_group(tag, &shows, line_mm, &fonts)));
    }
    (texts, pending)
}

/// How far a view's label sits above its view's boundary, on paper.
const LABEL_GAP_MM: f64 = 2.0;

/// A view's label set on paper round its own middle ([`crate::drawing_note_paper::label_group`]), for
/// [`assemble`] to place by its view. It reads no model, so it is set with the sheet's text.
#[derive(Clone, Debug, PartialEq)]
struct SetLabel {
    view: u64,
    group: Group,
    fonts: Vec<Font>,
    half: Vec2,
    /// Where its middle sits from its view's centre: none above the view ([`label_place`]).
    offset: Option<Vec2>,
    why: Option<String>,
    /// A detail view's mark on its parent: its boundary and its letter.
    mark: Option<DetailMark>,
}

/// What a detail view draws on its parent: the outline of its region, thin, and its letter beside
/// it (ISO 128-30), as [`solidcraft_geom::drawing_detail::Detail`] places them on the parent's
/// paper.
#[derive(Clone, Debug, PartialEq)]
struct DetailMark {
    parent: u64,
    boundary: Boundary,
    scale: Scale,
    line_group: LineGroup,
    letter: Group,
    fonts: Vec<Font>,
    half: Vec2,
}

/// Where a detail's letter sits, from its outline's centre: up and to the right, as a reader looks
/// for it.
const MARK_DIRECTION: f64 = std::f64::consts::FRAC_PI_4;
/// The gap between a detail's outline and its letter, in paper millimetres.
const MARK_GAP_MM: f64 = 2.0;

/// The labels `views` of `d` draw, in their order ([`Drawing::label_text`]), set in `lettering`; and
/// whether a text waits for a font to fall back to, which only the window's sheet leaves to the
/// search's thread (`wait` false).
fn view_labels<'a>(d: &Drawing, views: impl Iterator<Item = &'a View>, lettering: &Lettering, wait: bool) -> (Vec<SetLabel>, bool) {
    let read = |view: &View| Some((view.id, d.label_text(view.id)?, view.label.as_ref()?.offset.map(Vec2::from)));
    let texts: Vec<(u64, String, Option<Vec2>)> = views.filter_map(read).collect();
    if texts.is_empty() {
        return (Vec::new(), false);
    }
    let fonts = PaperFonts::new(&lettering.font, &lettering.language, wait);
    let mut pending = false;
    let mut set = |(view, text, offset): (u64, String, Option<Vec2>)| {
        let (paper, half) = crate::drawing_note_paper::label_group(view, &text, &fonts);
        pending |= paper.pending;
        let mark = detail_mark(d, view, &fonts);
        pending |= mark.as_ref().is_some_and(|(_, waits)| *waits);
        SetLabel { view, group: paper.group, fonts: paper.fonts, half, offset, why: paper.why, mark: mark.map(|(mark, _)| mark) }
    };
    let labels = texts.into_iter().map(&mut set).collect();
    (labels, pending)
}

/// Detail view `view`'s mark on its parent, its letter set in `fonts`, and whether that text waits
/// for a font: none for any other view, or one whose letter isn't shown.
fn detail_mark(d: &Drawing, view: u64, fonts: &PaperFonts) -> Option<(DetailMark, bool)> {
    let record = d.view(view)?;
    let ViewKind::Detail { parent, boundary } = &record.kind else { return None };
    let label = record.label.as_ref().filter(|label| label.shown && label.show_letter)?;
    let resolved = d.resolve_view(view).ok()?;
    let (paper, half) = crate::drawing_note_paper::label_group(view, &label.letter, fonts);
    let mark = DetailMark {
        parent: *parent,
        boundary: boundary.geometry().ok()?,
        scale: resolved.scale.value,
        line_group: resolved.line_group.value,
        letter: paper.group,
        fonts: paper.fonts,
        half,
    };
    Some((mark, paper.pending))
}

/// Where a label `half` in size sits from its view's centre: at its `offset`, else centred over the
/// view's `boundary` and [`LABEL_GAP_MM`] above it, as ISO 128-30 puts a view's letter, and above
/// whatever of `beside` (the boxes of what is drawn for the view) it would otherwise cross (#17853).
/// Both are centred on the view, as its lines are. Lowest first, it steps over each box it would
/// cross, so it takes the lowest free place: one label rising on one line needs no search wider.
fn label_place(offset: Option<Vec2>, half: Vec2, boundary: Option<Aabb2>, beside: &[Aabb2]) -> Vec2 {
    offset.unwrap_or_else(|| {
        let boundary = boundary.unwrap_or(Aabb2 { min: Vec2::ZERO, max: Vec2::ZERO });
        let x = boundary.center().x;
        let mut under: Vec<&Aabb2> = beside.iter().filter(|b| b.min.x < x + half.x && b.max.x > x - half.x).collect();
        under.sort_by(|a, b| a.min.y.total_cmp(&b.min.y));
        let mut bottom = boundary.max.y + LABEL_GAP_MM;
        for b in under {
            if b.min.y < bottom + 2.0 * half.y && b.max.y > bottom {
                bottom = b.max.y + LABEL_GAP_MM;
            }
        }
        Vec2::new(x, bottom + half.y)
    })
}

/// The box round view `id`'s label on its sheet, for the view `placed` on it, as the sheet places
/// it ([`Assembly::add_labels`]): none when it draws no label, or while its text waits for a font to
/// fall back to (the window's fonts, which never wait). A label placed by its view keeps clear of the
/// view's own annotations, drawn here through the view's frame, each on its session in `sessions`, as
/// the sheet draws them.
pub(crate) fn label_box(s: &Session, sessions: &mut ViewSessions, d: &Drawing, id: u64, placed: &crate::drawing_view::OnSheet) -> Option<Aabb2> {
    let lettering = Lettering { units: String::new(), language: d.language.clone(), font: d.font.clone() };
    let (labels, _) = view_labels(d, d.view(id).into_iter(), &lettering, false);
    let label = labels.into_iter().find(|label| !label.group.items.is_empty())?;
    let centre = d.view_centre(id).ok()?;
    let wanted = if label.offset.is_none() { dimensions_on(d, std::iter::once(id)) } else { Vec::new() };
    let beside = match d.resolve_view(id) {
        Ok(view) if !wanted.is_empty() => {
            let frame = crate::drawing_view::view_frame(&view, &placed.extents.model, Vec2::ZERO, CMD).ok();
            let drawn = draw_dimensions(s, Some(sessions), d, &wanted, |_| Some((frame?, view.line_group.value)), false);
            drawn.iter().filter_map(|one| Some(group_bounds(one.group.as_deref()?, &one.fonts))).collect()
        }
        _ => Vec::new(),
    };
    let boundary = Aabb2 { min: placed.boundary.min - centre, max: placed.boundary.max - centre };
    let at = centre + label_place(label.offset, label.half, Some(boundary), &beside);
    Some(group_bounds(&moved(&label.group, at), &label.fonts))
}

/// The dimensions, the notes and the balloons of `views` of `d`, by view and id.
fn dimensions_on(d: &Drawing, views: impl Iterator<Item = u64>) -> Vec<Annotation> {
    let views: BTreeSet<u64> = views.collect();
    let dimensions = d.dimensions.iter().filter(|record| views.contains(&record.view)).map(|record| Annotation::Dimension(record.clone()));
    let notes = d.notes.iter().filter_map(|note| Some(Annotation::Note(note.clone(), note.anchor.view().filter(|view| views.contains(view))?)));
    let balloons = d.balloons.iter().filter(|balloon| views.contains(&balloon.view)).map(|balloon| Annotation::Balloon(balloon.clone()));
    let mut on: Vec<Annotation> = dimensions.chain(notes).chain(balloons).collect();
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
    /// Each of `states`' views' centre and boundary (centred on the view), for its label.
    around: Vec<(Vec2, Option<Aabb2>)>,
    /// The boxes of what joined for each view, centred on it: what its label keeps clear of.
    beside: BTreeMap<u64, Vec<Aabb2>>,
    /// The objects on the sheet, each with its group's paint band, in the order they joined.
    objects: Vec<(u8, SheetObject)>,
    /// The frame and centre of each view whose lines are in the scene.
    placed: BTreeMap<u64, (ViewFrame, Vec2)>,
    dimensions: Vec<DimensionState>,
    notes: Vec<NoteState>,
    balloons: Vec<BalloonState>,
    tags: Vec<TagState>,
    sketches: Vec<SketchState>,
    fills: Vec<FillState>,
}

/// Why a group stayed out of a scene.
enum LeftOut {
    TooManyRecords,
    Invalid(String),
}

impl Assembly {
    fn add(&mut self, group: &Group, fonts: &[Font], centre: Vec2, of: DrawnFor) -> std::result::Result<Aabb2, LeftOut> {
        self.admit(group, fonts, of, || moved(group, centre))
    }

    // Sheet sketches are already in paper coordinates: preserve signed zeroes by cloning.
    fn admit(&mut self, source: &Group, fonts: &[Font], of: DrawnFor, group: impl FnOnce() -> Group) -> std::result::Result<Aabb2, LeftOut> {
        self.alone.add_fonts(fonts.iter().cloned());
        let more = source.records().saturating_add(self.scene.font_records_beyond(&self.alone.fonts));
        if self.records.saturating_add(more) > MAX_RECORDS {
            self.alone.fonts.clear();
            return Err(LeftOut::TooManyRecords);
        }
        self.alone.groups.push(group());
        if let Err(e) = self.alone.validate() {
            self.alone.groups.clear();
            self.alone.fonts.clear();
            return Err(LeftOut::Invalid(e.to_string()));
        }
        self.records += more;
        let bounds = self.alone.groups.first().map_or(Aabb2::EMPTY, |one| group_bounds(one, &self.alone.fonts));
        let band = source.kind.band(of);
        self.groups.extend(self.alone.groups.drain(..).map(|joined| (band, joined)));
        self.scene.add_fonts(std::mem::take(&mut self.alone.fonts));
        Ok(bounds)
    }

    /// Adds `dimensions` (and the notes and balloons of views) after the views, each moved to its view's centre.
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
                        self.beside.entry(one.view).or_default().push(Aabb2 { min: bounds_mm.min - centre, max: bounds_mm.max - centre });
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
                ObjectKind::Balloon => {
                    let row = one.row.clone().unwrap_or(RowRead::NoList(Vec::new()));
                    self.balloons.push(BalloonState { balloon: one.dimension, view: one.view, status, row, drawn, why })
                }
                _ => self.dimensions.push(DimensionState { dimension: one.dimension, view: one.view, status, drawn, why }),
            }
        }
    }

    /// Adds `texts`, the sheet's own notes and revision tags, after the views' dimensions and notes.
    /// One that would take the scene past a paper scene's limits is left out and says why.
    fn add_sheet_texts(&mut self, texts: &[SheetText]) {
        for one in texts {
            let mut why = one.why.clone();
            let kind = one.group.kind;
            let drawn = match self.add(&one.group, &one.fonts, Vec2::ZERO, DrawnFor::Sheet) {
                Ok(bounds_mm) => {
                    self.objects.extend(SheetObject::on_paper(one.object, kind, bounds_mm, None).map(banded(DrawnFor::Sheet)));
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
            match kind {
                ObjectKind::RevisionTag => self.tags.push(TagState { tag: one.object, drawn, why }),
                _ => self.notes.push(NoteState { note: one.object, view: None, status: dimension::Status::Ok, drawn, why }),
            }
        }
    }

    /// Adds `labels`, each by its view, after the views' annotations, so each keeps clear of its own
    /// view's ([`label_place`]), and gives each view its label's state.
    fn add_labels(&mut self, labels: &[SetLabel]) {
        for (at, (centre, boundary)) in std::mem::take(&mut self.around).into_iter().enumerate() {
            let Some(view) = self.states.get(at).map(|state| state.view) else { continue };
            let Some(label) = labels.iter().find(|label| label.view == view) else { continue };
            let beside = self.beside.remove(&view).unwrap_or_default();
            let drawn = self.add_label(label, centre, boundary, &beside);
            if let Some(state) = self.states.get_mut(at) {
                state.label = Some(drawn);
            }
        }
        // Every view is placed by now: each detail's mark on its parent.
        for label in labels {
            if let Some(mark) = &label.mark {
                self.add_mark(label.view, mark);
            }
        }
    }

    /// Adds `label`, its view's, for the view centred at `centre` with `boundary` round it and `beside`
    /// drawn for it (both centred on the view), and says whether it was drawn. One with no text set
    /// isn't; one that would take the scene past a paper scene's limits is left out and says why.
    fn add_label(&mut self, label: &SetLabel, centre: Vec2, boundary: Option<Aabb2>, beside: &[Aabb2]) -> LabelState {
        if label.group.items.is_empty() {
            return LabelState { drawn: false, why: label.why.clone() };
        }
        let at = centre + label_place(label.offset, label.half, boundary, beside);
        match self.add(&label.group, &label.fonts, at, DrawnFor::View) {
            Ok(bounds_mm) => {
                self.objects.extend(SheetObject::on_paper(label.view, ObjectKind::ViewLabel, bounds_mm, None).map(banded(DrawnFor::View)));
                LabelState { drawn: true, why: label.why.clone() }
            }
            Err(LeftOut::TooManyRecords) => {
                LabelState { drawn: false, why: Some(format!("with its text the sheet is more than a paper scene's {MAX_RECORDS} records")) }
            }
            Err(LeftOut::Invalid(e)) => LabelState { drawn: false, why: Some(e) },
        }
    }

    /// Adds detail view `view`'s `mark` on its parent, where the parent is drawn: its outline and its
    /// letter. Nothing is drawn while the parent isn't on the sheet, or the region doesn't fit it.
    fn add_mark(&mut self, view: u64, mark: &DetailMark) {
        let Some((frame, centre)) = self.placed.get(&mark.parent).copied() else { return };
        let Ok(detail) = solidcraft_geom::drawing_detail::Detail::new(&frame, mark.boundary, mark.scale, Vec2::ZERO) else { return };
        let Ok(anchor) = detail.label_anchor(MARK_DIRECTION, MARK_GAP_MM) else { return };
        let width_mm = LineKind::Thin.width_mm(mark.line_group);
        let mut items = vec![Item::Stroke { path: detail.outline(), closed: true, width_mm, pattern: None }];
        // The letter's middle a half its size further out, so its nearest corner meets the gap.
        let out = Vec2::from_angle(MARK_DIRECTION);
        items.extend(moved(&mark.letter, anchor + Vec2::new(out.x * mark.half.x, out.y * mark.half.y)).items);
        let group = Group { object: view, kind: ObjectKind::ViewLabel, items };
        let _ = self.add(&group, &mark.fonts, centre, DrawnFor::View);
    }

    /// Adds the sheet's `tables`, each already where it stands on the sheet, in their own paint band
    /// ([`ObjectKind::band`]), and gives each one's state. One that can't be read, or that would take
    /// the scene past a paper scene's limits, is left out and says why; the rest still draw.
    fn add_tables(&mut self, tables: &SheetTables) -> Vec<TableState> {
        let mut states = Vec::with_capacity(tables.tables.len());
        for (table, read) in &tables.tables {
            let state = |drawn, failed, why| TableState { table: *table, drawn, failed, why };
            states.push(match read {
                Err(why) => state(false, true, Some(why.clone())),
                Ok(on_paper) => match self.add(&on_paper.group, &on_paper.fonts, Vec2::ZERO, DrawnFor::Sheet) {
                    Ok(bounds_mm) => {
                        self.objects.extend(SheetObject::on_paper(*table, on_paper.group.kind, bounds_mm, None).map(banded(DrawnFor::Sheet)));
                        state(true, false, on_paper.why.clone())
                    }
                    Err(LeftOut::TooManyRecords) => {
                        state(false, false, Some(format!("with its lines the sheet is more than a paper scene's {MAX_RECORDS} records")))
                    }
                    Err(LeftOut::Invalid(e)) => state(false, false, Some(e)),
                },
            });
        }
        states
    }

    /// Adds physical fill paint before sketch strokes, through the same whole-sheet admission.
    fn add_fills<F: Borrow<DrawnFill>>(&mut self, fills: impl Iterator<Item = (u64, F)>) {
        for (fill, drawn) in fills {
            let drawn = drawn.borrow();
            let mut status = drawn.status.clone();
            if let Some(group) = &drawn.group {
                match self.admit(group, &[], DrawnFor::Sheet, || group.clone()) {
                    Ok(_) => {}
                    Err(LeftOut::TooManyRecords) => {
                        status =
                            FillStatus::Failed(format!("fill {}: with its paint the sheet exceeds {MAX_RECORDS} paper records", ids::encode(fill)));
                    }
                    Err(LeftOut::Invalid(error)) => status = FillStatus::Failed(format!("fill {}: {error}", ids::encode(fill))),
                }
            }
            self.fills.push(FillState { fill, sketch: drawn.sketch, status });
        }
    }

    /// Adds the sketches after dimensions/fills, using the same whole-sheet admission: a sheet's as
    /// they are, a view's through its view's frame and to its centre, as the view's dimensions go.
    /// A view's sketch whose view isn't in the scene is left out and says so.
    fn add_sketches<K: Borrow<DrawnSketch>>(&mut self, sketches: impl Iterator<Item = (u64, K)>) {
        for (sketch, drawn) in sketches {
            let drawn = drawn.borrow();
            let mut status = drawn.status.clone();
            if let Some(group) = &drawn.group {
                let admitted = match drawn.view {
                    None => self.admit(group, &[], DrawnFor::Sheet, || group.clone()),
                    Some(view) => match self.placed.get(&view).copied() {
                        Some((frame, centre)) => match sketch::through(group, &frame) {
                            Ok(placed) => self.admit(group, &[], DrawnFor::View, || moved(&placed, centre)),
                            Err(why) => Err(LeftOut::Invalid(why)),
                        },
                        None => Err(LeftOut::Invalid(NOT_DRAWN.into())),
                    },
                };
                match admitted {
                    Ok(_) => {}
                    Err(LeftOut::TooManyRecords) => {
                        status = SketchStatus::Failed(format!(
                            "sketch {}: with its strokes the sheet exceeds a paper scene's {MAX_RECORDS} records",
                            ids::encode(sketch)
                        ));
                    }
                    Err(LeftOut::Invalid(e)) => status = SketchStatus::Failed(format!("sketch {}: {e}", ids::encode(sketch))),
                }
            }
            self.sketches.push(SketchState { sketch, status });
        }
    }

    /// The scene with its groups in paint order, checked whole, and its objects in the same order.
    #[allow(clippy::type_complexity)]
    fn finish(self) -> Result<(Scene, Vec<ViewState>, Vec<SheetObject>, Vec<DimensionState>, Vec<NoteState>, Vec<BalloonState>)> {
        let mut scene = self.scene;
        scene.groups = in_paint_order(self.groups);
        scene.validate().map_err(|e| bad(CMD, e.to_string()))?;
        Ok((scene, self.states, in_paint_order(self.objects), self.dimensions, self.notes, self.balloons))
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
/// lines, in id order) moved to its centre, with its title block (`logos` under its lines), its symbol
/// in `projection` and its cells' text as [`block_text`] set it, which its band puts on top
/// ([`Assembly::finish`]). A view that would take the scene past a paper scene's limits is left out
/// and says why, and the views after it are still drawn when they fit. The views' labels join after
/// their annotations ([`Assembly::add_labels`]), so each keeps clear of them.
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
            let mut group = solidcraft_doc::drawing::projection_symbol::title_block_paper(&placed, record.id, projection);
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
    let (states, around, beside, placed) = (Vec::new(), Vec::new(), BTreeMap::new(), BTreeMap::new());
    let (dimensions, notes, balloons) = (Vec::new(), Vec::new(), Vec::new());
    let (tags, sketches, fills) = (Vec::new(), Vec::new(), Vec::new());
    let mut assembly =
        Assembly { scene, groups, records, alone, states, around, beside, objects, placed, dimensions, notes, balloons, tags, sketches, fills };
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
        assembly.around.push((centre, drawn.boundary));
        assembly.states.push(ViewState { view, status, stats: drawn.stats, why: drawn.why.clone(), label: None, hatches: drawn.hatches.clone() });
    }
    Ok(assembly)
}

// Existing private sketch admission cases use this adapter over the canonical Assembly.
#[cfg(test)]
fn assemble_with_sketches<D: Borrow<Drawn>, K: Borrow<DrawnSketch>>(
    record: &Sheet,
    projection: Projection,
    images: &[Group],
    views: impl Iterator<Item = (u64, Vec2, D)>,
    sketches: impl Iterator<Item = (u64, K)>,
) -> Result<(Scene, Vec<ViewState>, Vec<SheetObject>, Vec<SketchState>)> {
    let mut assembly = assemble(record, projection, &Set::default(), images, &[], views)?;
    assembly.add_sketches(sketches);
    let sketches = std::mem::take(&mut assembly.sketches);
    let (scene, views, objects, _, _, _) = assembly.finish()?;
    Ok((scene, views, objects, sketches))
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
    title_block_text::shown(s, &mut Tree::new(&s.doc, &state), d, at)
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

/// The tables of sheet `record` of `d`, read from `s`'s design and measured in `lettering`'s font
/// and language, as [`block_text`] sets the block's text; `wait` as there.
fn sheet_tables(s: &Session, d: &Drawing, record: &Sheet, lettering: &Lettering, wait: bool) -> SheetTables {
    crate::drawing_table_paper::sheet_tables(s, d, record, &PaperFonts::new(&lettering.font, &lettering.language, wait))
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

/// The sketches on sheet `sheet`: its own, and its views' (drawn through their view).
fn sketches_on(d: &Drawing, sheet: u64) -> Vec<&Arc<SheetSketch>> {
    let mut on: Vec<_> = d.sketches.iter().filter(|record| d.sketch_sheet(record) == Some(sheet)).collect();
    on.sort_unstable_by_key(|record| record.id);
    on
}

fn fills_on(d: &Drawing, sheet: u64) -> Vec<&Arc<SketchFill>> {
    let on: BTreeSet<_> = sketches_on(d, sheet).into_iter().map(|sketch| sketch.id).collect();
    let mut fills: Vec<_> = d.fills.iter().filter(|fill| on.contains(&fill.sketch)).collect();
    fills.sort_unstable_by_key(|fill| fill.id);
    fills
}

/// `e`'s message without the command's name: a view's reason in a sheet scene.
fn reason(e: EngineError) -> String {
    match e {
        EngineError::BadParams { msg, .. } => msg,
        e => e.to_string(),
    }
}

/// The provider's active drawing cache and the parked caches of drawings shown in other Sheet
/// tiles. Views share their drawing's lines and request tokens across display cameras.
#[derive(Default)]
pub struct Cache {
    /// The drawing it is for.
    drawing: Option<u64>,
    /// Other drawings displayed by live Sheet tiles. Only this root owns the map; parked
    /// entries have no nested caches or kept lines. The host's bounded view registry caps it.
    parked: BTreeMap<u64, Box<Cache>>,
    /// The sheet last extracted: the one [`step`] brings up to date.
    sheet: Option<u64>,
    /// Each view's last drawing, by id.
    views: BTreeMap<u64, Entry>,
    /// Each dimension's last drawing, by id.
    dimensions: BTreeMap<u64, DimensionEntry>,
    sketches: BTreeMap<u64, SketchEntry>,
    fills: BTreeMap<u64, FillEntry>,
    fill_regions: BTreeMap<u64, FillRegionEntry>,
    /// Metadata only: refresh once per document Arc, without evaluating expressions.
    source: Option<(Arc<Document>, Arc<SourceInputs>)>,
    context: Option<(Arc<Drawing>, Arc<SketchContext>)>,
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
    /// The bodies pictures are drawn from, for the last world state and design they were read for.
    bodies: Option<Bodies>,
    /// The picture being drawn.
    picturing: Option<Picturing>,
    /// The pixel tests a view's pictures may cost together, as the files' pictures carry theirs
    /// ([`crate::drawing_picture::ViewPictures`]): the renderer's [`MAX_PICTURE_PIXEL_TESTS`] when None.
    max_pixel_tests: Option<u64>,
    /// Views drawn, for tests: a move draws none.
    #[cfg(test)]
    projections: usize,
    #[cfg(test)]
    sketch_solves: usize,
    #[cfg(test)]
    fill_solves: usize,
    #[cfg(test)]
    fill_expansions: usize,
    /// Picture bands drawn, for tests: a step draws as many as its budget allows.
    #[cfg(test)]
    bands: usize,
}

#[derive(PartialEq)]
struct SourceInputs {
    units: String,
    defs: Vec<ParamDef>,
}

/// All sketch records participate in the document's cumulative solve-admission budgets.
/// Share this context across entries; view moves do not change it. A record edit conservatively
/// invalidates the sheet's solves, including cached failures caused by another record's budget.
struct SketchContext {
    drawing: Arc<Drawing>,
}

/// Solving admits the complete drawing's structure and cumulative sketch budgets. Every such
/// input participates, even when it belongs to another sheet. A valid numeric move or a new scale
/// of an otherwise unchanged view cannot affect admission (a view's sketch is solved in its plane,
/// and its view's frame places it later): keep its context and avoid solving sketches again.
fn same_sketch_context(was: &Drawing, now: &Drawing) -> bool {
    if was.sketches.len() != now.sketches.len()
        || !was.sketches.iter().zip(&now.sketches).all(|(a, b)| Arc::ptr_eq(a, b))
        || was.views.len() != now.views.len()
    {
        return false;
    }
    let valid = |place| match place {
        ViewPlace::At([x, y]) => is_paper_coordinate(x) && is_paper_coordinate(y),
        ViewPlace::Aligned(along) => is_paper_coordinate(along),
    };
    for (a, b) in was.views.iter().zip(&now.views) {
        if Arc::ptr_eq(a, b) {
            continue;
        }
        if std::mem::discriminant(&a.place) != std::mem::discriminant(&b.place) || !valid(a.place) || !valid(b.place) {
            return false;
        }
        let mut unmoved = (**b).clone();
        unmoved.place = a.place;
        unmoved.settings.scale = a.settings.scale;
        if **a != unmoved {
            return false;
        }
    }
    let mut unmoved = now.clone();
    unmoved.views = was.views.clone();
    *was == unmoved
}

#[derive(Clone)]
struct SketchEntry {
    source: Arc<SourceInputs>,
    context: Arc<SketchContext>,
    drawn: DrawnSketch,
    serial: u64,
}

#[derive(Clone)]
struct FillEntry {
    source: Arc<SourceInputs>,
    context: Arc<SketchContext>,
    drawn: DrawnFill,
    serial: u64,
}

struct FillRegionEntry {
    source: Arc<SourceInputs>,
    context: Arc<SketchContext>,
    regions: FillRegions,
}

/// What a view's lines show: each body of the world state by the occurrences that place it, its
/// definition's body name, whether it is a reference, its geometry where it stands, and its cosmetic
/// threads (which change no geometry, yet draw lines of their own). A world state
/// with the same draws the same lines, so an appearance, or an edit taken back, draws nothing. It
/// keys the whole set, not a view's own bodies: which bodies share a mesh, so the lines' last bits,
/// hangs on all of them ([`crate::mesh_lines`]).
#[derive(Debug, PartialEq)]
pub(crate) struct SourceKey(Vec<(Vec<u64>, String, bool, u64, Vec<ThreadInfo>)>);

impl SourceKey {
    /// None when there is nothing to show, or a body's geometry has no key: a view drawn from it
    /// always draws again.
    fn of(shown: &std::result::Result<ShownSet, Unmeasured>) -> Option<Arc<SourceKey>> {
        let set = shown.as_ref().ok()?;
        let body = |one: &Shown| {
            let body = set.body(one)?;
            let threads = set.state.threads.iter().filter(|t| t.body == body.name).cloned().collect();
            Some((one.path.clone(), one.body.clone(), one.reference, body.geometry_key()?, threads))
        };
        set.bodies.iter().map(body).collect::<Option<Vec<_>>>().map(|bodies| Arc::new(SourceKey(bodies)))
    }
}

/// A view's last drawing, and what it was drawn from: the world state (held, so its address can't
/// be reused while it is compared), what that showed, and the view as it drew. Kept whole when its
/// drawing closes, all but its world state ([`Kept::closing`]), so what is added here is kept too.
#[derive(Clone)]
struct Entry {
    state: Arc<ModelState>,
    shows: Option<Arc<SourceKey>>,
    view: ResolvedView,
    drawn: Drawn,
    serial: u64,
    /// Its pictures' pixels, by source, carried over from its last drawing until drawn again.
    pixels: BTreeMap<u64, Pixels>,
}

/// One picture's pixels as [`step`] drew them, centred on its view as its lines are, or why it
/// couldn't be drawn; and what they were drawn from.
#[derive(Clone, Debug)]
struct Pixels {
    grid: PixelGrid,
    rows: std::result::Result<Arc<[u8]>, String>,
    from: DrawnFrom,
    /// The pixel tests it cost: its view's pictures together stay within the renderer's limit.
    tests: u64,
}

impl Pixels {
    /// Drawn from what its picture would be drawn from now.
    fn current(&self, now: &DrawnFrom) -> bool {
        self.from.same(now)
    }
}

/// What a picture's pixels are drawn from: its view's drawing (by serial, which holds the picture's
/// rectangle), the bodies' looks, and the dpi of the sheet the page shows, which sets its grid. The
/// one rule for whether pixels, a picture being drawn or one asked for are still the ones wanted.
#[derive(Clone, Debug)]
struct DrawnFrom {
    serial: u64,
    looks: Arc<Looks>,
    /// None while no sheet is open: no grid, so nothing is drawn.
    dpi: Option<f64>,
}

impl DrawnFrom {
    /// For the pictures of the view `entry` draws, of bodies that look as `looks` says, at `dpi`
    /// ([`dpi_now`]).
    fn of(entry: &Entry, looks: &Arc<Looks>, dpi: Option<f64>) -> DrawnFrom {
        DrawnFrom { serial: entry.serial, looks: looks.clone(), dpi }
    }

    fn same(&self, other: &DrawnFrom) -> bool {
        self.serial == other.serial && Arc::ptr_eq(&self.looks, &other.looks) && self.dpi == other.dpi
    }
}

/// The dpi of the sheet the page shows ([`extract`]'s last), of the drawing open; None with none.
fn dpi_now(s: &Session) -> Option<f64> {
    let (Ok(d), Some(sheet)) = (s.drawing(CMD), s.drawing_scenes.sheet) else { return None };
    sheet_of(d, sheet).ok().map(|sheet| sheet.dpi)
}

/// How the bodies of a world state stand and look in a picture: each body's geometry key and its
/// colours, in the world state's order. Pictures drawn from equal looks are the same pixels. None,
/// equal to no other, when a body's geometry has no key or the bodies couldn't be read.
#[derive(Debug)]
struct Looks(Option<Vec<(u64, Look)>>);

/// A body's colours in a picture, as [`ProductScene`] gives them.
type Look = ([u8; 3], f64, Option<OpaqueSurface>, Option<Transmission>, Vec<FaceAppearance>);

impl PartialEq for Looks {
    fn eq(&self, other: &Looks) -> bool {
        matches!((&self.0, &other.0), (Some(a), Some(b)) if a == b)
    }
}

impl Looks {
    /// `scene`'s, the bodies of world state `state` in its order.
    fn of(state: &ModelState, scene: Option<&ProductScene>) -> Looks {
        let Some(scene) = scene.filter(|scene| scene.meshes.len() == state.bodies.len()) else { return Looks(None) };
        let look = |(body, mesh): (&solidcraft_doc::ModelBody, &solidcraft_render::ProductMesh)| {
            // Every field named, so a field the renderer gains stops the build here, to be decided: the
            // name never changes pixels, and the geometry key stands for the mesh.
            let solidcraft_render::ProductMesh { name: _, mesh: _, color, opacity, surface, transmission, faces } = mesh;
            Some((body.geometry_key()?, (*color, *opacity, *surface, *transmission, faces.clone())))
        };
        Looks(state.bodies.iter().zip(&scene.meshes).map(look).collect())
    }
}

/// Every body the design places as its pictures draw them ([`crate::product_scene::extract_where`]),
/// and how they look, read once for the world state and design they are for.
struct Bodies {
    state: Arc<ModelState>,
    doc: Arc<Document>,
    scene: std::result::Result<Arc<ProductScene>, String>,
    looks: Arc<Looks>,
}

/// The picture [`step`] is drawing, a band at a time, and what it is drawn from.
struct Picturing {
    view: u64,
    source: u64,
    from: DrawnFrom,
    grid: PixelGrid,
    tests: u64,
    rows: PictureRows,
}

impl Entry {
    /// Drawn from another world state than `state` that showed `now`.
    fn shows_the_same(&self, state: &Arc<ModelState>, now: &SourceKey) -> bool {
        !Arc::ptr_eq(&self.state, state) && self.shows.as_deref() == Some(now)
    }
}

/// A dimension's last drawing, or a note's or a balloon's, and what it was drawn from: its record, the
/// world state, its view's drawing (by serial, which holds its frame), the drawing's lettering and, for
/// one that reads the document, the document's revision ([`Annotation::reads_document`]). Kept as a
/// view's entry is ([`Entry`]).
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
    sketches: Vec<(u64, u64)>,
    fills: Vec<(u64, u64)>,
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
    /// The sheet's own notes and revision tags, set with its block.
    notes: Arc<Vec<SheetText>>,
    /// Its views' labels, set with its block.
    labels: Arc<Vec<SetLabel>>,
    /// The sheet's tables, read with the block's text and for the same five.
    tables: Arc<SheetTables>,
    /// Whether a text of the block, of a note on the sheet, of a label or of a table waits for a font to fall back to: until it
    /// doesn't, the block is set again on each [`extract`], and the sheet put together with it is never reused.
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
        let source = if self.drawing == open { Some(self) } else { open.and_then(|id| self.parked.get(&id).map(Box::as_ref)) };
        let Some(source) = source else { return Cache { kept: self.kept.copy_of(open), ..Cache::default() } };
        Cache {
            views: source.views.clone(),
            dimensions: source.dimensions.clone(),
            sketches: source.sketches.clone(),
            fills: source.fills.clone(),
            updated: source.updated.clone(),
            serial: source.serial,
            kept: self.kept.copy_of(open),
            ..Cache::default()
        }
    }

    /// Drawing `drawing`'s entries kept when it closed, back as its views' and dimensions', numbered
    /// again from the cache's serial, which started over at the close. Drawn from a world state that
    /// has gone ([`Kept::closing`]), so each view is out of date until [`keep_what_shows_the_same`]
    /// finds it shows the same, and each dimension is read again once its view is up to date (its
    /// `view_serial`, from before the close, never matches its view's new one either). A held drawing
    /// draws neither, so it shows them as they were.
    fn take_kept(&mut self, drawing: u64) {
        let Kept { views, dimensions, sketches, fills } = self.kept.take(drawing);
        for (id, mut entry) in views {
            self.serial += 1;
            entry.serial = self.serial;
            self.views.insert(id, entry);
        }
        for (id, mut entry) in dimensions {
            self.serial += 1;
            entry.serial = self.serial;
            self.dimensions.insert(id, entry);
        }
        for (id, kept) in fills {
            self.serial = self.serial.wrapping_add(1);
            self.fills.insert(id, FillEntry { source: kept.source, context: kept.context, drawn: kept.drawn, serial: self.serial });
        }
        for (id, kept) in sketches {
            self.serial = self.serial.wrapping_add(1);
            let entry = SketchEntry { source: kept.source, context: kept.context, drawn: kept.drawn, serial: self.serial };
            self.sketches.insert(id, entry);
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
        let sketches = sketches_on(d, record.id).iter().map(|record| (record.id, self.sketches.get(&record.id).map_or(0, |e| e.serial))).collect();
        let fills = fills_on(d, record.id).iter().map(|record| (record.id, self.fills.get(&record.id).map_or(0, |e| e.serial))).collect();
        Inputs { sheet: record.clone(), projection: d.projection, images, logos, views, dimensions, sketches, fills }
    }

    /// The views of the last scene assembled, while it is still its sheet's scene: the sheet as `d`
    /// has it, holding the same views, none drawn again since. So a view in it is on that sheet.
    fn assembled(&self, d: &Drawing) -> Option<&[ViewState]> {
        let last = self.last.as_ref()?;
        let on = views_on(d, last.sheet).ok()?;
        (self.inputs(d, sheet_of(d, last.sheet).ok()?, &on) == last.inputs).then_some(last.answer.views.as_slice())
    }

    fn fresh_sketch(&self, id: u64, source: &Arc<SourceInputs>, context: &Arc<SketchContext>) -> bool {
        self.sketches.get(&id).is_some_and(|e| Arc::ptr_eq(&e.source, source) && Arc::ptr_eq(&e.context, context))
    }

    fn fresh_fill(&self, id: u64, source: &Arc<SourceInputs>, context: &Arc<SketchContext>) -> bool {
        self.fills.get(&id).is_some_and(|entry| Arc::ptr_eq(&entry.source, source) && Arc::ptr_eq(&entry.context, context))
    }

    fn sketch_inputs(&mut self, doc: &Arc<Document>, drawing: &Arc<Drawing>) -> (Arc<SourceInputs>, Arc<SketchContext>) {
        let source = match &self.source {
            Some((was, source)) if Arc::ptr_eq(was, doc) => source.clone(),
            _ => {
                let next = SourceInputs { units: doc.units.clone(), defs: doc.param_defs() };
                match &self.source {
                    Some((_, source)) if **source == next => source.clone(),
                    _ => Arc::new(next),
                }
            }
        };
        self.source = Some((doc.clone(), source.clone()));
        let context = match &self.context {
            Some((was, context)) if Arc::ptr_eq(was, drawing) || same_sketch_context(&context.drawing, drawing) => context.clone(),
            _ => Arc::new(SketchContext { drawing: drawing.clone() }),
        };
        self.context = Some((drawing.clone(), context.clone()));
        (source, context)
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
            self.dimensions.get(&record.id()).filter(read_now).filter(|e| !record.reads_document() || e.revision == revision)
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
    /// The dimensions, notes and balloons its sheets show as they last were ([`out_of_date_annotations`]).
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
    // By id alone: its readers ask after a dimension, note or balloon by its id.
    let up_to_date = stale_dimensions.iter().map(|record| (record.id(), record.kind()));
    let annotations = out_of_date_annotations(d, &stale, up_to_date).into_iter().map(|(id, _)| id).collect();
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
    // The title block's cells, the sheet's notes, its views' labels and its tables are read and set again only for another
    // sheet, drawing, design, model or lettering (a name or a mass can change them), not for each view drawn.
    let (cells, text, notes, labels, tables, pending) = match last {
        Some(last) => (last.cells.clone(), last.text.clone(), last.notes.clone(), last.labels.clone(), last.tables.clone(), false),
        None => {
            let cells = cell_texts(s, &d, sheet);
            let record = sheet_of(&d, sheet)?;
            let (text, block_pending) = block_text(record, &lettering, &cells, false)?;
            let (notes, notes_pending) = sheet_texts(s, &d, sheet, &lettering, false);
            let (labels, labels_pending) = view_labels(&d, d.views.iter().map(|v| &**v).filter(|v| v.sheet == sheet), &lettering, false);
            let tables = sheet_tables(s, &d, record, &lettering, false);
            let pending = block_pending || notes_pending || labels_pending || tables.pending;
            (cells, Arc::new(text), Arc::new(notes), Arc::new(labels), Arc::new(tables), pending)
        }
    };
    let doc = s.doc.clone();
    let cache = &mut s.drawing_scenes;
    let record = sheet_of(&d, sheet)?;
    cache.sheet = Some(sheet);
    catch_up(s, &d, on.iter().map(|(id, ..)| *id), &state);
    let cache = &mut s.drawing_scenes;
    let sketches = sketches_on(&d, sheet);
    let fills = fills_on(&d, sheet);
    let (source, context) = cache.sketch_inputs(&s.doc, &d);
    let stale_sketches = sketches.iter().filter(|record| !cache.fresh_sketch(record.id, &source, &context)).map(|record| record.id).collect();
    let kept: BTreeSet<u64> = d.sketches.iter().map(|record| record.id).collect();
    cache.sketches.retain(|id, _| kept.contains(id));
    let stale_fills = fills.iter().filter(|fill| !cache.fresh_fill(fill.id, &source, &context)).map(|fill| fill.id).collect();
    let kept: BTreeSet<_> = d.fills.iter().map(|fill| fill.id).collect();
    cache.fills.retain(|id, _| kept.contains(id));
    let kept: BTreeSet<_> = d.fills.iter().map(|fill| fill.sketch).collect();
    cache.fill_regions.retain(|id, _| kept.contains(id));
    let stale = on.iter().filter(|(id, view, _)| !cache.fresh(*id, &state, view)).map(|(id, ..)| *id).collect();
    let out_of_date = cache.stale_dimensions(&d, &on, &state, &lettering, revision);
    let of = |kind: ObjectKind| out_of_date.iter().filter(|record| record.kind() == kind).map(Annotation::id).collect::<Vec<u64>>();
    let (stale_dimensions, stale_notes, stale_balloons) = (of(ObjectKind::Dimension), of(ObjectKind::Note), of(ObjectKind::Balloon));
    let waiting_for_fonts = pending || handed_back || !cache.waiting_dimensions(&d, &on, &state, &lettering, revision).is_empty();
    let inputs = cache.inputs(&d, record, &on);
    let (scene, views, objects, dimensions, notes_drawn, balloons, table_states, tags, sketches, fills, generation) = match cache.last.take() {
        // Text set while a font was looked for can differ with the same cells and faces.
        Some(last)
            if !last.pending
                && !pending
                && last.inputs == inputs
                && last.cells == cells
                && last.text.faces() == text.faces()
                && last.notes == notes
                && last.labels == labels
                && last.tables == tables =>
        {
            let answer = last.answer;
            (
                answer.scene,
                answer.views,
                answer.objects,
                answer.dimensions,
                answer.notes,
                answer.balloons,
                answer.tables,
                answer.tags,
                answer.sketches,
                answer.fills,
                answer.generation,
            )
        }
        _ => {
            // Views and annotations no longer in the drawing are forgotten: each holds a world state.
            let kept: BTreeSet<u64> = d.views.iter().map(|v| v.id).collect();
            cache.views.retain(|id, _| kept.contains(id));
            cache.dimensions.retain(|id, _| d.dimension(*id).is_some() || d.notes.by_id(*id).is_some() || d.balloons.by_id(*id).is_some());
            let drawn = on.iter().filter_map(|(id, _, centre)| cache.views.get(id).map(|e| (*id, *centre, &e.drawn)));
            let mut assembly = assemble(record, d.projection, &text, &inputs.images, &inputs.logos, drawn)?;
            assembly.add_dimensions(inputs.dimensions.iter().filter_map(|(id, _)| cache.dimensions.get(id).map(|e| &e.drawn)));
            assembly.add_labels(&labels);
            let table_states = assembly.add_tables(&tables);
            assembly.add_sheet_texts(&notes);
            let sketched = sketches.iter().filter_map(|record| cache.sketches.get(&record.id).map(|e| (record.id, &e.drawn)));
            let filled = fills.iter().filter_map(|record| cache.fills.get(&record.id).map(|entry| (record.id, &entry.drawn)));
            assembly.add_fills(filled);
            assembly.add_sketches(sketched);
            let fills = std::mem::take(&mut assembly.fills);
            let sketches = std::mem::take(&mut assembly.sketches);
            let tags = std::mem::take(&mut assembly.tags);
            let (scene, views, objects, dimensions, notes, balloons) = assembly.finish()?;
            let generation = GENERATION.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
            (Arc::new(scene), views, objects.into(), dimensions, notes, balloons, table_states, tags, sketches, fills, generation)
        }
    };
    let (pictures, drawing_pictures) = sheet_pictures(s, &d, &on, &state);
    let cache = &mut s.drawing_scenes;
    let answer = SheetView {
        scene,
        generation,
        views,
        stale,
        objects,
        dimensions,
        stale_dimensions,
        notes: notes_drawn,
        stale_notes,
        balloons,
        stale_balloons,
        tables: table_states,
        tags,
        waiting_for_fonts,
        sketches,
        stale_sketches,
        fills,
        stale_fills,
        pictures,
        drawing_pictures,
    };
    let serial = cache.serial;
    cache.last = Some(Assembled {
        sheet,
        drawing: d,
        doc,
        state,
        lettering,
        cells,
        text,
        notes,
        labels,
        tables,
        pending,
        serial,
        inputs,
        answer: answer.clone(),
    });
    Ok(answer)
}

/// How long one [`step`] draws a picture's bands: about half a frame at 60 Hz, so the window keeps
/// drawing while a picture is. A step draws at least one band, and stops after the band that ends
/// past it.
const PICTURE_STEP: std::time::Duration = std::time::Duration::from_millis(8);

/// Does the provider's next piece of work on the sheet last extracted (`Next`), and says whether more
/// is waiting: a short piece of work for the UI's tick, one view's projection, one read of a view's
/// dimensions, one fill or sketch, or a picture's bands for about [`PICTURE_STEP`]. Text that waits
/// for its font is no work for it: [`extract`] sets it, and its answer says so
/// ([`SheetView::waiting_for_fonts`]).
pub fn step(s: &mut Session) -> bool {
    let started = web_time::Instant::now();
    step_within(s, &mut || started.elapsed() >= PICTURE_STEP)
}

/// [`step`], whose picture bands stop once `spent` says so: it is asked after each band.
fn step_within(s: &mut Session, spent: &mut dyn FnMut() -> bool) -> bool {
    let Some((d, on, state)) = drawable(s) else { return false };
    let Some(work) = next(s, &d, &on, &state) else {
        s.drawing_scenes.picturing = None;
        return false;
    };
    match work {
        Next::View(id, view) => {
            let done = job_for(s, &d, id, view.clone(), state.clone()).run();
            finish_view(s, done);
        }
        Next::Dimensions(stale) => step_dimensions(s, &d, &on, &state, stale),
        Next::Fills => step_fill(s, &d),
        Next::Sketches => step_sketch(s, &d),
        // [`begin`]'s job beside the window; done here first with none in hand, as on the web.
        Next::PictureSetup(waiting) => {
            if let Some(job) = picture_job(s, waiting, &state) {
                finish_picture(s, job.run());
            }
            draw_bands(s, spent);
        }
        Next::Bands => draw_bands(s, spent),
    }
    next(s, &d, &on, &state).is_some()
}

fn fills_waiting(s: &mut Session, d: &Arc<Drawing>) -> bool {
    let Some(sheet) = s.drawing_scenes.sheet else { return false };
    let (source, context) = s.drawing_scenes.sketch_inputs(&s.doc, d);
    fills_on(d, sheet).iter().any(|record| !s.drawing_scenes.fresh_fill(record.id, &source, &context))
}

fn step_fill(s: &mut Session, d: &Arc<Drawing>) {
    let Some(sheet) = s.drawing_scenes.sheet else { return };
    let (source, context) = s.drawing_scenes.sketch_inputs(&s.doc, d);
    let fill_waiting: Vec<_> = fills_on(d, sheet).into_iter().filter(|record| !s.drawing_scenes.fresh_fill(record.id, &source, &context)).collect();
    let Some(record) = fill_waiting.first() else { return };
    let fresh_regions = s
        .drawing_scenes
        .fill_regions
        .get(&record.sketch)
        .is_some_and(|entry| Arc::ptr_eq(&entry.source, &source) && Arc::ptr_eq(&entry.context, &context));
    if !fresh_regions {
        let regions = d.resolve_sheet_fill_regions(&s.doc, record.sketch).map_err(|error| error.to_string());
        s.drawing_scenes.fill_regions.insert(record.sketch, FillRegionEntry { source: source.clone(), context: context.clone(), regions });
        #[cfg(test)]
        {
            s.drawing_scenes.fill_solves += 1;
        }
    }
    let regions = s.drawing_scenes.fill_regions.get(&record.sketch).map(|entry| &entry.regions);
    let drawn = draw_fill(d, record, regions);
    let cache = &mut s.drawing_scenes;
    cache.serial = cache.serial.wrapping_add(1);
    cache.fills.insert(record.id, FillEntry { source, context, drawn, serial: cache.serial });
    #[cfg(test)]
    {
        cache.fill_expansions += 1;
    }
}

fn sketches_waiting(s: &mut Session, d: &Arc<Drawing>) -> bool {
    let Some(sheet) = s.drawing_scenes.sheet else { return false };
    let (source, context) = s.drawing_scenes.sketch_inputs(&s.doc, d);
    sketches_on(d, sheet).iter().any(|record| !s.drawing_scenes.fresh_sketch(record.id, &source, &context))
}

/// One bounded sketch solve after the view jobs and annotation reads have finished.
fn step_sketch(s: &mut Session, d: &Arc<Drawing>) {
    let Some(sheet) = s.drawing_scenes.sheet else { return };
    let (source, context) = s.drawing_scenes.sketch_inputs(&s.doc, d);
    let waiting: Vec<_> = sketches_on(d, sheet).into_iter().filter(|record| !s.drawing_scenes.fresh_sketch(record.id, &source, &context)).collect();
    let Some(record) = waiting.first() else { return };
    let drawn = draw_sketch(s, d, record);
    let cache = &mut s.drawing_scenes;
    cache.serial = cache.serial.wrapping_add(1);
    cache.sketches.insert(record.id, SketchEntry { source, context, drawn, serial: cache.serial });
    #[cfg(test)]
    {
        cache.sketch_solves += 1;
    }
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

/// The provider's next piece of work, in its one order: [`begin`] hands out the long kinds and
/// [`step`] does whichever comes, so the two never take work out of turn.
enum Next<'a> {
    /// The first view out of date of `on`, as it resolves: its lines, with its dimensions read in the
    /// same swap.
    View(u64, &'a ResolvedView),
    /// The dimensions and notes out of date on views up to date, by view and id ([`step_dimensions`]).
    Dimensions(Vec<Annotation>),
    /// The first fill out of date on the sheet ([`step_fill`]).
    Fills,
    /// The first sketch out of date on the sheet ([`step_sketch`]).
    Sketches,
    /// The setup of the first picture out of date ([`waiting_picture`]).
    PictureSetup((u64, Picture, DrawnFrom)),
    /// More bands of the picture being drawn, the first out of date.
    Bands,
}

/// What [`drawable`]'s `d`, `on` and `state` have the provider do next; None when nothing waits.
fn next<'a>(s: &mut Session, d: &Arc<Drawing>, on: &'a [(u64, ResolvedView, Vec2)], state: &Arc<ModelState>) -> Option<Next<'a>> {
    if let Some((id, view, _)) = on.iter().find(|(id, view, _)| !s.drawing_scenes.fresh(*id, state, view)) {
        return Some(Next::View(*id, view));
    }
    let stale = s.drawing_scenes.stale_dimensions(d, on, state, &Lettering::of(s, d), s.revision);
    if !stale.is_empty() {
        return Some(Next::Dimensions(stale));
    }
    if fills_waiting(s, d) {
        return Some(Next::Fills);
    }
    if sketches_waiting(s, d) {
        return Some(Next::Sketches);
    }
    let (view, picture, from) = waiting_picture(s, on, state)?;
    let drawing = |p: &Picturing| p.view == view && p.source == picture.source && p.from.same(&from);
    Some(if s.drawing_scenes.picturing.as_ref().is_some_and(drawing) { Next::Bands } else { Next::PictureSetup((view, picture, from)) })
}

/// A shown set, and the world state it was worked out for.
type ShownFor = (Arc<ModelState>, std::result::Result<ShownSet, Unmeasured>);

/// Work beside the window, one view's lines or a picture's setup: [`begin`] asks for it,
/// [`Job::run`] does it anywhere, and [`finish`] takes it.
pub struct Job(Work);

enum Work {
    View(ViewJob),
    Picture(PictureJob),
}

/// A job's work, for [`finish`].
pub struct Done(Worked);

enum Worked {
    // Boxed: a view's lines and a picture's start are about 0.8 and 1.4 KB, and a `Done` moves between threads.
    View(Box<ViewDone>),
    Picture(Box<PictureDone>),
}

impl Job {
    /// The view whose lines this works out, or none for a picture's setup: for a log line.
    pub fn view(&self) -> Option<u64> {
        match &self.0 {
            Work::View(job) => Some(job.ticket.view),
            Work::Picture(_) => None,
        }
    }

    /// Does the job's work: the one long piece of it, on any thread.
    pub fn run(self) -> Done {
        Done(match self.0 {
            Work::View(job) => Worked::View(Box::new(job.run())),
            Work::Picture(job) => Worked::Picture(Box::new(job.run())),
        })
    }
}

/// One view's lines, worked out beside the window.
struct ViewJob {
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
    drawing: u64,
    view: u64,
    resolved: ResolvedView,
    state: Arc<ModelState>,
    revision: u64,
    lettering: Lettering,
}

/// A view job's work: the view's lines and what they show, its dimensions and notes through its new
/// frame, and the shown set they were drawn from.
struct ViewDone {
    ticket: Ticket,
    drawn: Drawn,
    shows: Option<Arc<SourceKey>>,
    dimensions: Vec<Annotation>,
    lines: Vec<DrawnDimension>,
    /// The shown set of the design as it places its parts, for the cache; none from another state.
    shown: Option<ShownFor>,
}

impl ViewJob {
    /// Works the view's lines, dimensions and notes out: the one long piece of work, on any thread.
    fn run(self) -> ViewDone {
        let ViewJob { copy, refused, later, placed, drawing, ticket, shown } = self;
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
        let (copy, failing, refused) = match made {
            Some(Ok(Ok(made))) => (made.session, made.failing, None),
            Some(Ok(Err(why))) => (copy, Vec::new(), Some(why)),
            // Drawn as its placeholder, then failed, below.
            Some(Err(_)) => (copy, Vec::new(), Some(Unmeasured::NotConfigured)),
            None => (copy, Vec::new(), refused),
        };
        // A design whose parts can't be placed draws its views as placeholders (`shown` says why).
        let state = copy.world_state().unwrap_or_else(|_| copy.model.state());
        let shown = match (refused, shown) {
            (Some(why), _) => Err(why),
            (None, Some((was, shown))) if Arc::ptr_eq(&was, &state) => shown,
            _ => crate::drawing_view::shown(&copy),
        };
        // One view a job, so nothing it prepares is lent to another.
        let mut drawn = draw(&copy, &failing, &shown, &mut PreparedBodies::default(), ticket.view, &ticket.resolved);
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
        ViewDone { ticket, drawn, shows, dimensions, lines, shown }
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

/// Asks for the work [`step`] would do next that takes long, to do beside the window: the first view
/// out of date on the sheet last extracted or, once its dimensions are read too, the setup of the
/// first picture out of date. None when nothing such is waiting (a picture set up draws its bands in
/// [`step`]), or while a deferred drawing holds. One request per drawing: asking for that drawing
/// again replaces its earlier request, without cancelling another visible drawing's work.
pub fn begin(s: &mut Session) -> Option<Job> {
    let (d, on, state) = drawable(s)?;
    match next(s, &d, &on, &state)? {
        Next::View(id, view) => Some(Job(Work::View(job_for(s, &d, id, view.clone(), state)))),
        Next::PictureSetup(waiting) => picture_job(s, waiting, &state).map(|job| Job(Work::Picture(job))),
        Next::Dimensions(_) | Next::Fills | Next::Sketches | Next::Bands => None,
    }
}

/// Asks for view `id` of drawing `d`, resolved as `view`, of the world `state`: the latest request
/// from now on.
fn job_for(s: &mut Session, d: &Arc<Drawing>, id: u64, view: ResolvedView, state: Arc<ModelState>) -> ViewJob {
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
            Ok(copy) => (copy.session, None, None, None),
            Err(why) => (designed(s), Some(why), None, None),
        }
    };
    let ticket = Ticket { request, drawing: d.id, view: id, resolved: view, state, revision: s.revision, lettering };
    ViewJob { copy, refused, later, placed, drawing: d.clone(), ticket, shown }
}

/// Takes a job's work into the cache when it is for its latest request and nothing it was asked from
/// has changed since. Otherwise it is dropped and nothing changes. Says whether it was taken.
pub fn finish(s: &mut Session, done: Done) -> bool {
    forget_closed(s);
    let drawing = match &done.0 {
        Worked::View(done) => done.ticket.drawing,
        Worked::Picture(done) => done.ticket.drawing,
    };
    let previous = s.open_drawing;
    if previous != Some(drawing) {
        if !s.drawing_scenes.parked.contains_key(&drawing) {
            return false;
        }
        s.open_drawing = Some(drawing);
        forget_closed(s);
    }
    let accepted = match done.0 {
        Worked::View(done) => finish_view(s, *done),
        Worked::Picture(done) => finish_picture(s, *done),
    };
    if previous != Some(drawing) {
        s.open_drawing = previous;
        forget_closed(s);
    }
    accepted
}

/// [`finish`] for a view's lines: nothing they were asked from has changed, the drawing, the model
/// and the view as it resolves.
fn finish_view(s: &mut Session, done: ViewDone) -> bool {
    let ViewDone { ticket, drawn, shows, dimensions, lines, shown } = done;
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
    cache.serial = cache.serial.wrapping_add(1);
    let serial = cache.serial;
    // Its last pictures show, stale, until they are drawn again.
    let pixels = cache.views.remove(&ticket.view).map(|e| e.pixels).unwrap_or_default();
    let entry = Entry { state: ticket.state.clone(), shows, view: ticket.resolved, drawn, serial, pixels };
    cache.views.insert(ticket.view, entry);
    cache.keep_dimensions(dimensions, lines, &ticket.state, serial, &ticket.lettering, ticket.revision);
    #[cfg(test)]
    {
        cache.projections += 1;
    }
    true
}

/// Forgets the current drawing's request: its lines, when they come, are dropped. Another live
/// drawing's request remains independent.
pub fn cancel(s: &mut Session) {
    forget_closed(s);
    s.drawing_scenes.requested = None;
}

/// A copy of `s` as the design places its parts: an occurrence move not captured yet is left out.
pub(crate) fn designed(s: &Session) -> Session {
    let mut copy = s.scratch();
    copy.pending_moves.clear();
    copy
}

/// Reads the dimensions and notes of `stale` (`Next::Dimensions`) on the first view that has any,
/// through its frame.
fn step_dimensions(s: &mut Session, d: &Drawing, on: &[(u64, ResolvedView, Vec2)], state: &Arc<ModelState>, stale: Vec<Annotation>) {
    let (lettering, revision) = (Lettering::of(s, d), s.revision);
    let Some(view) = stale.first().map(Annotation::view) else { return };
    // Its copy evaluates the design, which only its job does: its job reads them again with its lines.
    if by_their_jobs(s, on).contains(&view) {
        s.drawing_scenes.redraw(view);
        return;
    }
    let these: Vec<_> = stale.into_iter().filter(|record| record.view() == view).collect();
    let entry = s.drawing_scenes.views.get(&view);
    let frame = entry.and_then(|e| Some((e.drawn.frame?, e.view.line_group.value)));
    let view_serial = entry.map_or(0, |e| e.serial);
    let lines = draw_dimensions(s, Some(&mut ViewSessions::new(s)), d, &these, |_| frame, false);
    let cache = &mut s.drawing_scenes;
    cache.serial = cache.serial.wrapping_add(1);
    cache.keep_dimensions(these, lines, state, view_serial, &lettering, revision);
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
    // Each with what it says now: a joint deleted may leave its picture as it was. A view whose row
    // fails to build features is of another configuration, which is left behind.
    let says = |view: &ResolvedView| crate::drawing_view::state_reason(s, view, &[]);
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
                let shown = sessions.drawing(choice).and_then(|(from, _)| crate::drawing_view::shown(from));
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

/// The bodies pictures are drawn from now, and their looks, read once for `state` and the design:
/// as the design places its parts, as the lines are drawn ([`designed`]). Looks equal to the last
/// ones keep their identity, so a change that leaves them as they were keeps every picture current.
/// Bodies that can't be read now (the model is mid-gesture) say why and leave the last looks as they
/// were, so the pictures drawn from them aren't drawn again for it.
fn bodies(s: &mut Session, state: &Arc<ModelState>) -> (std::result::Result<Arc<ProductScene>, String>, Arc<Looks>) {
    if let Some(b) = s.drawing_scenes.bodies.as_ref().filter(|b| Arc::ptr_eq(&b.state, state) && Arc::ptr_eq(&b.doc, &s.doc)) {
        return (b.scene.clone(), b.looks.clone());
    }
    let copy = designed(s);
    let scene = crate::product_scene::extract_where(&copy, |_| true).map_err(|e| e.to_string());
    if let (Err(why), Some(was)) = (&scene, &s.drawing_scenes.bodies) {
        return (Err(why.clone()), was.looks.clone());
    }
    let looks = copy.world_state().map_or(Looks(None), |placed| Looks::of(&placed, scene.as_ref().ok()));
    let looks = match &s.drawing_scenes.bodies {
        Some(was) if *was.looks == looks => was.looks.clone(),
        _ => Arc::new(looks),
    };
    let scene = scene.map(Arc::new);
    s.drawing_scenes.bodies = Some(Bodies { state: state.clone(), doc: s.doc.clone(), scene: scene.clone(), looks: looks.clone() });
    (scene, looks)
}

/// A view's pictures in its group, centred on it, bottom first.
fn pictures_of(drawn: &Drawn) -> impl Iterator<Item = &Picture> {
    drawn.group.iter().flat_map(|g| &g.items).filter_map(|item| if let Item::Picture(p) = item { Some(p) } else { None })
}

/// The first picture of a view of `on`, up to date itself, whose pixels aren't current, and what it
/// would be drawn from now. The bodies are read only when a view has a picture.
fn waiting_picture(s: &mut Session, on: &[(u64, ResolvedView, Vec2)], state: &Arc<ModelState>) -> Option<(u64, Picture, DrawnFrom)> {
    let fresh = |s: &Session| on.iter().filter(|(id, view, _)| s.drawing_scenes.fresh(*id, state, view)).map(|(id, ..)| *id).collect::<Vec<_>>();
    let shaded: Vec<u64> =
        fresh(s).into_iter().filter(|id| s.drawing_scenes.views.get(id).is_some_and(|e| pictures_of(&e.drawn).next().is_some())).collect();
    if shaded.is_empty() {
        return None;
    }
    let (looks, dpi) = (bodies(s, state).1, dpi_now(s));
    shaded.into_iter().find_map(|id| {
        let entry = s.drawing_scenes.views.get(&id)?;
        let now = DrawnFrom::of(entry, &looks, dpi);
        let mut pictures = pictures_of(&entry.drawn);
        let picture = pictures.find(|p| !entry.pixels.get(&p.source).is_some_and(|px| px.current(&now)))?;
        Some((id, *picture, now))
    })
}

/// The grid the page draws `picture` on: its own at `dpi`, or at `dpi / k` for the least whole `k`
/// that keeps it within [`WINDOW_PICTURE_PIXELS`].
fn window_grid(picture: &Picture, dpi: f64) -> std::result::Result<PixelGrid, String> {
    let size = picture.rect.size();
    let at = |dpi: f64| (size.x * dpi / 25.4) * (size.y * dpi / 25.4);
    // The least k the area allows, then the next few, as each side rounds up to whole pixels.
    let least = (at(dpi) / WINDOW_PICTURE_PIXELS as f64).sqrt().floor();
    let least = if least.is_finite() && least >= 1.0 { least.min(1e6) as u32 } else { 1 };
    for k in least..least.saturating_add(4) {
        match picture.grid(dpi / f64::from(k)) {
            Ok(grid) if grid.pixels() <= WINDOW_PICTURE_PIXELS => return Ok(grid),
            Ok(_) | Err(PixelGridError::TooLarge { .. }) => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Err(format!("its picture is more than {WINDOW_PICTURE_PIXELS} pixels at any dpi"))
}

/// The grid the page draws `picture` on at the sheet's `dpi` ([`window_grid`]), or why it has none.
fn page_grid(picture: &Picture, dpi: Option<f64>) -> std::result::Result<PixelGrid, String> {
    dpi.ok_or_else(|| "its sheet isn't open".to_string()).and_then(|dpi| window_grid(picture, dpi))
}

/// The grid of a picture with no pixels: none, at its rectangle's corner.
fn no_pixels(picture: &Picture) -> PixelGrid {
    PixelGrid { x0: picture.rect.min.x, y0: picture.rect.min.y, pitch: 1.0, width: 0, height: 0 }
}

/// Draws bands of the picture being drawn until `spent` says so, and keeps it once whole. A picture
/// set up for anything else is replaced by the next setup, so it is never kept stale; one that can't
/// be drawn keeps why ([`finish_picture`]), so it isn't tried again until its view or its bodies
/// change.
fn draw_bands(s: &mut Session, spent: &mut dyn FnMut() -> bool) {
    let cache = &mut s.drawing_scenes;
    let Some(picturing) = cache.picturing.as_mut() else { return };
    let rows = loop {
        #[cfg(test)]
        {
            cache.bands += 1;
        }
        match picturing.rows.band() {
            Ok(None) if !spent() => {}
            Ok(None) => return,
            Ok(Some(rows)) => break Ok(Arc::from(rows)),
            Err(why) => break Err(why),
        }
    };
    let Some(Picturing { view, source, from, grid, tests, .. }) = cache.picturing.take() else { return };
    keep_pixels(s, view, source, Pixels { grid, rows, from, tests });
}

/// A picture's setup, worked out beside the window: the shown set, the parts its view draws and the
/// rows to draw, from a copy of the session and the bodies read for it.
struct PictureJob {
    copy: Session,
    ticket: PictureTicket,
    /// Its view as it resolved when drawn.
    resolved: ResolvedView,
    /// Its grid on the page, or the grid it has none on and why.
    grid: std::result::Result<PixelGrid, (PixelGrid, String)>,
    scene: std::result::Result<Arc<ProductScene>, String>,
    /// The pixel tests its view's other pictures cost, as they are now.
    others: u64,
    /// The pixel tests its view's pictures may cost together.
    max_pixel_tests: u64,
}

/// What a picture's setup was asked from: its request, its view, what the picture is drawn from,
/// and the session's world state and revision then.
struct PictureTicket {
    request: u64,
    drawing: u64,
    revision: u64,
    state: Arc<ModelState>,
    view: u64,
    picture: Picture,
    from: DrawnFrom,
}

/// A picture's setup done: its grid, its pixel tests and its rows to draw, or its grid and why it
/// can't be drawn.
struct PictureDone {
    ticket: PictureTicket,
    started: std::result::Result<(PixelGrid, u64, PictureRows), (PixelGrid, String)>,
}

impl PictureJob {
    /// Sets the picture up from the frame and the parts its lines were drawn by ([`view_pictures`],
    /// centred on the view), within its view's pixel-test limit.
    fn run(self) -> PictureDone {
        let PictureJob { copy, ticket, resolved, grid, scene, others, max_pixel_tests } = self;
        let started = grid.and_then(|grid| {
            let fail = |why: String| (grid, why);
            let scene = scene.map_err(fail)?;
            let shown = crate::drawing_view::shown(&copy);
            let pictures = view_pictures(&shown, ticket.view, &resolved, Vec2::ZERO).map_err(|e| fail(reason(e)))?;
            let source = ticket.picture.source;
            let (_, wanted) = pictures.into_iter().find(|(s, _)| *s == source).ok_or_else(|| fail("its view no longer draws it".into()))?;
            let mut rows = PictureRows::new(&scene, &wanted, &grid, GridWindow { x0: 0, y0: 0, x1: grid.width, y1: grid.height }).map_err(fail)?;
            let tests = rows.pixel_tests();
            if others.saturating_add(tests) > max_pixel_tests {
                return Err(fail(over_budget(max_pixel_tests)));
            }
            rows.prepare().map_err(fail)?;
            Ok((grid, tests, rows))
        });
        PictureDone { ticket, started }
    }
}

/// Asks for the setup of `waiting` ([`waiting_picture`]) on its grid at the sheet's dpi: the latest
/// request from now on. None when its view isn't drawn.
fn picture_job(s: &mut Session, waiting: (u64, Picture, DrawnFrom), state: &Arc<ModelState>) -> Option<PictureJob> {
    let (view, picture, from) = waiting;
    let entry = s.drawing_scenes.views.get(&view)?;
    let resolved = entry.view.clone();
    let others = entry.pixels.iter().filter(|(source, p)| **source != picture.source && p.current(&from)).map(|(_, p)| p.tests).sum();
    let max_pixel_tests = s.drawing_scenes.max_pixel_tests.unwrap_or(MAX_PICTURE_PIXEL_TESTS);
    let grid = page_grid(&picture, from.dpi).map_err(|why| (no_pixels(&picture), why));
    let scene = bodies(s, state).0;
    let request = REQUEST.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    s.drawing_scenes.requested = Some(request);
    let ticket = PictureTicket { request, drawing: s.open_drawing?, revision: s.revision, state: state.clone(), view, picture, from };
    Some(PictureJob { copy: designed(s), ticket, resolved, grid, scene, others, max_pixel_tests })
}

/// [`finish`] for a picture's setup: nothing it was asked from has changed, the design, the model,
/// and what the picture is drawn from. Then [`step`] draws its bands, or it keeps why it can't be
/// drawn.
fn finish_picture(s: &mut Session, done: PictureDone) -> bool {
    let PictureDone { ticket, started } = done;
    if s.drawing_scenes.requested != Some(ticket.request) {
        return false;
    }
    s.drawing_scenes.requested = None;
    let holds = s.revision == ticket.revision
        && s.world_state().is_ok_and(|state| Arc::ptr_eq(&state, &ticket.state))
        && (s.drawing_scenes.views.get(&ticket.view).zip(s.drawing_scenes.bodies.as_ref()))
            .is_some_and(|(entry, b)| ticket.from.same(&DrawnFrom::of(entry, &b.looks, dpi_now(s))));
    if !holds {
        return false;
    }
    let PictureTicket { view, picture, from, .. } = ticket;
    match started {
        Ok((grid, tests, rows)) => s.drawing_scenes.picturing = Some(Picturing { view, source: picture.source, from, grid, tests, rows }),
        Err((grid, why)) => {
            s.drawing_scenes.picturing = None;
            keep_pixels(s, view, picture.source, Pixels { grid, rows: Err(why), from, tests: 0 });
        }
    }
    true
}

/// Keeps `pixels` as picture `source` of view `view`, and forgets its pixels of pictures it no
/// longer draws. The cache's serial moves, so [`extract`] answers again.
fn keep_pixels(s: &mut Session, view: u64, source: u64, pixels: Pixels) {
    let cache = &mut s.drawing_scenes;
    let Some(entry) = cache.views.get_mut(&view) else { return };
    let drawn: BTreeSet<u64> = pictures_of(&entry.drawn).map(|p| p.source).collect();
    entry.pixels.retain(|source, _| drawn.contains(source));
    entry.pixels.insert(source, pixels);
    cache.serial += 1;
}

/// Each picture of the views `on` as the page shows it, moved to its view's centre, and whether one
/// is waiting to be drawn: not while a deferred drawing holds, as nothing is drawn then.
fn sheet_pictures(s: &mut Session, d: &Drawing, on: &[(u64, ResolvedView, Vec2)], state: &Arc<ModelState>) -> (Arc<[SheetPicture]>, bool) {
    if !on.iter().any(|(id, ..)| s.drawing_scenes.views.get(id).is_some_and(|e| pictures_of(&e.drawn).next().is_some())) {
        return (Arc::from([]), false);
    }
    let (looks, dpi) = (bodies(s, state).1, dpi_now(s));
    let cache = &s.drawing_scenes;
    let held = cache.holds(d, state);
    let (mut pictures, mut waiting) = (Vec::new(), false);
    for (id, view, centre) in on {
        let Some(entry) = cache.views.get(id) else { continue };
        let fresh = cache.fresh(*id, state, view);
        for picture in pictures_of(&entry.drawn) {
            let pixels = entry.pixels.get(&picture.source);
            let current = fresh && pixels.is_some_and(|p| p.current(&DrawnFrom::of(entry, &looks, dpi)));
            waiting |= !current && !held;
            let (grid, rows, why) = match pixels {
                Some(p) => (p.grid, p.rows.as_ref().ok().cloned(), p.rows.as_ref().err().cloned()),
                None => match page_grid(picture, dpi) {
                    Ok(grid) => (grid, None, None),
                    Err(why) => (no_pixels(picture), None, Some(why)),
                },
            };
            let grid = PixelGrid { x0: grid.x0 + centre.x, y0: grid.y0 + centre.y, ..grid };
            pictures.push(SheetPicture { source: picture.source, view: *id, grid, rows, stale: !current, why });
        }
    }
    (pictures.into(), waiting)
}

/// Drawings with real, still-open Sheet tiles retain their live scenes and request tokens.
/// Legacy inline drawing sessions retain only their current cache, as before.
fn live_scene_drawings(s: &Session) -> BTreeSet<u64> {
    s.view_context
        .descriptors()
        .iter()
        .take(solidcraft_views::MAX_VIEWS)
        .filter_map(|view| {
            if view.kind != solidcraft_views::ViewKind::Sheet {
                return None;
            }
            let subject = view.subject.as_deref()?;
            if let Some((drawing, sheet)) = subject.split_once('/') {
                let (drawing, sheet) = (ids::decode(drawing)?, ids::decode(sheet)?);
                return s.doc.drawing(drawing)?.sheets.iter().any(|record| record.id == sheet).then_some(drawing);
            }
            let sheet = ids::decode(subject)?;
            let mut owners = s.doc.drawings.iter().filter(|drawing| drawing.sheets.iter().any(|record| record.id == sheet));
            let drawing = owners.next()?;
            owners.next().is_none().then_some(drawing.id)
        })
        .collect()
}

/// Release a closed tile's world states and in-flight work, keeping the existing bounded
/// last-line/picture fallback used when a drawing is reopened.
fn keep_closed_cache(kept: &mut KeptLines, drawing: u64, cache: Cache) {
    let Cache { views, dimensions, sketches, fills, .. } = cache;
    let sketches = sketches.into_iter().map(|(id, entry)| (id, KeptSketch { source: entry.source, context: entry.context, drawn: entry.drawn }));
    let fills = fills.into_iter().map(|(id, entry)| (id, KeptFill { source: entry.source, context: entry.context, drawn: entry.drawn }));
    kept.keep(drawing, Kept { sketches: sketches.collect(), fills: fills.collect(), ..Kept::closing(views, dimensions) });
}

/// Select the current drawing's cache without cancelling work on another visible Sheet.
/// At most MAX_VIEWS drawings can be parked. Closed/deleted tiles release their live caches;
/// authored data and the focused command receiver are untouched.
pub(crate) fn forget_closed(s: &mut Session) {
    let live = live_scene_drawings(s);
    let mut current = std::mem::take(&mut s.drawing_scenes);
    let mut parked = std::mem::take(&mut current.parked);
    let mut kept = std::mem::take(&mut current.kept);
    let retired: Vec<_> = parked.keys().copied().filter(|id| !live.contains(id) || s.doc.drawing(*id).is_none()).collect();
    for drawing in retired {
        if let Some(cache) = parked.remove(&drawing)
            && s.doc.drawing(drawing).is_some()
        {
            keep_closed_cache(&mut kept, drawing, *cache);
        }
    }
    let switches = current.drawing.is_some_and(|drawing| s.open_drawing != Some(drawing) || s.doc.drawing(drawing).is_none())
        || (current.drawing.is_none() && s.open_drawing.is_some_and(|drawing| parked.contains_key(&drawing)));
    if switches {
        if let Some(drawing) = current.drawing
            && s.doc.drawing(drawing).is_some()
        {
            if live.contains(&drawing) {
                parked.insert(drawing, Box::new(current));
            } else {
                keep_closed_cache(&mut kept, drawing, current);
            }
        }
        current = s.open_drawing.and_then(|drawing| parked.remove(&drawing)).map(|cache| *cache).unwrap_or_default();
    }
    kept.retain(|drawing| s.doc.drawings.iter().any(|record| record.id == drawing));
    current.parked = parked;
    current.kept = kept;
    s.drawing_scenes = current;
}

#[cfg(test)]
#[path = "drawing_scene_tests.rs"]
pub(crate) mod tests;
