//! A drawing view's exact lines (W07 V8, #5866): the shown bodies that `hlr` can draw from their
//! records, with their visibility worked out on the exact curves and surfaces. The edges drawn,
//! and whether each is a tangent edge, come from the caller ([`Edges`]) by the view's one rule
//! ([`mesh_lines::drawn_edges`]), so a view draws the same edges either way; `hlr` decides only
//! where each is seen.
//!
//! A body is drawn exact unless one of [`Fallback`]'s reasons holds; the caller draws the rest from
//! the mesh projection, which tests them against every shown body, as before.
//!
//! A sheet's views lend each other the bodies they prepare for `hlr` ([`PreparedBodies`], #13218):
//! preparing doesn't depend on the view, so each body is prepared once for the sheet.

use std::collections::BTreeMap;
use std::sync::Arc;

use solidcore_record::{Seg2, Vec2, Vec3};
use solidcraft_doc::ModelState;
use solidcraft_geom::Similarity2;
use solidcraft_geom::drawing_view::ViewFrame;
use solidcraft_geom::hlr::{self, HlrError, Source, Visibility};
use solidcraft_geom::prelude::*;
use solidcraft_geom::section::CurveAccuracy;

use super::ShownSet;
use super::lines::{LineRequest, Piece, ViewLine, continues};
use crate::mesh_lines::{self, Placed, Stage};

/// The edges the view draws of a body, in its record's order, and which of them are tangent edges.
pub(super) type Edges<'a> = dyn Fn(&Placed) -> (Vec<bool>, Vec<bool>) + 'a;

/// Why a shown body's lines come from the mesh instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Fallback {
    /// A mesh body: it has no record to draw from.
    MeshBody,
    /// A reference part: its lines stay thin and clipped to the view, from the mesh.
    Reference,
    /// A face `hlr` doesn't draw exactly: a freeform face, or a trim it can't classify.
    Unsupported,
    /// The body's record, or its edge numbers, couldn't be read for `hlr`.
    Unread,
    /// An edge the view draws that `hlr` doesn't: a split between two faces of one surface.
    Seam,
    /// A body drawn from the mesh may stand in front of it, and `hlr` can't see that body.
    InFront,
    /// `hlr` refused the view as a whole (past its budget, say).
    Refused,
}

impl Fallback {
    /// What the reason is, for a message.
    pub fn name(self) -> &'static str {
        match self {
            Fallback::MeshBody => "mesh bodies",
            Fallback::Reference => "reference parts",
            Fallback::Unsupported => "faces the exact lines can't draw",
            Fallback::Unread => "records that can't be read",
            Fallback::Seam => "split faces",
            Fallback::InFront => "a mesh body in front",
            Fallback::Refused => "more work than the exact lines allow",
        }
    }
}

/// The exact half of a view's lines.
pub(super) struct Exact {
    /// Per shown body, in the shown set's order: its lines, or why they come from the mesh.
    pub drawn: Vec<std::result::Result<Vec<ViewLine>, Fallback>>,
    /// Pieces drawn by another piece, left out.
    pub covered: usize,
    /// Pieces whose visibility `hlr` couldn't tell, drawn as visible.
    pub uncertain: usize,
    /// Bodies this view prepared for `hlr`; an earlier view of the sheet lent it the rest.
    pub prepared: usize,
    pub prepare_ms: f64,
    pub projection_ms: f64,
    pub occlusion_ms: f64,
}

/// One placement group's prepared body, and the edges the view draws of it and their tangent flags.
struct Group {
    def: usize,
    drawn: Vec<bool>,
    tangent: Vec<bool>,
}

/// The bodies one shown set's views have prepared for `hlr`, with [`options`]: each body is prepared
/// when the first view that shows it asks, and lent to the views after it. A view of another model
/// state prepares its own and lends nothing. A cancelled prepare leaves nothing behind.
#[derive(Default)]
pub struct PreparedBodies {
    state: Option<Arc<ModelState>>,
    defs: Vec<hlr::Prepared>,
    /// Per body, by its index among the state's bodies: its index in `defs`, or why it can't be
    /// drawn exact whatever the view.
    bodies: BTreeMap<usize, std::result::Result<usize, Fallback>>,
}

