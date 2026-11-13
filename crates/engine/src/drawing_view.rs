//! Drawing views of the session's model (W07, #5866): what a view shows and how far it reaches,
//! and where a saved view sits on its sheet (#4816). The snapshot command and saved views use it;
//! their pictures (W17b) will too.
//!
//! A view shows the bodies the design places, where its occurrences place it ([`shown`]), less what
//! it chooses to leave out ([`ShownSet::of_view`]). The browser's eye counts for nothing: hiding a
//! part in the model never changes a drawing. A body placed through a reference occurrence is a
//! reference part: it doesn't set the view's extents when model parts are there too ([`measure`]).

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;
use std::sync::Arc;

use solidcore_record::{Plane, Vec2};
use solidcraft_doc::drawing::view::{ResolvedView, SetBy};
use solidcraft_doc::drawing::{Drawing, Sheet};
use solidcraft_doc::drawing_view::{Exploded, Pose, StudyStep};
use solidcraft_doc::{MAX_PLACEMENT_DEPTH, MAX_PLACEMENTS, ModelBody, ModelState, ids};
use solidcraft_geom::drawing_view::{Scale, ViewFrame};
use solidcraft_geom::{Aabb2, EPS};
use solidcraft_render::VisualStyle;

use crate::params::bad;
use crate::{Result, Session};

#[path = "drawing_view_lines.rs"]
mod lines;

pub use lines::{CHORD_MM, CHORD_RANGE, LineRequest, Lines, MIN_MODEL_CHORD_MM, Piece, Stats, ViewLine, lines, paper_group, paper_lines};

/// The paper millimetres each way of a view's boundary when there's nothing to measure, round its
/// centre, so it can still be picked, moved and deleted.
pub const PLACEHOLDER_MM: f64 = 20.0;

/// How far a view reaches on its own plane, in model millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Extents {
    /// The bodies' box in the plane's own coordinates ([`Plane::to_local`]).
    pub model: Aabb2,
    /// How far a curved face may lie outside its display mesh: the largest display tolerance.
    pub error_mm: f64,
    pub bodies: usize,
}

/// Why a view has nothing to measure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unmeasured {
    /// The assembly places more than a view walks, so a view of it would leave parts out.
    PastWalkLimit,
    NoBodies,
    /// The bodies of the assembly can't be placed.
    NotPlaced,
    /// The view's `source` and `hide` leave nothing, as when the occurrences it drew were deleted.
    NothingChosen,
    /// The exploded view it draws is no longer in the design.
    NoExplodedView,
    /// Its joints can't be driven to the position it draws.
    NotPosed,
}

/// Why a choice of occurrences that draws nothing is refused.
pub const NOTHING_CHOSEN: &str = "the source and hide leave no bodies to draw";

impl Unmeasured {
    /// Its name in replies.
    pub fn name(self) -> &'static str {
        match self {
            Unmeasured::PastWalkLimit => "past_walk_limit",
            Unmeasured::NoBodies => "no_bodies",
            Unmeasured::NotPlaced => "not_placed",
            Unmeasured::NothingChosen => "nothing_chosen",
            Unmeasured::NoExplodedView => "no_exploded_view",
            Unmeasured::NotPosed => "not_posed",
        }
    }
}

impl fmt::Display for Unmeasured {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Unmeasured::PastWalkLimit => write!(
                f,
                "the assembly is deeper than {MAX_PLACEMENT_DEPTH} levels or has more than {MAX_PLACEMENTS} placements: a view of it would leave parts out"
            ),
            Unmeasured::NoBodies => f.write_str("the view's source has no bodies"),
            Unmeasured::NotPlaced => f.write_str("the assembly's bodies can't be placed"),
            Unmeasured::NothingChosen => write!(f, "{NOTHING_CHOSEN}: the occurrences it chose may have been deleted"),
            Unmeasured::NoExplodedView => f.write_str("the exploded view it draws is no longer in the design"),
            Unmeasured::NotPosed => f.write_str("its position can't be placed"),
        }
    }
}

