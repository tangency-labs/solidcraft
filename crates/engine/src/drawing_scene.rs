//! The sheet scene (#4890 W10, #3890 W15): everything drawn on one sheet of a drawing as one paper
//! scene ([`Scene`]), for the sheet page and every export, so the screen and the files draw the
//! same thing. In paint order: the sheet's drawing border ([`SheetLayout::paper`]), then each view
//! on the sheet by id, its lines as [`paper_group`] makes them, then the dimensions of those
//! views, by view and id ([`crate::drawing_dimension_paper`]), then its title block's lines and
//! projection symbol, in the drawing's projection ([`TitleBlockLayout::paper`]), with its labels
//! and values set on them ([`title_block_text`]). Images placed on the sheet sit between the border
//! and the views ([`groups`]), so a view's lines draw over a photo. Tables join as one more kind of
//! group. A shaded view's group holds
//! its pictures under its lines; nothing here draws their pixels ([`crate::drawing_picture`]).
//!
//! [`build`] draws a sheet at once, each view the one way: its lines worked out centred on the view,
//! then moved to its centre, so the same lines on the same centre are the same bytes however they
//! got there. It reads the model as it is: no model is evaluated.
//!
//! The sheet page draws through the provider instead, which keeps each view's lines between frames:
//! [`extract`] returns the last scene at once and says which views are out of date, and [`step`]
//! brings one of them up to date, a short piece of work for the UI's tick: a view's projection with
//! its dimensions in the same swap, so new lines never show under old dimensions, or, once the views
//! are, the dimensions of one view that changed. A view moved across its sheet is moved, never
//! projected again, and an edited dimension is read again alone. A view drawn from a world state that
//! showed the same bodies, where and as they are now (`SourceKey`), is up to date: an appearance,
//! or an edit taken back, draws nothing. The provider evaluates no model either. Its cache is the
//! session's view state, beside the drawing: never saved, never an undo step, and dropped when its
//! drawing closes, all but its views' and dimensions' lines (`drawing_scene_kept.rs`), which the
//! drawing opened again starts from.
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
use solidcraft_doc::drawing::view::{ResolvedView, View};
use solidcraft_doc::drawing::{Drawing, Sheet, Updates};
use solidcraft_doc::drawing_view::TangentEdges;
use solidcraft_doc::{DocError, Document, ModelState, ids};
use solidcraft_geom::cell::Projection;
use solidcraft_geom::drawing_view::ViewFrame;
use solidcraft_geom::hlr::Source;
use solidcraft_geom::paper::line::LineGroup;
use solidcraft_geom::paper::text::{Font, TextRun, font_in};
use solidcraft_geom::paper::{Group, Item, MAX_RECORDS, ObjectKind, Picture, Scene};
use solidcraft_geom::prelude::*;
use solidcraft_geom::{Aabb2, ArcMap};
use solidcraft_sketch::PaperFonts;

use crate::drawing_picture::{picture_items, view_parts};
use crate::drawing_view::{CHORD_MM, LineRequest, MIN_MODEL_CHORD_MM, Shown, ShownSet, Stats, Styles, Unmeasured, paper_lines};
use crate::params::bad;
use crate::title_block_text::{self, CellText, Set};
use crate::{EngineError, Result, Session};

#[path = "drawing_scene_kept.rs"]
mod kept;
use kept::{Kept, KeptLines, KeptView};

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

/// Where one object of a sheet's scene sits on paper, for picking and marking it. Beside the scene,
/// not in it, so no writer sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SheetObject {
    pub object: u64,
    pub kind: ObjectKind,
    /// The box round what it draws, along its lines' paths; with no lines, its boundary.
    pub bounds_mm: Aabb2,
    /// A view's boundary ([`crate::drawing_view::on_sheet`]), so a view that can't be drawn can
    /// still be picked.
    pub boundary_mm: Option<Aabb2>,
}