impl PreparedBodies {
    /// Whether these are, or can now become, the bodies of `state`.
    fn serves(&mut self, state: &Arc<ModelState>) -> bool {
        Arc::ptr_eq(self.state.get_or_insert_with(|| state.clone()), state)
    }
}

/// What a view asks of `hlr`: every edge it can draw (the view's rule picks which are kept), no
/// freeform faces (their bodies fall back), on the meshes' own worker cap, within `hlr`'s budgets.
pub(super) fn options() -> hlr::Options {
    hlr::Options { tangent_edges: true, freeform: false, threads: mesh_lines::WORKERS, ..hlr::Options::default() }
}

/// The exact lines of the bodies `placed` holds, one per shown body of `shown` and in its order, in
/// the view `frame` places on paper, as `request` asks, with `hlr`'s `opts` ([`options`]) and the
/// `edges` the view draws. Bodies come from `prepared` when an earlier view prepared them, and those
/// this view prepares are kept there. `None` when cancelled.
pub(super) fn lines(
    placed: &[Placed],
    shown: &ShownSet,
    frame: &ViewFrame,
    request: &LineRequest,
    opts: &hlr::Options,
    edges: &Edges,
    prepared: &mut PreparedBodies,
) -> Option<Exact> {
    let started = web_time::Instant::now();
    let stage = Stage::new("hidden_lines", placed.len());
    let cancel = || stage.check().is_err();
    let mut own = PreparedBodies::default();
    let prepared = if prepared.serves(&shown.state) { prepared } else { &mut own };
    let made = prepared.defs.len();
    let groups = prepare(placed, shown, opts, edges, &cancel, prepared)?;
    let (defs, prepared) = (&prepared.defs, prepared.defs.len() - made);
    let prepare_ms = started.elapsed().as_secs_f64() * 1000.;
    let group = |item: &Placed| groups.get(item.group).ok_or(Fallback::Unread).and_then(|g| g.as_ref().map_err(|f| *f));
    let reference = |i: usize| shown.bodies.get(i).is_some_and(|b| b.reference);
    let mut drawn: Vec<std::result::Result<Vec<ViewLine>, Fallback>> =
        placed.iter().enumerate().map(|(i, item)| group(item).and(if reference(i) { Err(Fallback::Reference) } else { Ok(Vec::new()) })).collect();
    // The `hlr` scene: every body it can draw, reference parts only when they hide the model, so a
    // body drawn exact is tested exactly against all of them.
    let mut scene = Vec::new();
    let mut shown_at = Vec::new();
    for (i, item) in placed.iter().enumerate() {
        let Ok(g) = group(item) else { continue };
        if reference(i) && !request.reference_hides_model {
            continue;
        }
        let column = |c: usize| item.transform.get(c).map_or(Vec3::ZERO, |c| Vec3::new(c[0], c[1], c[2]));
        scene.push(hlr::Placement { body: g.def, rotation: [column(0), column(1), column(2)], translation: column(3) });
        shown_at.push(i);
    }
    let basis = frame.basis();
    in_front(placed, shown, request, &basis, &shown_at, &mut drawn);
    let mut exact = Exact { drawn, covered: 0, uncertain: 0, prepared, prepare_ms, projection_ms: 0.0, occlusion_ms: 0.0 };
    if !exact.drawn.iter().any(Result::is_ok) {
        return Some(exact);
    }
    let view = hlr::View { x: basis.x, y: basis.y, d: basis.y.cross(basis.x) };
    let found = match hlr::hidden_lines(defs, &scene, &view, opts, &cancel) {
        Ok(found) => found,
        Err(HlrError::Cancelled) => return None,
        Err(_) => {
            refuse_all(&mut exact);
            return Some(exact);
        }
    };
    stage.advance(placed.len()).ok()?;
    let [traced, crossed, classified, covered] = found.stats.phase_ms;
    exact.projection_ms = traced + crossed;
    exact.occlusion_ms = classified + covered;
    // `hlr` measures x and y from the world origin, the view's plane from the basis's origin.
    let Some(shift) = Similarity2::new(1.0, 0.0, Vec2::new(-basis.origin.dot(basis.x), -basis.origin.dot(basis.y))) else {
        refuse_all(&mut exact);
        return Some(exact);
    };
    // Every edge the view draws has a line in `hlr`, hidden or not, unless `hlr` calls it a seam.
    let mut has_line: Vec<Vec<bool>> =
        shown_at.iter().map(|i| placed.get(*i).and_then(|p| group(p).ok()).map_or(Vec::new(), |g| vec![false; g.drawn.len()])).collect();
    for line in &found.lines {
        if let (Source::Edge { edge, .. }, Some(seen)) = (line.source, has_line.get_mut(line.placement))
            && let Some(slot) = seen.get_mut(edge)
        {
            *slot = true;
        }
    }
    for (at, i) in shown_at.iter().enumerate() {
        let (Some(item), Some(seen)) = (placed.get(*i), has_line.get(at)) else { continue };
        let all = group(item).is_ok_and(|g| g.drawn.iter().zip(seen).all(|(drawn, seen)| !drawn || *seen));
        if let Some(d) = exact.drawn.get_mut(*i).filter(|d| d.is_ok() && !all) {
            *d = Err(Fallback::Seam);
        }
    }
    for line in found.lines {
        let Some(&i) = shown_at.get(line.placement) else { continue };
        let Some(g) = placed.get(i).and_then(|p| group(p).ok()) else { continue };
        let source = match line.source {
            Source::Edge { edge, .. } if g.drawn.get(edge) == Some(&true) => Source::Edge { edge, tangent: g.tangent.get(edge) == Some(&true) },
            Source::Edge { .. } => continue,
            silhouette @ Source::Silhouette { .. } => silhouette,
        };
        let Some(Ok(lines)) = exact.drawn.get_mut(i) else { continue };
        let mut pieces = Vec::with_capacity(line.pieces.len());
        for piece in line.pieces {
            let Some(visibility) = shown_as(piece.visibility, piece.covered, request.hidden, &mut exact.covered, &mut exact.uncertain) else {
                continue;
            };
            // An edge seen end on is a point on the lines that meet at its ends, so `hlr` gives it as
            // covered; a piece with no length left is dropped with the paper scene's strokes.
            let (path, accuracy) = on_paper(&piece.geometry, piece.accuracy, frame, &shift, request.chord_mm);
            pieces.push(Piece { visibility, path, accuracy });
        }
        if !pieces.is_empty() {
            lines.push(ViewLine { shown: i, reference: false, source, pieces });
        }
    }
    Some(exact)
}