/// One body a view shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    /// Its index among the world state's bodies: the body in place, with its mesh.
    pub index: usize,
    /// The occurrences that place it, from the design's root.
    pub path: Vec<u64>,
    /// Its definition's body name.
    pub body: String,
    /// It is placed through a reference occurrence ([`solidcraft_doc::Document::reference_occurrences`]).
    pub reference: bool,
}

/// How a view draws its two kinds of part.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Styles {
    pub display: VisualStyle,
    pub reference: VisualStyle,
    pub reference_hides_model: bool,
}

impl From<&ResolvedView> for Styles {
    fn from(view: &ResolvedView) -> Styles {
        Styles { display: view.visual_style.value, reference: view.reference_style.value, reference_hides_model: view.reference_hides_model.value }
    }
}

impl Styles {
    /// Leaves out a shaded part's own lines: its picture draws it, and it still hides what lies
    /// behind it.
    pub fn leave_shaded_to_pictures(&self, lines: &mut Lines) {
        lines.lines.retain(|l| !(if l.reference { self.reference } else { self.display }).is_shaded());
    }
}

/// What a view shows, worked out once per request: the world state and each body it shows, in the
/// world state's order. The extents and the lines read it.
#[derive(Clone)]
pub struct ShownSet {
    pub state: Arc<ModelState>,
    pub bodies: Vec<Shown>,
}

impl ShownSet {
    /// The shown body in place.
    pub fn body(&self, shown: &Shown) -> Option<&ModelBody> {
        self.state.bodies.get(shown.index)
    }

    /// The bodies under one of the `source` occurrence paths (all of them when there is none;
    /// `[]` takes the design's own bodies) and under no `hide` path, in the same order.
    pub fn keep(mut self, source: Option<&[Vec<u64>]>, hide: &[Vec<u64>]) -> ShownSet {
        let source: Option<HashSet<&[u64]>> = source.map(|paths| paths.iter().map(Vec::as_slice).collect());
        let hide: HashSet<&[u64]> = hide.iter().map(Vec::as_slice).collect();
        self.bodies.retain(|b| chosen(&b.path, source.as_ref(), &hide));
        self
    }

    /// What saved view `view` shows of the set: [`ShownSet::keep`] by what it resolves to, which a
    /// view that chooses nothing skips. Paths no occurrence makes any more match no body.
    pub fn of_view(&self, view: &ResolvedView) -> Cow<'_, ShownSet> {
        let (source, hide) = (view.occurrences.value.as_deref(), view.hide.value.as_deref().unwrap_or_default());
        if source.is_none() && hide.is_empty() { Cow::Borrowed(self) } else { Cow::Owned(self.clone().keep(source, hide)) }
    }
}

/// Whether a body placed by `path` is drawn: under a `source` path (`[]` only for the design's own
/// bodies) and under no `hide` path. Each of the path's heads is one look-up, so a body costs at
/// most 64 of them however many paths there are.
fn chosen(path: &[u64], source: Option<&HashSet<&[u64]>>, hide: &HashSet<&[u64]>) -> bool {
    let heads = || (1..=path.len()).filter_map(|n| path.get(..n));
    let wanted = source.is_none_or(|set| if path.is_empty() { set.contains(path) } else { heads().any(|h| set.contains(h)) });
    wanted && !heads().any(|h| hide.contains(h))
}

/// Why a saved view doesn't draw what an occurrence path places ([`left_out`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeftOut {
    /// A `hide` path covers it: that path, and the view that set it.
    Hidden { by: Vec<u64>, set_by: SetBy },
    /// No `source` path takes it: the view that set the source.
    NotChosen { set_by: SetBy },
}

