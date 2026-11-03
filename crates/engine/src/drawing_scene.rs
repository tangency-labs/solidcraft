//! The sheet scene (#4890 W10, #3890 W15): everything drawn on one sheet of a drawing as one paper
//! scene ([`Scene`]), for the sheet page and every export, so the screen and the files draw the
//! same thing. In paint order: the sheet's drawing border ([`SheetLayout::paper`]), then each view
//! on the sheet by id, its visible lines as [`paper_group`] makes them, then its title block's lines
//! and projection symbol, in the drawing's projection ([`TitleBlockLayout::paper`]). Images placed
//! on the sheet sit between the border and the views ([`groups`]), so a view's lines draw over a
//! photo. Text, tables and hidden lines each join as one more kind of group. A shaded view's group
//! holds its pictures under its lines; nothing here draws their pixels ([`crate::drawing_picture`]).
//!
//! [`build`] draws a sheet at once, each view the one way: its lines worked out centred on the view,
//! then moved to its centre, so the same lines on the same centre are the same bytes however they
//! got there. It reads the model as it is: no model is evaluated.
//!
//! The sheet page draws through the provider instead, which keeps each view's lines between frames:
//! [`extract`] returns the last scene at once and says which views are out of date, and [`step`]
//! brings one of them up to date, a short piece of work for the UI's tick. A view moved across its
//! sheet is moved, never projected again. The provider evaluates no model either. Its cache is the
//! session's view state, beside the drawing: never saved, never an undo step, and dropped when its
//! drawing closes.
//!
//! [`SheetLayout::paper`]: solidcraft_geom::drawing_sheet::SheetLayout::paper
//! [`groups`]: crate::drawing_pictures::groups
//! [`TitleBlockLayout::paper`]: solidcraft_geom::title_block::TitleBlockLayout::paper

use std::borrow::Borrow;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use solidcraft_doc::drawing::view::{ResolvedView, View};
use solidcraft_doc::drawing::{Drawing, Sheet};
use solidcraft_doc::drawing_view::TangentEdges;
use solidcraft_doc::{DocError, ModelState, ids};
use solidcraft_geom::cell::Projection;
use solidcraft_geom::paper::text::TextRun;
use solidcraft_geom::paper::{Group, Item, MAX_RECORDS, ObjectKind, Picture, Scene};
use solidcraft_geom::{Aabb2, ArcMap, Loop2, Region2, Seg2, Vec2};

use crate::drawing_picture::{picture_items, view_parts};
use crate::drawing_view::{CHORD_MM, LineRequest, MIN_MODEL_CHORD_MM, ShownSet, Stats, Styles, Unmeasured, paper_group};
use crate::params::bad;
use crate::{EngineError, Result, Session};

/// The command the sheet scene answers for, in its errors.
pub const CMD: &str = "drawing.scene";

/// Numbers every scene the provider assembles, in every session, so a page never sees one number
/// for two scenes, not even across closing and opening its drawing.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// What became of one view of a sheet scene.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Its visible lines are in the scene.
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
    /// The sheet's border, then each view on it, in paint order.
    pub objects: Vec<SheetObject>,
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
    /// The sheet's border, then each view drawn so far, where it was last drawn.
    pub objects: Arc<[SheetObject]>,
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
    // Worked out once for the sheet: every view draws from the same one. Each view's lines are
    // placed as they are drawn, so at most one view's are held beside the scene.
    let shown = crate::drawing_view::shown(s);
    let drawn = on.iter().map(|(id, view, centre)| (*id, *centre, draw(s, &shown, *id, view)));
    let (scene, views, objects) = assemble(record, drawing.projection, &crate::drawing_pictures::groups(drawing, sheet), drawn)?;
    Ok(SheetScene { scene, views, objects })
}

/// A view's lines for a sheet scene, centred on the view, or why it has none.
#[derive(Clone, Debug)]
struct Drawn {
    group: Option<Group>,
    status: Status,
    stats: Option<Stats>,
    /// Its boundary, centred on the view as its lines are.
    boundary: Option<Aabb2>,
}