/// One sheet drawn at once ([`build`]).
#[derive(Clone, Debug)]
pub struct SheetScene {
    pub scene: Scene,
    /// Each view on the sheet, in id order.
    pub views: Vec<ViewState>,
    /// The sheet's border, then each view on it, then each dimension drawn, in paint order.
    pub objects: Vec<SheetObject>,
    /// Each dimension of a view on the sheet, by view and id.
    pub dimensions: Vec<DimensionState>,
    /// The views and dimensions a deferred drawing shows as they last were ([`as_shown`]).
    pub out_of_date: Vec<u64>,
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
    /// A title block's or a dimension's text waits for the search for a font to fall back to: the
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
/// can't be drawn, or isn't in the scene, has no lines. No model is evaluated.
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
    // Worked out once for the sheet: every view draws from the same one. Each view's lines are
    // placed as they are drawn, so at most one view's are held beside the scene.
    let shown = crate::drawing_view::shown(s);
    let drawn = on.iter().map(|(id, view, centre)| (*id, *centre, draw(s, &shown, *id, view)));
    let (text, _) = block_text(record, &Lettering::of(s, drawing), &cell_texts(s, drawing, sheet), true)?;
    let mut assembly = assemble(record, drawing.projection, &text, &crate::drawing_pictures::groups(drawing, sheet), drawn)?;
    // After the views, as they need the views' frames: one read for the sheet's dimensions.
    let frames: BTreeMap<u64, ViewFrame> = assembly.placed.iter().map(|(view, (frame, _))| (*view, *frame)).collect();
    let wanted = dimensions_on(drawing, on.iter().map(|(id, ..)| *id));
    let group = |view: u64| on.binary_search_by_key(&view, |(id, ..)| *id).ok().and_then(|at| on.get(at)).map(|(_, v, _)| v.line_group.value);
    let dimensions = draw_dimensions(s, drawing, &wanted, |view| Some((*frames.get(&view)?, group(view)?)), true);
    assembly.add_dimensions(dimensions.iter());
    let (scene, views, objects, dimensions) = assembly.finish()?;
    Ok(SheetScene { scene, views, objects, dimensions, out_of_date: Vec::new() })
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
    // A held view's dimensions are out of date with it: listed after the views, never read again here
    // against its old lines. `stale_dimensions` holds those of the views up to date.
    let held = dimensions_on(d, shown.stale.iter().copied()).into_iter().map(|record| record.id);
    let out_of_date = shown.stale.iter().copied().chain(held).chain(shown.stale_dimensions).collect();
    let (scene, objects) = (Arc::unwrap_or_clone(shown.scene), shown.objects.to_vec());
    Ok(SheetScene { scene, views: shown.views, objects, dimensions: shown.dimensions, out_of_date })
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
    group: Option<Group>,
    status: Status,
    stats: Option<Stats>,
    /// Its boundary, centred on the view as its lines are.
    boundary: Option<Aabb2>,
    /// The frame its lines were drawn through, for its dimensions.
    frame: Option<ViewFrame>,
    /// What drew each item of `group`, for [`line_at`].
    owners: Option<Arc<Owners>>,
}

/// What drew each item of a view's group: for a stroke, the body (its index among `bodies`) and what
/// of it; nothing for a picture.
#[derive(Debug)]
struct Owners {
    bodies: Vec<Shown>,
    items: Vec<Option<(usize, Source)>>,
}

/// A dimension's lines for a sheet scene, centred on its view, and how it read.
#[derive(Clone, Debug)]
struct DrawnDimension {
    dimension: u64,
    view: u64,
    status: dimension::Status,
    group: Option<Group>,
    /// The fonts its group's text uses.
    fonts: Vec<Font>,
    why: Option<String>,
    /// Whether its text waits for a font to fall back to: [`extract`] reads it again until it doesn't.
    pending: bool,
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
    let none = |status| Drawn { group: None, status, stats: None, boundary, frame: None, owners: None };
    let styles = Styles::from(view);
    if let Some(why) = crate::drawing_view::not_drawable(view.visual_style.value, view.reference_style.value) {
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
            Drawn { group: Some(group), status: Status::Drawn, stats: Some(lines.stats), boundary, frame: Some(frame), owners }
        }
        Err(e) => none(Status::Failed(reason(e))),
    }
}