/// Why saved view `view` doesn't draw the body that occurrence path `path` places, if it doesn't:
/// [`chosen`] asked about one path, with the `hide` path that covers it. No model is read.
pub fn left_out(view: &ResolvedView, path: &[u64]) -> Option<LeftOut> {
    let source: Option<HashSet<&[u64]>> = view.occurrences.value.as_deref().map(|paths| paths.iter().map(Vec::as_slice).collect());
    let hide: HashSet<&[u64]> = view.hide.value.as_deref().unwrap_or_default().iter().map(Vec::as_slice).collect();
    if chosen(path, source.as_ref(), &hide) {
        return None;
    }
    let by = (1..=path.len()).filter_map(|n| path.get(..n)).find(|head| hide.contains(head));
    Some(match by {
        Some(by) => LeftOut::Hidden { by: by.to_vec(), set_by: view.hide.set_by },
        None => LeftOut::NotChosen { set_by: view.occurrences.set_by },
    })
}

/// The state of its assembly a view draws (#13048): its exploded choice and its position, the one key
/// of the session it draws from. A position's values are kept as their bits, by joint, so views with
/// the same position share one copy (`-0.0` is kept as `0.0`); a study step as itself, so views of one
/// step share one copy. A step and the values it gives, typed as joints, make two copies that draw alike.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Choice {
    exploded: Exploded,
    pose: Vec<(u64, Vec<u64>)>,
    study: Option<StudyStep>,
}

impl Choice {
    /// What saved view `view` draws.
    pub fn of(view: &ResolvedView) -> Choice {
        let mut pose: Vec<(u64, Vec<u64>)> = match &view.pose.value {
            Pose::Designed | Pose::Study(_) => Vec::new(),
            Pose::Joints(joints) => joints.iter().map(|j| (j.joint, j.values.iter().map(|v| (v + 0.0).to_bits()).collect())).collect(),
        };
        pose.sort();
        let study = if let Pose::Study(at) = view.pose.value { Some(at) } else { None };
        Choice { exploded: view.exploded.value, pose, study }
    }

    /// Whether it draws the design as it places its parts, as the default choice does.
    pub fn is_placed(&self) -> bool {
        self.exploded == Exploded::Assembled && self.pose.is_empty() && self.study.is_none()
    }
}

/// The sessions the views of one read of a drawing draw from (#13048): the design as it places its
/// parts, and a copy for each other state a view draws ([`Choice`]), made the first time a view asks
/// for it and shared by every view that draws it. A copy costs what placing every body again costs
/// (its world state misses the session's cache), so one read makes each at most once. The design is
/// never changed.
pub struct ViewSessions<'a> {
    placed: &'a Session,
    copies: BTreeMap<Choice, std::result::Result<Session, Unmeasured>>,
}