/// How the view draws a piece `hlr` sees at `visibility`, or `None` when it doesn't: a `covered`
/// piece is drawn by another (counted in `covered_count`), a hidden one only when `hidden` lines are
/// asked for, and an uncertain one as visible (counted in `uncertain`).
fn shown_as(visibility: Visibility, covered: bool, hidden: bool, covered_count: &mut usize, uncertain: &mut usize) -> Option<Visibility> {
    if covered {
        *covered_count += 1;
        return None;
    }
    match visibility {
        Visibility::Hidden if !hidden => None,
        Visibility::Uncertain => {
            *uncertain += 1;
            Some(Visibility::Visible)
        }
        v => Some(v),
    }
}

/// Every body still drawn exact sent to the mesh: `hlr` couldn't draw the view.
fn refuse_all(exact: &mut Exact) {
    for d in exact.drawn.iter_mut().filter(|d| d.is_ok()) {
        *d = Err(Fallback::Refused);
    }
}

/// Per placement group, its first body as `prepared` holds it, prepared for `hlr` with `opts` when no
/// earlier view has, or why it can't be drawn exact; with the `edges` this view draws of it. Each
/// body is known by its index among the shown set's state's bodies. `None` when cancelled, keeping
/// only the bodies prepared whole.
fn prepare(
    placed: &[Placed],
    shown: &ShownSet,
    opts: &hlr::Options,
    edges: &Edges,
    cancel: &(dyn Fn() -> bool + Sync),
    prepared: &mut PreparedBodies,
) -> Option<Vec<std::result::Result<Group, Fallback>>> {
    let mut groups = Vec::new();
    // Groups are numbered in the order of their first bodies.
    for (i, item) in placed.iter().enumerate().filter(|(_, item)| item.first) {
        let index = shown.bodies.get(i).map(|b| b.index);
        let def = match index.and_then(|index| prepared.bodies.get(&index)) {
            Some(known) => *known,
            None => {
                let made = prepare_body(item, opts, cancel, &mut prepared.defs)?;
                if let Some(index) = index {
                    prepared.bodies.insert(index, made);
                }
                made
            }
        };
        // The edges come in the record's order, as `hlr`'s do; this view's must all be `hlr`'s.
        groups.push(def.and_then(|def| {
            let (drawn, tangent) = edges(item);
            let all = prepared.defs.get(def).is_some_and(|d| d.report.edges == drawn.len());
            if all { Ok(Group { def, drawn, tangent }) } else { Err(Fallback::Unread) }
        }));
    }
    Some(groups)
}