/// The records of `wanted` read once, each drawn through its view's frame (`None`: its view isn't
/// drawn), in its line group, in `wanted`'s order. A record that can't be read says so instead.
fn draw_dimensions(
    s: &Session,
    d: &Drawing,
    wanted: &[Arc<Dimension>],
    frame: impl Fn(u64) -> Option<(ViewFrame, LineGroup)>,
    wait: bool,
) -> Vec<DrawnDimension> {
    let ids: Vec<u64> = wanted.iter().map(|record| record.id).collect();
    let read = s.doc.resolve_dimensions(&s.model, d, &ids);
    let lettering = PaperFonts::new(&d.font, &d.language, wait);
    let one = |at: usize, record: &Arc<Dimension>| match read.as_ref().map(|all| all.get(at)) {
        Ok(Some(resolved)) => {
            let paper = frame(record.view)
                .and_then(|(frame, lines)| crate::drawing_dimension_paper::paper_group(record, resolved, &frame, lines, &lettering));
            let drawn = |group, fonts, why, pending| DrawnDimension {
                dimension: record.id,
                view: record.view,
                status: resolved.status.clone(),
                group,
                fonts,
                why,
                pending,
            };
            match paper {
                Some(paper) => drawn(Some(paper.group), paper.fonts, paper.why, paper.pending),
                None => {
                    let nothing = !resolved.points.is_empty() && frame(record.view).is_some();
                    drawn(None, Vec::new(), nothing.then(|| "what it measures has no place to draw on its view".to_string()), false)
                }
            }
        }
        Ok(None) | Err(_) => {
            let status = dimension::Status::Unmeasurable(format!("dimension {} can't be read", ids::encode(record.id)));
            DrawnDimension { dimension: record.id, view: record.view, status, group: None, fonts: Vec::new(), why: None, pending: false }
        }
    };
    wanted.iter().enumerate().map(|(at, record)| one(at, record)).collect()
}

/// The dimensions of `views` of `d`, by view and id.
fn dimensions_on(d: &Drawing, views: impl Iterator<Item = u64>) -> Vec<Arc<Dimension>> {
    let views: BTreeSet<u64> = views.collect();
    let mut on: Vec<Arc<Dimension>> = d.dimensions.iter().filter(|record| views.contains(&record.view)).cloned().collect();
    on.sort_by_key(|record| (record.view, record.id));
    on
}

/// A sheet's scene being put together: what [`assemble`] gives, for its dimensions to join.
struct Assembly {
    scene: Scene,
    /// The groups in the scene, as a paper scene counts them.
    records: usize,
    /// One group checked alone on the sheet before it joins, so one past the limits leaves out only
    /// itself, and checking costs one pass over each group.
    alone: Scene,
    states: Vec<ViewState>,
    objects: Vec<SheetObject>,
    /// The frame and centre of each view whose lines are in the scene.
    placed: BTreeMap<u64, (ViewFrame, Vec2)>,
    dimensions: Vec<DimensionState>,
    /// The sheet's title block, painted over everything by [`Assembly::finish`]: counted in `records`
    /// from the start, so nothing takes its records.
    block: Option<Group>,
}

/// Why a group stayed out of a scene.
enum LeftOut {
    TooManyRecords,
    Invalid(String),
}

impl Assembly {
    /// Puts `group`, whose text uses `fonts`, in the scene moved to `centre`, and gives the box round
    /// it there. Only a group that joins adds to the scene's fonts, and a font's records it adds count.
    fn add(&mut self, group: &Group, fonts: &[Font], centre: Vec2) -> std::result::Result<Aabb2, LeftOut> {
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
        self.scene.groups.append(&mut self.alone.groups);
        self.scene.add_fonts(std::mem::take(&mut self.alone.fonts));
        Ok(bounds)
    }