impl<'a> ViewSessions<'a> {
    /// The views' sessions, where `placed` is the design as it places its parts.
    pub fn new(placed: &'a Session) -> ViewSessions<'a> {
        ViewSessions { placed, copies: BTreeMap::new() }
    }

    /// The session saved view `view` draws from, or why it can't be drawn ([`copy_for`]).
    pub fn of(&mut self, view: &ResolvedView) -> std::result::Result<&Session, Unmeasured> {
        self.drawing(&Choice::of(view))
    }

    /// The session the views that draw `choice` draw from.
    pub fn drawing(&mut self, choice: &Choice) -> std::result::Result<&Session, Unmeasured> {
        if choice.is_placed() {
            return Ok(self.placed);
        }
        let placed = self.placed;
        self.copies.entry(choice.clone()).or_insert_with(|| copy_for(placed, choice)).as_ref().map_err(|why| *why)
    }

    /// How many copies it made.
    pub fn copies(&self) -> usize {
        self.copies.values().filter(|copy| copy.is_ok()).count()
    }

    /// The session view `view` of `d` draws from, where its dimensions, balloons and notes on its model
    /// are read, so they measure its parts where it draws them (#13048). A view whose copy can't be
    /// made draws its placeholder, so the sheet draws none of its records: they read as the design
    /// places its parts, as they did before each view read its own.
    pub fn of_view(&mut self, d: &Drawing, view: u64) -> &Session {
        let placed = self.placed;
        match d.resolve_view(view) {
            Ok(view) => self.drawing(&Choice::of(&view)).unwrap_or(placed),
            Err(_) => placed,
        }
    }

    /// What `read` (a resolver: [`solidcraft_doc::Document::resolve_dimensions`], `resolve_notes`
    /// or `resolve_balloons`) gives for `records` of `d`, each its id and its view's, each read on
    /// the session its view draws from ([`ViewSessions::of_view`]), in `records`' order. One call per
    /// state of the assembly, so each body's names are worked out once per state, not per record.
    pub fn read_on<T>(
        &mut self,
        d: &Drawing,
        records: &[(u64, u64)],
        read: impl Fn(&solidcraft_doc::Document, &solidcraft_doc::Model, &Drawing, &[u64]) -> solidcraft_doc::Result<Vec<T>>,
    ) -> solidcraft_doc::Result<Vec<T>> {
        let mut states: BTreeMap<Choice, Vec<usize>> = BTreeMap::new();
        for (at, (_, view)) in records.iter().enumerate() {
            let choice = d.resolve_view(*view).map(|view| Choice::of(&view)).unwrap_or_default();
            states.entry(choice).or_default().push(at);
        }
        let mut out: Vec<Option<T>> = records.iter().map(|_| None).collect();
        let placed = self.placed;
        for (choice, ats) in states {
            let from = self.drawing(&choice).unwrap_or(placed);
            let ids: Vec<u64> = ats.iter().filter_map(|at| records.get(*at)).map(|(id, _)| *id).collect();
            for (at, one) in ats.into_iter().zip(read(&from.doc, &from.model, d, &ids)?) {
                if let Some(slot) = out.get_mut(at) {
                    *slot = Some(one);
                }
            }
        }
        Ok(out.into_iter().flatten().collect())
    }
}

/// A copy of `placed` as a view drawing `choice` draws it: its position's joints driven to their
/// values as `joint.drive` drives them (the limits, closed loops, motion links and contact sets
/// alike), or to a study step's as `motion.play` drives them (a step past the study's end at its
/// last), skipping a joint or study the design no longer has, then its exploded view's parts moved
/// out from there, as `explode.show` shows them, written into the copy's own design so every reader
/// of where the parts are sees them. Or why it can't be: the exploded view is no longer in the design
/// or moves a part out of range (a hostile file), or the joints can't be driven.
pub fn copy_for(placed: &Session, choice: &Choice) -> std::result::Result<Session, Unmeasured> {
    use crate::cmd::motion::{drive_with_contact, explode_moves, exploded_at, keyed, values_at};
    // From where the design places its parts: an occurrence move not captured yet is left out.
    let mut copy = placed.scratch();
    copy.pending_moves.clear();
    // A study sets the values it keys, and the joints' other values follow the solve; joint values
    // set every value they give.
    let study = choice.study.and_then(|at| at.drawn(&copy.doc.assembly.studies));
    let (want, given): (Vec<(u64, Vec<f64>)>, BTreeSet<(u64, usize)>) = match study {
        Some((m, step)) => (values_at(&copy, m, f64::from(step)), keyed(m)),
        None => {
            let want: Vec<(u64, Vec<f64>)> =
                choice.pose.iter().map(|(joint, bits)| (*joint, bits.iter().map(|b| f64::from_bits(*b)).collect())).collect();
            let given = want.iter().flat_map(|(joint, values)| (0..values.len()).map(move |i| (*joint, i))).collect();
            (want, given)
        }
    };
    let joints = &copy.doc.assembly.joints;
    let want: Vec<(u64, Vec<f64>)> = want.into_iter().filter(|(joint, _)| joints.iter().any(|j| j.id == *joint)).collect();
    if !want.is_empty() {
        drive_with_contact(&mut copy, &want, &given).map_err(|_| Unmeasured::NotPosed)?;
    }
    if let Exploded::View(id) = choice.exploded {
        let view = copy.doc.assembly.exploded.iter().find(|v| v.id == id).ok_or(Unmeasured::NoExplodedView)?;
        let moves = exploded_at(&explode_moves(&copy, view), 1.0);
        if !moves.values().all(solidcraft_doc::is_placement) {
            return Err(Unmeasured::NotPlaced);
        }
        // What `world_state` would do with them as pending moves, once, in the copy's design.
        for o in &mut copy.doc_mut().occurrences {
            if let Some(m) = moves.get(&o.id) {
                o.transform = *m;
            }
        }
    }
    Ok(copy)
}

/// The joints saved view `view`'s position names that `s`'s design no longer has, in its order. The
/// view draws with the rest ([`copy_for`]).
pub fn gone_joints(s: &Session, view: &ResolvedView) -> Vec<u64> {
    let Pose::Joints(joints) = &view.pose.value else { return Vec::new() };
    joints.iter().map(|j| j.joint).filter(|id| !s.doc.assembly.joints.iter().any(|joint| joint.id == *id)).collect()
}

/// What the sheet says beside saved view `view` when its position names joints or a motion study
/// `s`'s design no longer has (#13048), as a drawn note says why. With none of its joints left, or
/// its study gone, it draws the design's placement.
pub fn position_reason(s: &Session, view: &ResolvedView) -> Option<String> {
    if let Pose::Study(at) = view.pose.value {
        let gone = at.drawn(&s.doc.assembly.studies).is_none();
        return gone.then(|| "its position names a motion study the design no longer has".into());
    }
    let gone = gone_joints(s, view).len();
    (gone > 0).then(|| format!("its position names {gone} joint{} the design no longer has", if gone == 1 { "" } else { "s" }))
}

/// What saved view `view` draws from `sessions`: every body its session places ([`shown`]), or why
/// there's nothing to show.
pub fn shown_by(sessions: &mut ViewSessions, view: &ResolvedView) -> std::result::Result<ShownSet, Unmeasured> {
    shown(sessions.of(view)?)
}

/// Every body the design places, or why there's nothing to show: the assembly places more than a
/// view walks, or it has no bodies.
pub fn shown(s: &Session) -> std::result::Result<ShownSet, Unmeasured> {
    if !s.doc.placements_complete() {
        return Err(Unmeasured::PastWalkLimit);
    }
    let state = s.world_state().map_err(|_| Unmeasured::NotPlaced)?;
    // The world state names each placed body as this walk does, so its name finds its path.
    let mut placed: BTreeMap<String, (Vec<u64>, String)> =
        s.doc.body_frames_with_source(&s.model.state()).into_iter().map(|(name, body, _, path, _)| (name, (path, body))).collect();
    let references = s.doc.reference_occurrences();
    let bodies: Vec<Shown> = state
        .bodies
        .iter()
        .enumerate()
        .map(|(index, b)| {
            let (path, body) = placed.remove(&b.name).unwrap_or_else(|| (Vec::new(), b.definition_name().to_string()));
            let reference = path.iter().any(|occurrence| references.contains(occurrence));
            Shown { index, path, body, reference }
        })
        .collect();
    if bodies.is_empty() { Err(Unmeasured::NoBodies) } else { Ok(ShownSet { state, bodies }) }
}

/// A saved view on its sheet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OnSheet {
    /// Its box on the sheet, in paper millimetres.
    pub boundary: Aabb2,
    /// Its design's extents on its plane; with nothing to measure, the placeholder's box there,
    /// [`PLACEHOLDER_MM`] each way on paper round the plane's origin.
    pub extents: Extents,
    /// Why there's nothing to measure, when there isn't.
    pub unmeasured: Option<Unmeasured>,
}