/// View `id`, drawing as `view` does, of the bodies `shown` holds: its visible lines, sampled at
/// [`CHORD_MM`] on paper, or coarser past 100:1, where that would ask the mesh for chords finer than
/// [`MIN_MODEL_CHORD_MM`]; centred on the view ([`assemble`] moves them to its centre). Lines past
/// the paper scene's [`MAX_RECORDS`] are refused here, so no view holds more than a scene takes.
fn draw(s: &Session, shown: &std::result::Result<ShownSet, Unmeasured>, id: u64, view: &ResolvedView) -> Drawn {
    let measured = shown.as_ref().map_err(|why| *why).and_then(|set| {
        let extents = crate::drawing_view::measure(set, &view.basis)?;
        Ok((set, extents, crate::drawing_view::shows(set, &view.basis, &extents, view.reference_shown_percent)))
    });
    let placing = measured.as_ref().map(|(_, extents, around)| (*extents, *around)).map_err(|why| *why);
    let boundary = crate::drawing_view::placed(view, placing, Vec2::ZERO, CMD).ok().map(|placed| placed.boundary);
    let none = |status| Drawn { group: None, status, stats: None, boundary };
    let styles = Styles::from(view);
    // A style that draws hidden lines is refused here too, so the lines below are visible ones.
    if let Some(why) = crate::drawing_view::not_drawable(view.display_style.value, view.reference_style.value, true) {
        return none(Status::Failed(why.into()));
    }
    // Every body the design places, until saved views have `source` and `hide` (V4) to keep a part.
    // Its reference parts draw too, around what the extents measure.
    let (set, extents, around) = match measured {
        Ok(measured) => measured,
        Err(why) => return none(Status::Placeholder(why)),
    };
    let tangent_edges = view.tangent_edges.value == TangentEdges::Show;
    // The margin keeps the mesh's chord at or above its floor once divided back by the factor.
    let chord_mm = CHORD_MM.max(MIN_MODEL_CHORD_MM * view.scale.value.factor() * (1.0 + 1e-9));
    let reference_hides_model = view.reference_hides_model.value;
    let request = LineRequest { shows: around, hidden: false, reference_hidden: false, reference_hides_model, tangent_edges, chord_mm };
    let lines = crate::drawing_view::frame(view.basis, view.scale.value, &extents.model, Vec2::ZERO, CMD).and_then(|frame| {
        let rect = frame.boundary(&around).map_err(|e| bad(CMD, e.to_string()))?;
        let mut lines = crate::drawing_view::lines(s, set, &frame, &request, CMD)?;
        styles.leave_shaded_to_pictures(&mut lines);
        Ok((lines, rect))
    });
    match lines {
        Ok((lines, rect)) => {
            let mut group = paper_group(&lines, id);
            group.items.splice(0..0, picture_items(id, &view_parts(set, styles), rect));
            if group.records() > MAX_RECORDS {
                return none(Status::Failed(format!("its lines are more than a paper scene's {MAX_RECORDS} records: choose a smaller scale")));
            }
            Drawn { group: Some(group), status: Status::Drawn, stats: Some(lines.stats), boundary }
        }
        Err(e) => none(Status::Failed(reason(e))),
    }
}

/// The scene of sheet `record`: its border, then `images`, then each of `views` (its id, centre and
/// lines, in id order) moved to its centre, then its title block, its symbol in `projection`. A view
/// that would take the scene past a paper scene's limits is left out and says why, and the views
/// after it are still drawn when they fit.
fn assemble<D: Borrow<Drawn>>(
    record: &Sheet,
    projection: Projection,
    images: &[Group],
    views: impl Iterator<Item = (u64, Vec2, D)>,
) -> Result<(Scene, Vec<ViewState>, Vec<SheetObject>)> {
    let size_mm = record.sheet.size_mm().map_err(|e| bad(CMD, e.to_string()))?;
    let layout = record.border.as_ref().map(|border| border.layout(&record.sheet)).transpose().map_err(|e| bad(CMD, e.to_string()))?;
    let mut border = layout.as_ref().map_or_else(Vec::new, |layout| layout.paper(record.id));
    // The block sits on the border. It's painted over the views, but counted with the border, so no
    // view takes its records.
    let block = match (&record.title_block, &layout) {
        (Some(block), Some(layout)) => Some(block.layout(layout).map_err(|e| bad(CMD, e.to_string()))?.paper(record.id, projection)),
        _ => None,
    };
    let mut objects = Vec::new();
    if !border.is_empty() {
        // The border's groups are one object, measured before the images painted with them join
        // them: an image isn't one of the objects a sheet's commands pick.
        let bounds_mm = border.iter().map(group_bounds).fold(Aabb2::EMPTY, |all, one| all.union(&one));
        objects.push(SheetObject { object: record.id, kind: ObjectKind::DrawingBorder, bounds_mm, boundary_mm: None });
    }
    border.extend_from_slice(images);
    let mut records = border.iter().chain(&block).map(Group::records).fold(0usize, usize::saturating_add);
    let mut scene = Scene::new(size_mm, border);
    // Each view's group is counted onto the scene and checked alone on the sheet first, so one past
    // the limits leaves out only itself, and checking costs one pass over each group.
    let mut alone = Scene::new(size_mm, Vec::with_capacity(1));
    let mut states = Vec::new();
    for (view, centre, drawn) in views {
        let drawn: &Drawn = drawn.borrow();
        let mut status = drawn.status.clone();
        let mut lines = None;
        if let Some(group) = &drawn.group {
            let more = group.records();
            if records.saturating_add(more) > MAX_RECORDS {
                status = Status::Failed(format!(
                    "with its lines the sheet is more than a paper scene's {MAX_RECORDS} records: put it on another sheet, or choose a smaller scale"
                ));
            } else {
                alone.groups.push(moved(group, centre));
                match alone.validate() {
                    Ok(()) => {
                        records += more;
                        lines = alone.groups.first().map(group_bounds);
                        scene.groups.append(&mut alone.groups);
                    }
                    Err(e) => {
                        alone.groups.clear();
                        status = Status::Failed(format!("{e}: choose a smaller scale"));
                    }
                }
            }
        }
        let boundary_mm = drawn.boundary.map(|b| Aabb2 { min: b.min + centre, max: b.max + centre });
        if let Some(bounds_mm) = lines.filter(|b| !b.is_empty()).or(boundary_mm) {
            objects.push(SheetObject { object: view, kind: ObjectKind::View, bounds_mm, boundary_mm });
        }
        states.push(ViewState { view, status, stats: drawn.stats });
    }
    scene.groups.extend(block);
    scene.validate().map_err(|e| bad(CMD, e.to_string()))?;
    Ok((scene, states, objects))
}