    /// Adds `dimensions` after the views, each moved to its view's centre. One whose view isn't in
    /// the scene or that would take it past a paper scene's limits is left out and says why.
    fn add_dimensions<'a>(&mut self, dimensions: impl Iterator<Item = &'a DrawnDimension>) {
        for one in dimensions {
            let mut why = one.why.clone();
            let mut drawn = false;
            match (self.placed.get(&one.view).map(|(_, centre)| *centre), &one.group) {
                (None, _) => why = Some("its view isn't drawn".into()),
                (Some(centre), Some(group)) => match self.add(group, &one.fonts, centre) {
                    Ok(bounds_mm) => {
                        drawn = true;
                        self.objects.push(SheetObject { object: one.dimension, kind: ObjectKind::Dimension, bounds_mm, boundary_mm: None });
                    }
                    Err(LeftOut::TooManyRecords) => {
                        why = Some(format!("with its lines the sheet is more than a paper scene's {MAX_RECORDS} records"))
                    }
                    Err(LeftOut::Invalid(e)) => why = Some(e),
                },
                (Some(_), None) => {}
            }
            self.dimensions.push(DimensionState { dimension: one.dimension, view: one.view, status: one.status.clone(), drawn, why });
        }
    }

    /// The scene with its title block on top, checked whole.
    fn finish(self) -> Result<(Scene, Vec<ViewState>, Vec<SheetObject>, Vec<DimensionState>)> {
        let mut scene = self.scene;
        scene.groups.extend(self.block);
        scene.validate().map_err(|e| bad(CMD, e.to_string()))?;
        Ok((scene, self.states, self.objects, self.dimensions))
    }
}