/// Why a view drawing in `display` and `reference` styles can't be drawn yet, if it can't: shaded
/// views with lines need the view's exact lines. The snapshot and the sheet scene refuse alike.
pub fn not_drawable(display: VisualStyle, reference: VisualStyle) -> Option<&'static str> {
    let lined = |style: VisualStyle| style.is_shaded() && style != VisualStyle::Shaded;
    (lined(display) || lined(reference)).then_some("not supported yet: shaded views with lines need the view's exact lines (W06)")
}

/// Why saved view `view` can't be drawn yet, if it can't, as it draws its parts apart (#13048): its
/// shaded parts' pictures are rendered from the design as it places its parts. Its lines alone draw.
/// Exploded comes first, as an exploded view of a position is still exploded.
pub fn not_drawable_apart(view: &ResolvedView) -> Option<&'static str> {
    if !(view.visual_style.value.is_shaded() || view.reference_style.value.is_shaded()) {
        None
    } else if view.exploded.value != Exploded::Assembled {
        Some("not supported yet: a shaded view of an exploded view")
    } else if view.pose.value != Pose::Designed {
        Some("not supported yet: a shaded view in a position of its own")
    } else {
        None
    }
}

/// What saved view `view` shows of `shown`, with its extents on its plane, or why there's nothing to
/// measure: the one rule its boundary, its fitted scale and its lines are worked out by.
pub fn measured<'a>(
    shown: &'a std::result::Result<ShownSet, Unmeasured>,
    view: &ResolvedView,
) -> std::result::Result<(Cow<'a, ShownSet>, Extents), Unmeasured> {
    let set = shown.as_ref().map_err(|why| *why)?.of_view(view);
    if set.bodies.is_empty() {
        return Err(Unmeasured::NothingChosen);
    }
    let extents = measure(&set, &view.basis)?;
    Ok((set, extents))
}