/// `item`'s body prepared for `hlr` with `opts`, pushed onto `defs`: its index there, or why it can't
/// be drawn exact whatever the view. `None` when cancelled.
fn prepare_body(
    item: &Placed,
    opts: &hlr::Options,
    cancel: &(dyn Fn() -> bool + Sync),
    defs: &mut Vec<hlr::Prepared>,
) -> Option<std::result::Result<usize, Fallback>> {
    let body = &item.body.body;
    if body.is_mesh() {
        return Some(Err(Fallback::MeshBody));
    }
    Some(match hlr::prepare_cancellable(body.record(), opts, cancel) {
        Err(HlrError::Cancelled) => return None,
        Err(_) => Err(Fallback::Unread),
        Ok(def) if !(def.report.freeform_faces.is_empty() && def.report.unsupported_trims.is_empty()) => Err(Fallback::Unsupported),
        Ok(def) => {
            defs.push(def);
            Ok(defs.len() - 1)
        }
    })
}

/// Sends to the mesh each body `drawn` still draws exact that a body outside the `hlr` scene could
/// hide: one whose box on the view overlaps it and reaches nearer the eye somewhere. The boxes are
/// the bodies' exact boxes (a mesh body's are its triangles'), read only when there is such a body.
fn in_front(
    placed: &[Placed],
    shown: &ShownSet,
    request: &LineRequest,
    basis: &solidcore_record::Plane,
    shown_at: &[usize],
    drawn: &mut [std::result::Result<Vec<ViewLine>, Fallback>],
) {
    let mut in_scene = vec![false; placed.len()];
    for i in shown_at {
        if let Some(slot) = in_scene.get_mut(*i) {
            *slot = true;
        }
    }
    let hides = |i: usize| !shown.bodies.get(i).is_some_and(|b| b.reference) || request.reference_hides_model;
    let outside: Vec<usize> = (0..placed.len()).filter(|i| in_scene.get(*i) == Some(&false) && hides(*i)).collect();
    if outside.is_empty() {
        return;
    }
    // The body's box on the view: x and y across it, and its depth along the line of sight.
    let d = basis.y.cross(basis.x);
    let on_view = |i: usize| {
        let b = placed.get(i)?.body.body.exact_bounds().ok()?;
        let corners = (0..8).map(|k| {
            Vec3::new(
                if k & 1 == 0 { b.min.x } else { b.max.x },
                if k & 2 == 0 { b.min.y } else { b.max.y },
                if k & 4 == 0 { b.min.z } else { b.max.z },
            )
        });
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for c in corners {
            for (k, v) in [c.dot(basis.x), c.dot(basis.y), c.dot(d)].into_iter().enumerate() {
                lo[k] = lo[k].min(v);
                hi[k] = hi[k].max(v);
            }
        }
        lo.iter().chain(&hi).all(|v| v.is_finite()).then_some((lo, hi))
    };
    let blockers: Vec<Option<([f64; 3], [f64; 3])>> = outside.iter().map(|i| on_view(*i)).collect();
    for (i, slot) in drawn.iter_mut().enumerate().filter(|(_, slot)| slot.is_ok()) {
        let mine = on_view(i);
        // A box that can't be read could stand anywhere.
        let hidden_by = |other: &Option<([f64; 3], [f64; 3])>| match (mine, other) {
            (Some((lo, hi)), Some((olo, ohi))) => olo[0] < hi[0] && lo[0] < ohi[0] && olo[1] < hi[1] && lo[1] < ohi[1] && olo[2] < hi[2],
            _ => true,
        };
        if blockers.iter().any(hidden_by) {
            *slot = Err(Fallback::InFront);
        }
    }
}