/// The box round what `group` draws, along its lines' paths and round its pictures. Text isn't
/// measured here: nothing on a sheet draws any yet, and its outlines need its font.
fn group_bounds(group: &Group) -> Aabb2 {
    let mut bounds = Aabb2::EMPTY;
    for item in &group.items {
        let mut add = |path: &[Seg2]| path.iter().for_each(|seg| bounds = bounds.union(&seg.bounds()));
        match item {
            Item::Stroke { path, .. } => add(path),
            Item::Fill { region } => region.loops().for_each(|ring| add(&ring.segs)),
            Item::Text(_) => {}
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
    /// The shown set of the world state views were last drawn from, worked out once for it.
    shown: Option<(Arc<ModelState>, std::result::Result<ShownSet, Unmeasured>)>,
    last: Option<Assembled>,
    /// Numbers each view drawn, so a scene is assembled again only when one of its views was.
    serial: u64,
    /// Views drawn, for tests: a move draws none.
    #[cfg(test)]
    projections: usize,
}

/// A view's last drawing, and what it was drawn from: the world state (held, so its address can't
/// be reused while it is compared) and the view as it drew.
struct Entry {
    state: Arc<ModelState>,
    view: ResolvedView,
    drawn: Drawn,
    serial: u64,
}

/// What a scene is assembled from: the sheet's record, the drawing's projection (its title block's
/// symbol), the images on the sheet, and each view on it with its entry's serial (0: not drawn yet)
/// and its centre.
#[derive(PartialEq)]
struct Inputs {
    sheet: Sheet,
    projection: Projection,
    images: Vec<Group>,
    views: Vec<(u64, u64, Vec2)>,
}

/// The last answer of [`extract`], for the sheet, drawing, world state and serial it was for: the
/// same four again return it as it is.
struct Assembled {
    sheet: u64,
    drawing: Arc<Drawing>,
    state: Arc<ModelState>,
    serial: u64,
    inputs: Inputs,
    answer: SheetView,
}

impl Cache {
    /// Whether view `id`, drawing as `view` now, was last drawn so, from `state`.
    fn fresh(&self, id: u64, state: &Arc<ModelState>, view: &ResolvedView) -> bool {
        self.views.get(&id).is_some_and(|e| Arc::ptr_eq(&e.state, state) && e.view == *view)
    }

    /// What sheet `record` of `d`, holding the views `on` ([`views_on`]), is assembled from now.
    fn inputs(&self, d: &Drawing, record: &Sheet, on: &[(u64, ResolvedView, Vec2)]) -> Inputs {
        let serial = |id: &u64| self.views.get(id).map_or(0, |e| e.serial);
        let images = crate::drawing_pictures::groups(d, record.id);
        let views = on.iter().map(|(id, _, centre)| (*id, serial(id), *centre)).collect();
        Inputs { sheet: record.clone(), projection: d.projection, images, views }
    }

    /// The views of the last scene assembled, while it is still its sheet's scene: the sheet as `d`
    /// has it, holding the same views, none drawn again since. So a view in it is on that sheet.
    fn assembled(&self, d: &Drawing) -> Option<&[ViewState]> {
        let last = self.last.as_ref()?;
        let on = views_on(d, last.sheet).ok()?;
        (self.inputs(d, sheet_of(d, last.sheet).ok()?, &on) == last.inputs).then_some(last.answer.views.as_slice())
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
/// is on. It reads the cache: nothing is drawn. A view that drew but didn't fit its sheet's scene is
/// as the last assembly left it, while that is still its sheet's scene ([`Cache::assembled`]).
pub fn standing(s: &Session, d: &Drawing) -> Vec<ViewStanding> {
    let state = s.world_state();
    let cache = &s.drawing_scenes;
    let assembled = cache.assembled(d).unwrap_or_default();
    let in_assembly = |id: u64| assembled.binary_search_by_key(&id, |v| v.view).ok().and_then(|at| assembled.get(at));
    let one = |view: &Arc<View>| {
        let fresh = d.resolve_view(view.id).is_ok_and(|resolved| cache.fresh(view.id, &state, &resolved));
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
    let state = s.world_state();
    let cache = &mut s.drawing_scenes;
    if let Some(last) = &cache.last
        && last.sheet == sheet
        && last.serial == cache.serial
        && Arc::ptr_eq(&last.drawing, &d)
        && Arc::ptr_eq(&last.state, &state)
    {
        return Ok(last.answer.clone());
    }
    let record = sheet_of(&d, sheet)?;
    let on = views_on(&d, sheet)?;
    cache.drawing = Some(d.id);
    cache.sheet = Some(sheet);
    let stale = on.iter().filter(|(id, view, _)| !cache.fresh(*id, &state, view)).map(|(id, ..)| *id).collect();
    let inputs = cache.inputs(&d, record, &on);
    let (scene, views, objects, generation) = match cache.last.take() {
        Some(last) if last.inputs == inputs => (last.answer.scene, last.answer.views, last.answer.objects, last.answer.generation),
        _ => {
            // Views no longer in the drawing are forgotten: each holds a world state.
            let kept: BTreeSet<u64> = d.views.iter().map(|v| v.id).collect();
            cache.views.retain(|id, _| kept.contains(id));
            let drawn = on.iter().filter_map(|(id, _, centre)| cache.views.get(id).map(|e| (*id, *centre, &e.drawn)));
            let (scene, views, objects) = assemble(record, d.projection, &inputs.images, drawn)?;
            (Arc::new(scene), views, objects.into(), GENERATION.fetch_add(1, Ordering::Relaxed).wrapping_add(1))
        }
    };
    let answer = SheetView { scene, generation, views, stale, objects };
    cache.last = Some(Assembled { sheet, drawing: d, state, serial: cache.serial, inputs, answer: answer.clone() });
    Ok(answer)
}

/// Draws the first view out of date on the sheet last extracted, and says whether more are
/// waiting: a short piece of work for the UI's tick, one view's projection.
pub fn step(s: &mut Session) -> bool {
    forget_closed(s);
    let (Ok(d), Some(sheet)) = (s.drawing(CMD).cloned(), s.drawing_scenes.sheet) else { return false };
    let Ok(on) = views_on(&d, sheet) else { return false };
    let state = s.world_state();
    let waiting: Vec<(u64, ResolvedView)> =
        on.into_iter().filter(|(id, view, _)| !s.drawing_scenes.fresh(*id, &state, view)).map(|(id, view, _)| (id, view)).collect();
    let Some(&(id, view)) = waiting.first() else { return false };
    let shown = match s.drawing_scenes.shown.take() {
        Some((was, shown)) if Arc::ptr_eq(&was, &state) => shown,
        _ => crate::drawing_view::shown(s),
    };
    let drawn = draw(s, &shown, id, &view);
    let cache = &mut s.drawing_scenes;
    cache.shown = Some((state.clone(), shown));
    cache.serial += 1;
    cache.views.insert(id, Entry { state, view, drawn, serial: cache.serial });
    #[cfg(test)]
    {
        cache.projections += 1;
    }
    waiting.len() > 1
}

/// Drops the cache once its drawing isn't the one open: closing a drawing (or opening another) ends
/// its scenes, and the world states they hold. [`Session::execute`] calls it after every command.
pub(crate) fn forget_closed(s: &mut Session) {
    if s.drawing_scenes.drawing.is_some_and(|d| s.open_drawing != Some(d)) {
        s.drawing_scenes = Cache::default();
    }
}

#[cfg(test)]
#[path = "drawing_scene_tests.rs"]
pub(crate) mod tests;