/// The extents of the bodies `shown` holds, seen on `basis`, or that they have nothing to measure:
/// of its model parts when it has any, as a reference part only shows around them, else of its
/// reference parts.
pub fn measure(shown: &ShownSet, basis: &Plane) -> std::result::Result<Extents, Unmeasured> {
    let has_model = shown.bodies.iter().any(|b| !b.reference);
    let bodies = shown.bodies.iter().filter(|b| !(has_model && b.reference)).filter_map(|b| shown.body(b));
    let extents = bodies.fold(Extents { model: Aabb2::EMPTY, error_mm: 0.0, bodies: 0 }, |e, body| Extents {
        model: e.model.union(&body.mesh_bounds_in(basis)),
        error_mm: e.error_mm.max(body.display_tol()),
        bodies: e.bodies + 1,
    });
    if extents.model.is_empty() { Err(Unmeasured::NoBodies) } else { Ok(extents) }
}

/// What a view of `shown` shows on `basis`: its `extents`, each side grown on its own towards the box
/// that also holds its reference parts, `percent` of the way. A side no reference part passes stays
/// exactly where it is, and at 0 no reference part is measured.
pub fn shows(shown: &ShownSet, basis: &Plane, extents: &Extents, percent: f64) -> Aabb2 {
    if percent <= 0.0 {
        return extents.model;
    }
    let references = shown.bodies.iter().filter(|b| b.reference).filter_map(|b| shown.body(b));
    let all = references.fold(extents.model, |all, body| all.union(&body.mesh_bounds_in(basis)));
    let part = percent / 100.0;
    let side = |from: f64, to: f64| if from == to { from } else { from * (1.0 - part) + to * part };
    let model = extents.model;
    Aabb2 {
        min: Vec2::new(side(model.min.x, all.min.x), side(model.min.y, all.min.y)),
        max: Vec2::new(side(model.max.x, all.max.x), side(model.max.y, all.max.y)),
    }
}

/// The view through `basis` at `scale` on paper, with the middle of the model's box (model mm on its
/// plane) at `centre`, however far reference parts grow what it shows, so it stays lined up with its
/// projected views: the one rule the snapshot and saved views are placed by.
pub fn frame(basis: Plane, scale: Scale, model: &Aabb2, centre: Vec2, cmd: &str) -> Result<ViewFrame> {
    ViewFrame::new(basis, scale, model.center(), centre).map_err(|e| bad(cmd, e.to_string()))
}