/// A piece's `geometry` (screen millimetres from the world origin) on paper, as lines and arcs,
/// with its accuracy there. A conic that is a circular arc becomes that arc, and arcs of one circle
/// join; another conic (a circle seen at an angle) is cut into chords within `chord` on paper, a
/// proven bound. Anything else is chords of its polyline, sampled.
fn on_paper(geometry: &[Seg2], accuracy: CurveAccuracy, frame: &ViewFrame, shift: &Similarity2, chord: f64) -> (Vec<Seg2>, CurveAccuracy) {
    let mut path: Vec<Seg2> = Vec::with_capacity(geometry.len());
    let (mut flattened, mut proven) = (false, true);
    for seg in geometry {
        match frame.seg_to_paper(&seg.similar(shift)) {
            seg @ (Seg2::Line { .. } | Seg2::Arc { .. }) => joined(&mut path, seg),
            Seg2::Conic { a, apex, b, w } => match circular(a, apex, b, w) {
                Some(arc) => joined(&mut path, arc),
                None => {
                    flattened = true;
                    proven &= chords(Seg2::Conic { a, apex, b, w }, chord, 0, &mut path);
                }
            },
            other => {
                (flattened, proven) = (true, false);
                path.extend(other.polyline(chord).windows(2).filter_map(|w| Some(Seg2::Line { a: *w.first()?, b: *w.get(1)? })));
            }
        }
    }
    let accuracy = match (frame.accuracy_on_paper(accuracy), flattened, proven) {
        (on_paper, false, _) => on_paper,
        (CurveAccuracy::Analytic, true, true) => CurveAccuracy::Bounded { max_deviation_mm: chord },
        (CurveAccuracy::Bounded { max_deviation_mm }, true, true) => CurveAccuracy::Bounded { max_deviation_mm: max_deviation_mm + chord },
        (CurveAccuracy::Analytic, true, false) => CurveAccuracy::Sampled { max_sample_deviation_mm: chord },
        (CurveAccuracy::Bounded { max_deviation_mm: off } | CurveAccuracy::Sampled { max_sample_deviation_mm: off }, true, _) => {
            CurveAccuracy::Sampled { max_sample_deviation_mm: off + chord }
        }
    };
    (path, accuracy)
}

/// `seg` added to the end of `path`, joined to the arc before it when it goes on along its circle.
fn joined(path: &mut Vec<Seg2>, seg: Seg2) {
    if let (Some(Seg2::Arc { center, radius, start, sweep }), Seg2::Arc { center: c, radius: r, start: s, sweep: turn }) = (path.last_mut(), seg)
        && continues((*center, *radius, *start, *sweep), (c, r, s, turn))
        && (*sweep + turn).abs() <= std::f64::consts::TAU + 1e-9
    {
        *sweep += turn;
        return;
    }
    path.push(seg);
}

/// The circular arc the conic from `a` round `apex` to `b` with weight `w` is, or `None` when it
/// is another conic: its legs are equal, and `w` is the cosine of the angle each makes with the
/// chord, both to 1e-9.
fn circular(a: Vec2, apex: Vec2, b: Vec2, w: f64) -> Option<Seg2> {
    let (leg, chord) = (apex - a, b - a);
    let (length, across) = (leg.len(), chord.len());
    if !(length > 0.0 && across > 0.0 && w > 0.0 && w < 1.0) || ((b - apex).len() - length).abs() > 1e-9 * length {
        return None;
    }
    if (leg.dot(chord) / (length * across) - w).abs() > 1e-9 {
        return None;
    }
    // Half the turn is the angle between a leg and the chord, and the centre lies along the normal
    // to the leg at `a`, on the chord's side.
    let half = w.acos();
    let radius = length / half.tan();
    let side = if leg.perp().dot(chord) >= 0.0 { 1.0 } else { -1.0 };
    let center = a + leg.perp() * (side * radius / length);
    let arc = Seg2::Arc { center, radius, start: (a - center).angle(), sweep: side * 2.0 * half };
    (arc.end().dist(b) <= 1e-9 * (radius + across)).then_some(arc)
}