/// The scene of sheet `record`: its border, then `images`, then each of `views` (its id, centre and
/// lines, in id order) moved to its centre, with its title block, its symbol in `projection` and
/// its cells' text as [`block_text`] set it, to go on top ([`Assembly::finish`]). A view that would
/// take the scene past a paper scene's limits is left out and says why, and the views after it are
/// still drawn when they fit.
fn assemble<D: Borrow<Drawn>>(
    record: &Sheet,
    projection: Projection,
    text: &Set,
    images: &[Group],
    views: impl Iterator<Item = (u64, Vec2, D)>,
) -> Result<Assembly> {
    let size_mm = record.sheet.size_mm().map_err(|e| bad(CMD, e.to_string()))?;
    let layout = record.border.as_ref().map(|border| border.layout(&record.sheet)).transpose().map_err(|e| bad(CMD, e.to_string()))?;
    let mut border = layout.as_ref().map_or_else(Vec::new, |layout| layout.paper(record.id));
    // The block sits on the border. It's painted over the views, but counted with the border (its
    // text's fonts too), so no view takes its records.
    let (block, fonts) = match (&record.title_block, &layout) {
        (Some(block), Some(layout)) => {
            let placed = block.layout(layout).map_err(|e| bad(CMD, e.to_string()))?;
            let mut group = placed.paper(record.id, projection);
            group.items.extend(text.runs.iter().cloned().map(Item::Text));
            (Some(group), text.fonts.clone())
        }
        _ => (None, Vec::new()),
    };
    let mut objects = Vec::new();
    if !border.is_empty() {
        // The border's groups are one object, measured before the images painted with them join
        // them: an image isn't one of the objects a sheet's commands pick.
        let bounds_mm = border.iter().map(|group| group_bounds(group, &[])).fold(Aabb2::EMPTY, |all, one| all.union(&one));
        objects.push(SheetObject { object: record.id, kind: ObjectKind::DrawingBorder, bounds_mm, boundary_mm: None });
    }
    border.extend_from_slice(images);
    let records = border.iter().chain(&block).map(Group::records).fold(0usize, usize::saturating_add);
    let mut scene = Scene::new(size_mm, border);
    let alone = Scene::new(size_mm, Vec::with_capacity(1));
    // The block's fonts, each once: all they hold counts, as the table held none before.
    scene.add_fonts(fonts);
    let records = records.saturating_add(alone.font_records_beyond(&scene.fonts));
    let mut assembly = Assembly { scene, records, alone, states: Vec::new(), objects, placed: BTreeMap::new(), dimensions: Vec::new(), block };
    for (view, centre, drawn) in views {
        let drawn: &Drawn = drawn.borrow();
        let mut status = drawn.status.clone();
        let mut lines = None;
        if let Some(group) = &drawn.group {
            match assembly.add(group, &[], centre) {
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
        if let Some(bounds_mm) = lines.filter(|b| !b.is_empty()).or(boundary_mm) {
            assembly.objects.push(SheetObject { object: view, kind: ObjectKind::View, bounds_mm, boundary_mm });
        }
        assembly.states.push(ViewState { view, status, stats: drawn.stats });
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
fn moved(group: &Group, by: Vec2) -> Group {
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

/// A dimension's last drawing, and what it was drawn from: its record, the world state, its view's
/// drawing (by serial, which holds its frame) and the drawing's lettering.
#[derive(Clone)]
struct DimensionEntry {
    state: Arc<ModelState>,
    record: Arc<Dimension>,
    view_serial: u64,
    lettering: Lettering,
    drawn: DrawnDimension,
    serial: u64,
}

/// What a scene is assembled from: the sheet's record, the drawing's projection (its title block's
/// symbol), the images on the sheet, each view on it with its entry's serial (0: not drawn yet)
/// and its centre, and each dimension read so far with its entry's serial.
#[derive(PartialEq)]
struct Inputs {
    sheet: Sheet,
    projection: Projection,
    images: Vec<Group>,
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
    /// Whether a text of the block waits for a font to fall back to: until it doesn't, the block is
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
    /// ([`Session::scratch_as_shown`]): the world state its last [`update`] took, and each view's and
    /// dimension's last drawing, and the serial they were numbered to, so the copy's numbers never
    /// repeat theirs. Not the lines kept for drawings closed, the last answer or a request in flight,
    /// which only the session's own page uses; the drawing and sheet are set by the read.
    pub(crate) fn shown_copy(&self) -> Cache {
        Cache {
            views: self.views.clone(),
            dimensions: self.dimensions.clone(),
            updated: self.updated.clone(),
            serial: self.serial,
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
        // Each kept with an empty world state ([`forget_closed`]), so read again: its `view_serial`,
        // from before the close, never matches its view's new one either.
        for (id, mut entry) in kept.dimensions {
            self.serial += 1;
            entry.serial = self.serial;
            self.dimensions.insert(id, entry);
        }
    }

    /// `d` is deferred, and `state` isn't the one its last [`update`] took.
    fn holds(&self, d: &Drawing, state: &Arc<ModelState>) -> bool {
        d.updates == Updates::Deferred && !self.updated.as_ref().is_some_and(|updated| Arc::ptr_eq(updated, state))
    }

    /// Whether view `id`, drawing as `view` now, was last drawn so, from `state`.
    fn fresh(&self, id: u64, state: &Arc<ModelState>, view: &ResolvedView) -> bool {
        self.views.get(&id).is_some_and(|e| Arc::ptr_eq(&e.state, state) && e.view == *view)
    }

    /// What sheet `record` of `d`, holding the views `on` ([`views_on`]), is assembled from now.
    fn inputs(&self, d: &Drawing, record: &Sheet, on: &[(u64, ResolvedView, Vec2)]) -> Inputs {
        let serial = |id: &u64| self.views.get(id).map_or(0, |e| e.serial);
        let images = crate::drawing_pictures::groups(d, record.id);
        let views = on.iter().map(|(id, _, centre)| (*id, serial(id), *centre)).collect();
        let read = dimensions_on(d, on.iter().map(|(id, ..)| *id));
        let dimensions = read.iter().filter_map(|record| self.dimensions.get(&record.id).map(|e| (record.id, e.serial))).collect();
        Inputs { sheet: record.clone(), projection: d.projection, images, views, dimensions }
    }

    /// The views of the last scene assembled, while it is still its sheet's scene: the sheet as `d`
    /// has it, holding the same views, none drawn again since. So a view in it is on that sheet.
    fn assembled(&self, d: &Drawing) -> Option<&[ViewState]> {
        let last = self.last.as_ref()?;
        let on = views_on(d, last.sheet).ok()?;
        (self.inputs(d, sheet_of(d, last.sheet).ok()?, &on) == last.inputs).then_some(last.answer.views.as_slice())
    }

    /// Keeps `records` as `lines` drew them, numbered with the cache's serial.
    fn keep_dimensions(
        &mut self,
        records: Vec<Arc<Dimension>>,
        lines: Vec<DrawnDimension>,
        state: &Arc<ModelState>,
        view_serial: u64,
        lettering: &Lettering,
    ) {
        let serial = self.serial;
        for (record, drawn) in records.into_iter().zip(lines) {
            let entry = DimensionEntry { state: state.clone(), record, view_serial, lettering: lettering.clone(), drawn, serial };
            self.dimensions.insert(entry.record.id, entry);
        }
    }

    /// The dimensions of the views `on`, by view and id, to read again: each whose view is up to
    /// date, last drawn from another record, model, drawing of its view, or lettering.
    fn stale_dimensions(&self, d: &Drawing, on: &[(u64, ResolvedView, Vec2)], state: &Arc<ModelState>, lettering: &Lettering) -> Vec<Arc<Dimension>> {
        self.dimensions_where(d, on, state, lettering, |current| current.is_none())
    }

    /// The dimensions of the views `on`, by view and id, read as they are now but with text that
    /// waits for its font: [`extract`] sets it, as it has no lines to draw again.
    fn waiting_dimensions(
        &self,
        d: &Drawing,
        on: &[(u64, ResolvedView, Vec2)],
        state: &Arc<ModelState>,
        lettering: &Lettering,
    ) -> Vec<Arc<Dimension>> {
        self.dimensions_where(d, on, state, lettering, |current| current.is_some_and(|e| e.drawn.pending))
    }

    /// The dimensions of the up-to-date views `on` that `keep` takes, given each one's entry when it
    /// was read from this record, model, drawing of its view and lettering.
    fn dimensions_where(
        &self,
        d: &Drawing,
        on: &[(u64, ResolvedView, Vec2)],
        state: &Arc<ModelState>,
        lettering: &Lettering,
        keep: impl Fn(Option<&DimensionEntry>) -> bool,
    ) -> Vec<Arc<Dimension>> {
        let fresh = on.iter().filter(|(id, view, _)| self.fresh(*id, state, view));
        let serials: BTreeMap<u64, u64> = fresh.filter_map(|(id, ..)| self.views.get(id).map(|e| (*id, e.serial))).collect();
        let current = |record: &Arc<Dimension>, serial: u64| {
            self.dimensions
                .get(&record.id)
                .filter(|e| Arc::ptr_eq(&e.record, record) && Arc::ptr_eq(&e.state, state) && e.view_serial == serial && e.lettering == *lettering)
        };
        let mut found = dimensions_on(d, serials.keys().copied());
        found.retain(|record| serials.get(&record.view).is_some_and(|serial| keep(current(record, *serial))));
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

/// Each view of `d`, the drawing open, in id order, as the provider last drew it, whichever sheet it
/// is on. It reads the cache, caught up first ([`catch_up`]): nothing is drawn. A view that drew but
/// didn't fit its sheet's scene is as the last assembly left it, while that is still its sheet's
/// scene ([`Cache::assembled`]).
pub fn standing(s: &mut Session, d: &Drawing) -> Vec<ViewStanding> {
    forget_closed(s);
    let state = s.world_state().ok();
    if let Some(state) = &state {
        catch_up(s, d, d.views.iter().map(|v| v.id), state);
    }
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
    d.views.iter().map(one).collect()
}

/// Sheet `sheet` of the drawing open, as the provider last drew it: at once, as nothing is drawn
/// here. The first call for a sheet gives its border, with every view out of date.
pub fn extract(s: &mut Session, sheet: u64) -> Result<SheetView> {
    forget_closed(s);
    let d = s.drawing(CMD)?.clone();
    let state = s.world_state()?;
    let lettering = Lettering::of(s, &d);
    let on = views_on(&d, sheet)?;
    set_waiting_dimensions(s, &d, &on, &state, &lettering);
    let last = s.drawing_scenes.last.as_ref().filter(|last| !last.pending && last.reads(sheet, &d, &s.doc, &state, &lettering));
    if let Some(last) = last
        && last.serial == s.drawing_scenes.serial
    {
        return Ok(last.answer.clone());
    }
    // The title block's cells are read and set again only for another sheet, drawing, design, model
    // or lettering (a name or a mass can change them), not for each view drawn.
    let (cells, text, pending) = match last {
        Some(last) => (last.cells.clone(), last.text.clone(), false),
        None => {
            let cells = cell_texts(s, &d, sheet);
            let (text, pending) = block_text(sheet_of(&d, sheet)?, &lettering, &cells, false)?;
            (cells, Arc::new(text), pending)
        }
    };
    let doc = s.doc.clone();
    let cache = &mut s.drawing_scenes;
    let record = sheet_of(&d, sheet)?;
    cache.sheet = Some(sheet);
    catch_up(s, &d, on.iter().map(|(id, ..)| *id), &state);
    let cache = &mut s.drawing_scenes;
    let stale = on.iter().filter(|(id, view, _)| !cache.fresh(*id, &state, view)).map(|(id, ..)| *id).collect();
    let stale_dimensions = cache.stale_dimensions(&d, &on, &state, &lettering).iter().map(|record| record.id).collect();
    let waiting_for_fonts = pending || !cache.waiting_dimensions(&d, &on, &state, &lettering).is_empty();
    let inputs = cache.inputs(&d, record, &on);
    let (scene, views, objects, dimensions, generation) = match cache.last.take() {
        // Text set while a font was looked for can differ with the same cells and faces.
        Some(last) if !last.pending && !pending && last.inputs == inputs && last.cells == cells && last.text.faces() == text.faces() => {
            (last.answer.scene, last.answer.views, last.answer.objects, last.answer.dimensions, last.answer.generation)
        }
        _ => {
            // Views and dimensions no longer in the drawing are forgotten: each holds a world state.
            let kept: BTreeSet<u64> = d.views.iter().map(|v| v.id).collect();
            cache.views.retain(|id, _| kept.contains(id));
            cache.dimensions.retain(|id, _| d.dimension(*id).is_some());
            let drawn = on.iter().filter_map(|(id, _, centre)| cache.views.get(id).map(|e| (*id, *centre, &e.drawn)));
            let mut assembly = assemble(record, d.projection, &text, &inputs.images, drawn)?;
            assembly.add_dimensions(inputs.dimensions.iter().filter_map(|(id, _)| cache.dimensions.get(id).map(|e| &e.drawn)));
            let (scene, views, objects, dimensions) = assembly.finish()?;
            (Arc::new(scene), views, objects.into(), dimensions, GENERATION.fetch_add(1, Ordering::Relaxed).wrapping_add(1))
        }
    };
    let answer = SheetView { scene, generation, views, stale, objects, dimensions, stale_dimensions, waiting_for_fonts };
    cache.last =
        Some(Assembled { sheet, drawing: d, doc, state, lettering, cells, text, pending, serial: cache.serial, inputs, answer: answer.clone() });
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
    waiting.len() > 1 || !s.drawing_scenes.stale_dimensions(&d, &on, &state, &lettering).is_empty()
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
    /// The session as the design places its parts.
    copy: Session,
    /// The drawing open when the job was asked for: its records of the view's dimensions.
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

/// A job's work, for [`finish`]: the view's lines and what they show, its dimensions through its new
/// frame, and the shown set they were drawn from.
pub struct Done {
    ticket: Ticket,
    drawn: Drawn,
    shows: Option<Arc<SourceKey>>,
    dimensions: Vec<Arc<Dimension>>,
    lines: Vec<DrawnDimension>,
    shown: ShownFor,
}

impl Job {
    /// Works the view's lines and dimensions out: the one long piece of work, on any thread.
    pub fn run(self) -> Done {
        let Job { copy, drawing, ticket, shown } = self;
        // A design whose parts can't be placed draws its views as placeholders (`shown` says why).
        let state = copy.world_state().unwrap_or_else(|_| copy.model.state());
        let shown = match shown {
            Some((was, shown)) if Arc::ptr_eq(&was, &state) => shown,
            _ => crate::drawing_view::shown(&copy),
        };
        let drawn = draw(&copy, &shown, ticket.view, &ticket.resolved);
        let shows = SourceKey::of(&shown);
        // The view's dimensions through its new frame, taken in the same swap as its lines.
        let frame = drawn.frame.map(|frame| (frame, ticket.resolved.line_group.value));
        let dimensions = dimensions_on(&drawing, std::iter::once(ticket.view));
        let lines = draw_dimensions(&copy, &drawing, &dimensions, |_| frame, false);
        Done { ticket, drawn, shows, dimensions, lines, shown: (state, shown) }
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
    let ticket = Ticket { request, view: id, resolved: view, state, revision: s.revision, lettering };
    Job { copy: designed(s), drawing: d.clone(), ticket, shown: s.drawing_scenes.shown.take() }
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
    cache.shown = Some(shown);
    cache.serial += 1;
    let serial = cache.serial;
    let entry = Entry { state: ticket.state.clone(), shows, view: ticket.resolved, drawn, serial };
    cache.views.insert(ticket.view, entry);
    cache.keep_dimensions(dimensions, lines, &ticket.state, serial, &ticket.lettering);
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
fn designed(s: &Session) -> Session {
    let mut copy = s.scratch();
    copy.pending_moves.clear();
    copy
}

/// Reads the dimensions out of date of the first view that has any, through its frame, and says
/// whether another view's are waiting.
fn step_dimensions(s: &mut Session, d: &Drawing, on: &[(u64, ResolvedView, Vec2)], state: &Arc<ModelState>, lettering: Lettering) -> bool {
    let stale = s.drawing_scenes.stale_dimensions(d, on, state, &lettering);
    let Some(view) = stale.first().map(|record| record.view) else { return false };
    let (these, others): (Vec<_>, Vec<_>) = stale.into_iter().partition(|record| record.view == view);
    let entry = s.drawing_scenes.views.get(&view);
    let frame = entry.and_then(|e| Some((e.drawn.frame?, e.view.line_group.value)));
    let view_serial = entry.map_or(0, |e| e.serial);
    let lines = draw_dimensions(s, d, &these, |_| frame, false);
    let cache = &mut s.drawing_scenes;
    cache.serial += 1;
    cache.keep_dimensions(these, lines, state, view_serial, &lettering);
    !others.is_empty()
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

/// Sets the text of each dimension on `on` that waited for its font, once the search has answered:
/// one read of those dimensions alone, kept only when its text is set, so a text still waiting
/// changes nothing.
fn set_waiting_dimensions(s: &mut Session, d: &Drawing, on: &[(u64, ResolvedView, Vec2)], state: &Arc<ModelState>, lettering: &Lettering) {
    let waiting = s.drawing_scenes.waiting_dimensions(d, on, state, lettering);
    if waiting.is_empty() {
        return;
    }
    let cache = &s.drawing_scenes;
    let lines = draw_dimensions(s, d, &waiting, |view| cache.views.get(&view).and_then(|e| Some((e.drawn.frame?, e.view.line_group.value))), false);
    let set: Vec<(Arc<Dimension>, DrawnDimension)> = waiting.into_iter().zip(lines).filter(|(_, drawn)| !drawn.pending).collect();
    let cache = &mut s.drawing_scenes;
    for (record, drawn) in set {
        let Some(view_serial) = cache.dimensions.get(&record.id).map(|e| e.view_serial) else { continue };
        cache.serial += 1;
        cache.keep_dimensions(vec![record], vec![drawn], state, view_serial, lettering);
    }
}

/// Each view of `on` drawn from another world state that showed what `state` shows takes `state`:
/// drawn as it draws now, it is then up to date ([`Cache::fresh`]) and draws nothing. `state`'s shown
/// set is worked out once, only when a view is behind it, and kept for [`step`] to draw from.
fn keep_what_shows_the_same(s: &mut Session, on: impl Iterator<Item = u64> + Clone, state: &Arc<ModelState>) {
    let entry = |id: u64| s.drawing_scenes.views.get(&id);
    if !on.clone().any(|id| entry(id).is_some_and(|e| e.shows.is_some() && !Arc::ptr_eq(&e.state, state))) {
        return;
    }
    let shown = match s.drawing_scenes.shown.take() {
        Some((was, shown)) if Arc::ptr_eq(&was, state) => shown,
        _ => crate::drawing_view::shown(s),
    };
    let now = SourceKey::of(&shown);
    let cache = &mut s.drawing_scenes;
    cache.shown = Some((state.clone(), shown));
    let Some(now) = now else { return };
    for id in on {
        if let Some(entry) = cache.views.get_mut(&id).filter(|e| e.shows_the_same(state, &now)) {
            entry.state = state.clone();
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
        // Their world state is let go too: an empty one, which no state after the reopen is.
        let gone: Arc<ModelState> = Arc::default();
        let dimensions = std::mem::take(&mut cache.dimensions).into_iter().map(|(id, e)| (id, DimensionEntry { state: gone.clone(), ..e }));
        kept.keep(drawing, Kept { views: views.collect(), dimensions: dimensions.collect() });
        *cache = Cache { kept, ..Cache::default() };
    }
    cache.kept.retain(|drawing| s.doc.drawings.iter().any(|d| d.id == drawing));
}

#[cfg(test)]
#[path = "drawing_scene_tests.rs"]
pub(crate) mod tests;