/// Saved view `id` of `drawing`, one of `s`'s design's drawings, on its sheet: the extents of what it
/// shows on its plane ([`measured`]; a view's source is the design the drawing is in), grown by
/// [`shows`], at its scale, with the model's middle on its centre ([`Drawing::view_centre`]); with nothing to
/// measure, a placeholder [`PLACEHOLDER_MM`] each way round its centre. It reads the model as it
/// is evaluated: no model is evaluated.
pub fn on_sheet(s: &Session, drawing: &Drawing, id: u64, cmd: &str) -> Result<OnSheet> {
    on_sheet_from(&mut ViewSessions::new(s), drawing, id, cmd)
}

/// [`on_sheet`], drawn from `sessions`: one read of several views makes each copy once.
pub fn on_sheet_from(sessions: &mut ViewSessions, drawing: &Drawing, id: u64, cmd: &str) -> Result<OnSheet> {
    let view = drawing.resolve_view(id).map_err(|e| bad(cmd, e.to_string()))?;
    let centre = drawing.view_centre(id).map_err(|e| bad(cmd, e.to_string()))?;
    let shown = shown_by(sessions, &view);
    let measured = measured(&shown, &view).map(|(set, e)| (e, shows(&set, &view.basis, &e, view.reference_shown_percent)));
    placed(&view, measured, centre, cmd)
}

/// A view drawing as `view` does on its sheet, from what [`measure`] and [`shows`] said of it, with the
/// model's middle on `centre`; with nothing to measure, the placeholder.
pub fn placed(view: &ResolvedView, measured: std::result::Result<(Extents, Aabb2), Unmeasured>, centre: Vec2, cmd: &str) -> Result<OnSheet> {
    let scale = view.scale.value;
    let (extents, shows, unmeasured) = match measured {
        Ok((extents, shows)) => (extents, shows, None),
        Err(why) => {
            let half = PLACEHOLDER_MM / 2.0 / scale.factor();
            let model = Aabb2 { min: Vec2::new(-half, -half), max: Vec2::new(half, half) };
            (Extents { model, error_mm: 0.0, bodies: 0 }, model, Some(why))
        }
    };
    let boundary = frame(view.basis, scale, &extents.model, centre, cmd)?.boundary(&shows).map_err(|e| bad(cmd, e.to_string()))?;
    Ok(OnSheet { boundary, extents, unmeasured })
}

/// Each of `views` of `drawing` (one of `s`'s design's) whose boundary leaves its sheet's drawing
/// space ([`Sheet::drawing_space`]), sides included, or lies over its title block, with where it
/// sits: what a change of paper or block, or a move to another sheet, leaves outside. Nothing is moved.
pub fn outside(s: &Session, drawing: &Drawing, views: &[u64], cmd: &str) -> Result<Vec<(u64, OnSheet)>> {
    let (mut out, mut sessions) = (Vec::new(), ViewSessions::new(s));
    for &id in views {
        let view = drawing.view(id).ok_or_else(|| bad(cmd, format!("no view {}", ids::encode(id))))?;
        let placed = on_sheet_from(&mut sessions, drawing, id, cmd)?;
        if leaves(sheet(drawing, view.sheet, cmd)?, &placed, cmd)? {
            out.push((id, placed));
        }
    }
    Ok(out)
}

/// Sheet `id` of `drawing`.
pub fn sheet<'a>(drawing: &'a Drawing, id: u64, cmd: &str) -> Result<&'a Sheet> {
    drawing.sheets.iter().find(|s| s.id == id).map(|s| &**s).ok_or_else(|| bad(cmd, format!("no sheet {}", ids::encode(id))))
}

/// Where a sheet's views belong: inside `space` and off `block`. One rule for the views [`outside`]
/// lists, the views `drawing.check` lists and the scale a view is fitted at.
#[derive(Clone, Copy, Debug)]
pub struct Room {
    pub space: Aabb2,
    pub block: Option<Aabb2>,
}