/// Deepest halving of a conic into chords: 4096 chords of one piece of at most a quarter turn.
const MAX_HALVINGS: u32 = 12;

/// Chords of the conic `seg` added to `path`, each within `chord` of it: a conic with positive weights
/// lies in its control triangle, so a chord is no further from it than the apex is from the chord.
/// Halves it (exactly) until that holds; false when it would take more than [`MAX_HALVINGS`].
fn chords(seg: Seg2, chord: f64, depth: u32, path: &mut Vec<Seg2>) -> bool {
    let Seg2::Conic { a, apex, b, .. } = seg else {
        path.push(seg);
        return true;
    };
    let along = b - a;
    let t = if along.len2() > 0.0 { ((apex - a).dot(along) / along.len2()).clamp(0.0, 1.0) } else { 0.0 };
    let off = apex.dist(a + along * t);
    if off <= chord || depth >= MAX_HALVINGS {
        path.push(Seg2::Line { a, b });
        return off <= chord;
    }
    let (first, second) = seg.split_half();
    let one = chords(first, chord, depth + 1, path);
    chords(second, chord, depth + 1, path) && one
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2};

    use super::*;

    /// A prepare cancelled before its body is whole keeps nothing, so the next view prepares the body
    /// itself and draws it exact; a body prepared is lent by its index among the state's bodies.
    #[test]
    fn a_cancelled_prepare_keeps_nothing() {
        let mut s = crate::Session::default();
        s.execute("solid.box", &serde_json::json!({"length": 100, "width": 50, "height": 20, "body_name": "Block"})).unwrap();
        let shown = crate::drawing_view::shown(&s).unwrap();
        let placed = mesh_lines::placed(&s, &shown.state, |_| true, "test").unwrap();
        let ask = mesh_lines::Ask { tangent_edges: false, sources: true };
        let edges = |item: &Placed| mesh_lines::drawn_edges(&item.body.mesh(), ask);
        let mut prepared = PreparedBodies::default();
        assert!(prepare(&placed, &shown, &options(), &edges, &|| true, &mut prepared).is_none());
        assert!(prepared.defs.is_empty() && prepared.bodies.is_empty());
        let groups = prepare(&placed, &shown, &options(), &edges, &|| false, &mut prepared).unwrap();
        assert!(matches!(groups.as_slice(), [Ok(Group { def: 0, .. })]));
        assert_eq!((prepared.defs.len(), prepared.bodies.get(&shown.bodies[0].index)), (1, Some(&Ok(0))));
    }

    /// A quarter of a circle of 5 round (1, 2), from 0 to 90 degrees, as a conic.
    fn quarter(clockwise: bool) -> (Vec2, Vec2, Vec2) {
        let c = Vec2::new(1.0, 2.0);
        let (a, b) = (c + Vec2::new(5.0, 0.0), c + Vec2::new(0.0, 5.0));
        if clockwise { (b, c + Vec2::new(5.0, 5.0), a) } else { (a, c + Vec2::new(5.0, 5.0), b) }
    }

    /// An uncertain piece is drawn as a visible stroke and counted; a covered piece is left out and
    /// counted; a hidden piece is drawn only when hidden lines are asked for. Tested here on the
    /// pieces' values, because no body drawn exact can leave `hlr` uncertain through commands: a
    /// freeform face, the one cause, sends its body to the mesh first.
    #[test]
    fn an_uncertain_piece_is_drawn_visible_and_counted() {
        use Visibility::{Hidden, Uncertain, Visible};
        let (mut covered, mut uncertain) = (0, 0);
        let mut drawn = |visibility, is_covered, hidden| shown_as(visibility, is_covered, hidden, &mut covered, &mut uncertain);
        assert_eq!(drawn(Uncertain, false, false), Some(Visible));
        assert_eq!(drawn(Uncertain, false, true), Some(Visible));
        assert_eq!(drawn(Hidden, false, true), Some(Hidden));
        assert_eq!(drawn(Hidden, false, false), None);
        assert_eq!(drawn(Visible, false, false), Some(Visible));
        assert_eq!(drawn(Visible, true, true), None);
        assert_eq!((covered, uncertain), (1, 2));
    }

    /// A quarter circle given as a conic is that arc, either way round; a quarter ellipse is not one,
    /// nor is a conic of the wrong weight.
    #[test]
    fn a_circular_conic_is_its_arc() {
        let (a, apex, b) = quarter(false);
        let Some(Seg2::Arc { center, radius, start, sweep }) = circular(a, apex, b, FRAC_1_SQRT_2) else { panic!() };
        assert!(center.dist(Vec2::new(1.0, 2.0)) < 1e-12 && (radius - 5.0).abs() < 1e-12, "{center:?} {radius}");
        assert!(start.abs() < 1e-12 && (sweep - FRAC_PI_2).abs() < 1e-12, "{start} {sweep}");
        let (a, apex, b) = quarter(true);
        let Some(Seg2::Arc { start, sweep, .. }) = circular(a, apex, b, FRAC_1_SQRT_2) else { panic!() };
        assert!((start - FRAC_PI_2).abs() < 1e-12 && (sweep + FRAC_PI_2).abs() < 1e-12, "{start} {sweep}");
        let squashed = |p: Vec2| Vec2::new(p.x, 2.0 + (p.y - 2.0) * 0.5);
        let (a, apex, b) = quarter(false);
        assert_eq!(circular(squashed(a), squashed(apex), squashed(b), FRAC_1_SQRT_2), None);
        assert_eq!(circular(a, apex, b, 0.8), None);
    }

    /// A quarter ellipse is cut into chords each within the bound, end to end from its start to its
    /// end; a bound it can't meet in 4096 chords is said.
    #[test]
    fn an_ellipse_is_cut_into_chords_within_the_bound() {
        let ellipse = Seg2::Conic { a: Vec2::new(30.0, 0.0), apex: Vec2::new(30.0, 10.0), b: Vec2::new(0.0, 10.0), w: FRAC_1_SQRT_2 };
        let mut path = Vec::new();
        assert!(chords(ellipse, 0.01, 0, &mut path));
        assert!(path.len() > 4 && path.iter().all(|s| matches!(s, Seg2::Line { .. })), "{path:?}");
        assert_eq!((path.first().map(Seg2::start), path.last().map(Seg2::end)), (Some(ellipse.start()), Some(ellipse.end())));
        for w in path.windows(2) {
            assert!(w[0].end().dist(w[1].start()) < 1e-12, "{w:?}");
        }
        // Each chord against the curve between its ends, found by the curve's own points.
        for i in 0..=400 {
            let p = ellipse.point_at(i as f64 / 400.0);
            let near = path.iter().map(|s| s.point_at(s.closest_param(p)).dist(p)).fold(f64::INFINITY, f64::min);
            assert!(near <= 0.01 + 1e-12, "{p:?} is {near} off");
        }
        assert!(!chords(ellipse, 1e-12, 0, &mut Vec::new()));
    }

    /// The quarters of one circle join into one arc, up to a whole turn; an arc of another circle,
    /// or one that doesn't go on from the last, starts again.
    #[test]
    fn arcs_of_one_circle_join() {
        let c = Vec2::new(0.0, 0.0);
        let q = |k: f64| Seg2::Arc { center: c, radius: 5.0, start: k * FRAC_PI_2, sweep: FRAC_PI_2 };
        let mut path = Vec::new();
        for k in 0..4 {
            joined(&mut path, q(k as f64));
        }
        assert_eq!(path.len(), 1);
        let Some(Seg2::Arc { sweep, .. }) = path.first() else { panic!() };
        assert!((sweep - std::f64::consts::TAU).abs() < 1e-12);
        joined(&mut path, q(0.0));
        assert_eq!(path.len(), 2, "past a whole turn");
        let mut path = vec![q(0.0)];
        joined(&mut path, Seg2::Arc { center: c, radius: 6.0, start: FRAC_PI_2, sweep: FRAC_PI_2 });
        joined(&mut path, q(3.0));
        assert_eq!(path.len(), 3);
    }
}