impl Room {
    /// `sheet`'s: its drawing space ([`Sheet::drawing_space`]) and its title block.
    pub fn on(sheet: &Sheet, cmd: &str) -> Result<Room> {
        let space = sheet.drawing_space().map_err(|e| bad(cmd, e.to_string()))?;
        let block = sheet.title_block_outline().map_err(|e| bad(cmd, e.to_string()))?;
        Ok(Room { space, block })
    }

    /// Whether `boundary` is off the block: drawn up to it, a side on its outline, it is.
    pub fn clear_of_block(&self, boundary: &Aabb2) -> bool {
        self.block.is_none_or(|block| !block.inflated(-EPS).overlaps(boundary))
    }

    /// Whether `boundary` is where a view belongs. Drawn up to the border, its sides on it, it is
    /// inside: the margin takes rounding.
    pub fn holds(&self, boundary: &Aabb2) -> bool {
        let space = self.space.inflated(EPS);
        space.contains(boundary.min) && space.contains(boundary.max) && self.clear_of_block(boundary)
    }
}

/// Whether a view `placed` on `sheet` isn't where its [`Room`] says: [`outside`]'s rule.
pub fn leaves(sheet: &Sheet, placed: &OnSheet, cmd: &str) -> Result<bool> {
    Ok(!Room::on(sheet, cmd)?.holds(&placed.boundary))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `source` path takes everything under it, `[]` only the design's own bodies, and a `hide`
    /// path wins over a `source` path above it.
    #[test]
    fn a_body_is_drawn_under_a_source_path_and_no_hide_path() {
        let set = |paths: &[&'static [u64]]| paths.iter().copied().collect::<HashSet<&[u64]>>();
        let paths: [&[u64]; 5] = [&[], &[1], &[1, 2], &[1, 2, 3], &[4]];
        let cases: [(Option<HashSet<&[u64]>>, HashSet<&[u64]>, [bool; 5]); 4] = [
            (None, set(&[&[1, 2]]), [true, true, false, false, true]),
            (Some(set(&[&[1, 2]])), set(&[]), [false, false, true, true, false]),
            (Some(set(&[&[]])), set(&[]), [true, false, false, false, false]),
            (Some(set(&[&[1], &[4]])), set(&[&[1, 2]]), [false, true, false, false, true]),
        ];
        for (source, hide, want) in cases {
            let got = paths.map(|p| chosen(p, source.as_ref(), &hide));
            assert_eq!(got, want, "{source:?} {hide:?}");
        }
    }

    /// Seeded paths and choices: `chosen` always agrees with the rule worked out the long way.
    #[test]
    fn seeded_choices_agree_with_the_rule() {
        let pick = |seed: u64, salt: u64, n: u64| ((seed ^ salt).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 33) % n;
        let path = |seed: u64, salt: u64| -> Vec<u64> { (0..pick(seed, salt, 4)).map(|i| 1 + pick(seed, salt + 10 + i, 3)).collect() };
        for seed in 1..=500u64 {
            let sources: Vec<Vec<u64>> = (0..pick(seed, 1, 3)).map(|i| path(seed, 100 + i * 20)).collect();
            let hides: Vec<Vec<u64>> = (0..pick(seed, 2, 3)).map(|i| path(seed, 200 + i * 20)).filter(|p| !p.is_empty()).collect();
            let body = path(seed, 3);
            let source = (pick(seed, 4, 2) == 0).then(|| sources.iter().map(Vec::as_slice).collect::<HashSet<&[u64]>>());
            let hide: HashSet<&[u64]> = hides.iter().map(Vec::as_slice).collect();
            let under = |p: &Vec<u64>| if p.is_empty() { body.is_empty() } else { body.starts_with(p) };
            let long_way = (source.is_none() || sources.iter().any(under)) && !hides.iter().any(under);
            assert_eq!(chosen(&body, source.as_ref(), &hide), long_way, "seed {seed}: {body:?} {sources:?} {hides:?}");
        }
    }
}
