//! A drawing view's exact lines (W07 V8, #5866): the shown bodies that `hlr` can draw from their
//! records, with their visibility worked out on the exact curves and surfaces. The edges drawn,
//! and whether each is a tangent edge, come from the caller ([`Edges`]) by the view's one rule
//! ([`mesh_lines::drawn_edges`]), so a view draws the same edges either way; `hlr` decides only
//! where each is seen.
//!
//! A body is drawn exact unless one of [`Fallback`]'s reasons holds; the caller draws the rest from
//! the mesh projection, which tests them against every shown body, as before. An edge of a body
//! drawn exact that `hlr` doesn't draw (a split between two faces of one surface) comes from the
//! mesh projection alone ([`Exact::split`]). Reference parts follow
//! the mesh projection's rule: their lines are tested against everything, and the model's see
//! through them unless the view says they hide it. So their lines come from a second `hlr` call
//! with them in the scene.
//!
//! A sheet's views lend each other the bodies they prepare for `hlr` ([`PreparedBodies`], #13218):
//! preparing doesn't depend on the view, so each body is prepared once for the sheet, and both of a
//! view's `hlr` calls use it.

use std::cell::OnceCell;
use std::collections::BTreeMap;
use std::sync::Arc;

use solidcore_record::brep::Brep;
use solidcore_record::{Seg2, Vec2, Vec3};
use solidcraft_doc::ModelState;
use solidcraft_geom::drawing_view::ViewFrame;
use solidcraft_geom::prelude::*;
use solidcraft_geom::section::CurveAccuracy;
use solidcraft_geom::{Aabb2, EPS, Similarity2};
use solidcraft_kernel::hlr::{self, HlrError, Source, Visibility};

use super::ShownSet;
use super::lines::{LineRequest, Piece, ViewLine, continues, joined_sweep, on_arc, within};
use crate::mesh_lines::{self, Placed, Stage};

/// The edges the view draws of a body, in its record's order, and which of them are tangent edges.
pub(super) type Edges<'a> = dyn Fn(&Placed) -> (Vec<bool>, Vec<bool>) + 'a;

/// Why a shown body's lines come from the mesh instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Fallback {
    /// A mesh body: it has no record to draw from.
    MeshBody,
    /// A face `hlr` doesn't draw exactly: a freeform face, or a trim it can't classify.
    Unsupported,
    /// The body's record, or its edge numbers, couldn't be read for `hlr`.
    Unread,
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
            Fallback::Unsupported => "faces the exact lines can't draw",
            Fallback::Unread => "records that can't be read",
            Fallback::InFront => "a mesh body in front",
            Fallback::Refused => "more work than the exact lines allow",
        }
    }
}

/// The exact half of a view's lines.
pub(super) struct Exact {
    /// Per shown body, in the shown set's order: its lines, or why they come from the mesh.
    pub drawn: Vec<std::result::Result<Vec<ViewLine>, Fallback>>,
    /// The edges of bodies drawn exact that the view draws and `hlr` doesn't, by shown body and edge:
    /// splits between two faces of one surface (#2455). The caller draws them from the mesh.
    pub split: Vec<(usize, usize)>,
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
    let groups = prepare(
        placed,
        shown,
        opts,
        edges,
        &cancel,
        &|| {
            let _ = stage.advance(1);
        },
        prepared,
    )?;
    let (defs, prepared) = (&prepared.defs, prepared.defs.len() - made);
    let prepare_ms = started.elapsed().as_secs_f64() * 1000.;
    let reference: Vec<bool> = (0..placed.len()).map(|i| shown.bodies.get(i).is_some_and(|b| b.reference)).collect();
    let mut drawn: Vec<std::result::Result<Vec<ViewLine>, Fallback>> = placed
        .iter()
        .map(|item| groups.get(item.group).ok_or(Fallback::Unread).and_then(|g| g.as_ref().map(|_| Vec::new()).map_err(|f| *f)))
        .collect();
    let basis = frame.basis();
    // The bodies' boxes on the view, read only when a body drawn from the mesh may hide one drawn
    // exact, or a reference part is drawn exact.
    let boxes = OnceCell::new();
    let boxes = || boxes.get_or_init(|| placed.iter().map(|item| view_box(item, &basis)).collect::<Vec<ViewBox>>()).as_slice();
    if drawn.iter().any(Result::is_err) {
        in_front(&reference, request.reference_hides_model, boxes(), &mut drawn);
    }
    let mut exact = Exact { drawn, split: Vec::new(), covered: 0, uncertain: 0, prepared, prepare_ms, projection_ms: 0.0, occlusion_ms: 0.0 };
    // `hlr` measures x and y from the world origin, the view's plane from the basis's origin.
    let Some(shift) = Similarity2::new(1.0, 0.0, Vec2::new(-basis.origin.dot(basis.x), -basis.origin.dot(basis.y))) else {
        refuse(&mut exact, |_| true);
        return Some(exact);
    };
    let calls = Calls {
        defs,
        groups: &groups,
        placed,
        reference: &reference,
        frame,
        request,
        opts,
        view: hlr::View { x: basis.x, y: basis.y, d: basis.y.cross(basis.x) },
        shift,
        clip: reference.contains(&false).then_some(request.shows),
        model_chord: request.chord_mm / frame.scale().factor(),
    };
    // The model's lines, tested against the model, and against the reference parts when they hide it.
    let model: Vec<usize> = (0..placed.len()).filter(|i| !reference[*i] || request.reference_hides_model).collect();
    calls.draw(&model, false, &cancel, &mut exact)?;
    // The reference parts' own lines, tested against everything: every reference part, first, so one
    // on a model line keeps all of it, then each model body that may hide one drawn exact.
    let drawn_exact: Vec<usize> = (0..placed.len()).filter(|i| reference[*i] && exact.drawn.get(*i).is_some_and(Result::is_ok)).collect();
    if !drawn_exact.is_empty() {
        let hides_one = |i: usize| drawn_exact.iter().any(|r| may_hide(boxes().get(i).copied().flatten(), boxes().get(*r).copied().flatten()));
        let (parts, others): (Vec<usize>, Vec<usize>) = (0..placed.len()).partition(|i| reference[*i]);
        let scene: Vec<usize> = parts.into_iter().chain(others.into_iter().filter(|i| hides_one(*i))).collect();
        calls.draw(&scene, true, &cancel, &mut exact)?;
    }
    stage.advance(placed.len()).ok()?;
    Some(exact)
}

/// What a view's `hlr` calls share.
struct Calls<'a> {
    defs: &'a [hlr::Prepared],
    groups: &'a [std::result::Result<Group, Fallback>],
    placed: &'a [Placed<'a>],
    /// Per shown body, whether it is a reference part.
    reference: &'a [bool],
    frame: &'a ViewFrame,
    request: &'a LineRequest,
    opts: &'a hlr::Options,
    view: hlr::View,
    /// From `hlr`'s screen millimetres to the view's plane.
    shift: Similarity2,
    /// What reference lines are cut to: what the view's boundary holds, when it shows model parts too
    /// (only then can they reach past it).
    clip: Option<Aabb2>,
    /// The view's chord in model millimetres.
    model_chord: f64,
}

impl Calls<'_> {
    /// The prepared group of shown body `i`, when `hlr` can draw it.
    fn group(&self, i: usize) -> Option<&Group> {
        self.placed.get(i).and_then(|item| self.groups.get(item.group)).and_then(|g| g.as_ref().ok())
    }

    /// One `hidden_lines` call, its scene each of `members` that `hlr` can draw, in that order: the
    /// lines of the members still drawn exact that are reference parts, or not, as `of_reference`
    /// says. `None` when cancelled.
    fn draw(&self, members: &[usize], of_reference: bool, cancel: &(dyn Fn() -> bool + Sync), exact: &mut Exact) -> Option<()> {
        let keep: Vec<bool> =
            (0..self.placed.len()).map(|i| self.reference.get(i) == Some(&of_reference) && exact.drawn.get(i).is_some_and(Result::is_ok)).collect();
        let mut scene = Vec::new();
        let mut shown_at = Vec::new();
        for &i in members {
            let (Some(item), Some(g)) = (self.placed.get(i), self.group(i)) else { continue };
            let column = |c: usize| item.transform.get(c).map_or(Vec3::ZERO, |c| Vec3::new(c[0], c[1], c[2]));
            scene.push(hlr::Placement { body: g.def, rotation: [column(0), column(1), column(2)], translation: column(3) });
            shown_at.push(i);
        }
        if !shown_at.iter().any(|i| keep.get(*i) == Some(&true)) {
            return Some(());
        }
        let found = match hlr::hidden_lines(self.defs, &scene, &self.view, self.opts, cancel) {
            Ok(found) => found,
            Err(HlrError::Cancelled) => return None,
            Err(_) => {
                refuse(exact, |i| keep.get(i) == Some(&true));
                return Some(());
            }
        };
        let [traced, crossed, classified, covered] = found.stats.phase_ms;
        exact.projection_ms += traced + crossed;
        exact.occlusion_ms += classified + covered;
        // Every edge the view draws has a line in `hlr`, hidden or not, unless `hlr` calls it a seam:
        // those are left to the mesh.
        let mut has_line: Vec<Vec<bool>> = shown_at.iter().map(|i| self.group(*i).map_or(Vec::new(), |g| vec![false; g.drawn.len()])).collect();
        for line in &found.lines {
            if let (Source::Edge { edge, .. }, Some(seen)) = (line.source, has_line.get_mut(line.placement))
                && let Some(slot) = seen.get_mut(edge)
            {
                *slot = true;
            }
        }
        for (at, i) in shown_at.iter().enumerate().filter(|(_, i)| keep.get(**i) == Some(&true)) {
            let (Some(g), Some(seen)) = (self.group(*i), has_line.get(at)) else { continue };
            let unseen = g.drawn.iter().zip(seen).enumerate().filter(|(_, (drawn, seen))| **drawn && !**seen);
            exact.split.extend(unseen.map(|(edge, _)| (*i, edge)));
        }
        let hidden = if of_reference { self.request.reference_hidden } else { self.request.hidden };
        let clip = self.clip.filter(|_| of_reference);
        for line in found.lines {
            let Some(&i) = shown_at.get(line.placement).filter(|i| keep.get(**i) == Some(&true)) else { continue };
            let Some(g) = self.group(i) else { continue };
            let source = match line.source {
                Source::Edge { edge, .. } if g.drawn.get(edge) == Some(&true) => Source::Edge { edge, tangent: g.tangent.get(edge) == Some(&true) },
                Source::Edge { .. } => continue,
                silhouette @ Source::Silhouette { .. } => silhouette,
            };
            let Some(Ok(lines)) = exact.drawn.get_mut(i) else { continue };
            let mut pieces = Vec::with_capacity(line.pieces.len());
            for piece in line.pieces {
                let Some(visibility) = shown_as(piece.visibility, piece.covered, hidden, &mut exact.covered, &mut exact.uncertain) else {
                    continue;
                };
                // An edge seen end on is a point on the lines that meet at its ends, so `hlr` gives it
                // as covered; a piece with no length left is dropped with the paper scene's strokes.
                let geometry: Vec<Seg2> = piece.geometry.iter().map(|seg| seg.similar(&self.shift)).collect();
                let runs = match clip {
                    Some(area) => clipped(&geometry, &area, self.model_chord),
                    None => vec![(geometry, false)],
                };
                for (run, chorded) in runs {
                    let (path, accuracy) = on_paper(&run, piece.accuracy, self.frame, self.request.chord_mm, chorded);
                    pieces.push(Piece { visibility, path, accuracy });
                }
            }
            if !pieces.is_empty() {
                lines.push(ViewLine { shown: i, reference: of_reference, source, pieces });
            }
        }
        Some(())
    }
}

/// The parts of `path` (on the view's plane) inside `area`, each run end to end, and whether it was
/// cut on chords within `chord`: a piece inside is kept as it is and a line is cut where it
/// crosses. Another curve is cut on its chords, and an arc's part (a circular conic's too) then
/// drawn on its circle ([`on_arc`]), as the mesh projection draws one; an exact cut of a curve at a
/// box isn't the app's maths to do.
fn clipped(path: &[Seg2], area: &Aabb2, chord: f64) -> Vec<(Vec<Seg2>, bool)> {
    let mut runs: Vec<(Vec<Seg2>, bool)> = Vec::new();
    // The last part kept reached the end of its piece, so a part from the next one's start goes on.
    let mut going = false;
    for seg in path {
        if inside(seg, area) {
            match runs.last_mut() {
                Some((run, _)) if going => run.push(*seg),
                _ => runs.push((vec![*seg], false)),
            }
            going = true;
            continue;
        }
        let seg = &match *seg {
            Seg2::Conic { a, apex, b, w } => circular(a, apex, b, w).unwrap_or(*seg),
            _ => *seg,
        };
        let curve = !matches!(seg, Seg2::Line { .. });
        let points = if curve { seg.polyline(chord) } else { vec![seg.start(), seg.end()] };
        // Runs of the chords' parts inside, each with whether it starts at the piece's start.
        let mut kept: Vec<(Vec<Vec2>, bool)> = Vec::new();
        let mut ends = false;
        for (k, w) in points.windows(2).enumerate() {
            let &[a, b] = w else { continue };
            ends = false;
            let Some((p, q)) = within(a, b, area).filter(|(p, q)| p.dist(*q) > EPS) else { continue };
            match kept.last_mut() {
                Some((run, _)) if run.last().is_some_and(|last| last.dist(p) <= EPS) => run.push(q),
                _ => kept.push((vec![p, q], k == 0 && p.dist(a) <= EPS)),
            }
            ends = q.dist(b) <= EPS;
        }
        let near = |radius: f64| super::lines::near(chord, radius);
        for (points, starts) in kept {
            let part = match seg {
                Seg2::Arc { .. } => on_arc(seg, &points, false, near, 0.0).map(|(arc, _)| vec![arc]),
                _ => None,
            };
            let part = part.unwrap_or_else(|| points.windows(2).filter_map(|w| Some(Seg2::Line { a: *w.first()?, b: *w.get(1)? })).collect());
            match runs.last_mut() {
                Some((run, chorded)) if going && starts => {
                    run.extend(part);
                    *chorded |= curve;
                }
                _ => runs.push((part, curve)),
            }
            going = false;
        }
        going = ends;
    }
    runs
}

/// Whether all of `seg` lies in `area`: a line's or an arc's own box does, or a conic's control
/// triangle (the conic lies within it). Any other curve is cut on its chords.
fn inside(seg: &Seg2, area: &Aabb2) -> bool {
    match *seg {
        Seg2::Line { .. } | Seg2::Arc { .. } => {
            let b = seg.bounds();
            area.contains(b.min) && area.contains(b.max)
        }
        Seg2::Conic { a, apex, b, .. } => [a, apex, b].into_iter().all(|p| area.contains(p)),
        _ => false,
    }
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

/// Every body still drawn exact that `which` names sent to the mesh: `hlr` couldn't draw them.
fn refuse(exact: &mut Exact, which: impl Fn(usize) -> bool) {
    for (_, d) in exact.drawn.iter_mut().enumerate().filter(|(i, d)| d.is_ok() && which(*i)) {
        *d = Err(Fallback::Refused);
    }
}

/// Per placement group, its first body as `prepared` holds it, prepared for `hlr` with `opts` when no
/// earlier view has, or why it can't be drawn exact; with the `edges` this view draws of it. Each
/// body is known by its index among the shown set's state's bodies. The bodies still to prepare
/// are prepared side by side, on the meshes' own worker cap, because each takes milliseconds and a
/// frame of a thousand tubes has a thousand of them (#20484); `done` is called as each finishes.
/// `None` when cancelled, keeping only the bodies prepared whole.
fn prepare(
    placed: &[Placed],
    shown: &ShownSet,
    opts: &hlr::Options,
    edges: &Edges,
    cancel: &(dyn Fn() -> bool + Sync),
    done: &(dyn Fn() + Sync),
    prepared: &mut PreparedBodies,
) -> Option<Vec<std::result::Result<Group, Fallback>>> {
    let firsts: Vec<(usize, &Placed)> = placed.iter().enumerate().filter(|(_, item)| item.first).collect();
    let index_of = |i: usize| shown.bodies.get(i).map(|b| b.index);
    // The records of the bodies no earlier view prepared, in order; `None` for a mesh body.
    let wanted: Vec<Option<&Brep>> = firsts
        .iter()
        .filter(|(i, _)| index_of(*i).is_none_or(|index| !prepared.bodies.contains_key(&index)))
        .map(|(_, item)| (!item.body.body.is_mesh()).then(|| item.body.body.record()))
        .collect();
    let mut made = solidcraft_kernel::map_on_threads(&wanted, mesh_lines::WORKERS, |record| {
        // A body not started by the time of a cancel is not prepared at all: the workers take
        // every queued body, and preparing one reads and copies it before the kernel looks.
        if cancel() {
            return None;
        }
        let made = prepare_body(*record, opts, cancel);
        // Only a finished body moves the count; a cancelled one is told by the next check.
        if made.is_some() {
            done();
        }
        made
    })
    .into_iter();
    let mut cancelled = false;
    let mut groups = Vec::new();
    // Groups are numbered in the order of their first bodies.
    for (i, item) in firsts {
        let index = index_of(i);
        let def = match index.and_then(|index| prepared.bodies.get(&index)) {
            Some(known) => *known,
            None => {
                // A worker that panicked lost its body: it is read as one `hlr` can't draw.
                let made = match made.next().flatten() {
                    Some(Some(Made::Def(def))) => {
                        prepared.defs.push(*def);
                        Ok(prepared.defs.len() - 1)
                    }
                    Some(Some(Made::Fallback(why))) => Err(why),
                    Some(None) => {
                        cancelled = true;
                        continue;
                    }
                    None => Err(Fallback::Unread),
                };
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
    if cancelled { None } else { Some(groups) }
}

/// A body prepared for `hlr`, or why it can't be drawn exact whatever the view.
enum Made {
    Def(Box<hlr::Prepared>),
    Fallback(Fallback),
}

/// `record` (`None` for a mesh body) prepared for `hlr` with `opts`. `None` when cancelled.
fn prepare_body(record: Option<&Brep>, opts: &hlr::Options, cancel: &(dyn Fn() -> bool + Sync)) -> Option<Made> {
    let Some(record) = record else { return Some(Made::Fallback(Fallback::MeshBody)) };
    Some(match hlr::prepare_cancellable(record, opts, cancel) {
        Err(HlrError::Cancelled) => return None,
        Err(_) => Made::Fallback(Fallback::Unread),
        Ok(def) if !(def.report.freeform_faces.is_empty() && def.report.unsupported_trims.is_empty()) => Made::Fallback(Fallback::Unsupported),
        Ok(def) => Made::Def(Box::new(def)),
    })
}

/// A body's box on the view: x and y across it, and its depth along the line of sight. `None` when
/// its bounds can't be read.
type ViewBox = Option<([f64; 3], [f64; 3])>;

/// `item`'s box on the view of `basis`, from its exact box (a mesh body's is its triangles').
fn view_box(item: &Placed, basis: &solidcore_record::Plane) -> ViewBox {
    let b = item.body.body.exact_bounds().ok()?;
    let d = basis.y.cross(basis.x);
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for k in 0..8 {
        let c = Vec3::new(
            if k & 1 == 0 { b.min.x } else { b.max.x },
            if k & 2 == 0 { b.min.y } else { b.max.y },
            if k & 4 == 0 { b.min.z } else { b.max.z },
        );
        for (k, v) in [c.dot(basis.x), c.dot(basis.y), c.dot(d)].into_iter().enumerate() {
            lo[k] = lo[k].min(v);
            hi[k] = hi[k].max(v);
        }
    }
    lo.iter().chain(&hi).all(|v| v.is_finite()).then_some((lo, hi))
}

/// Whether the body boxed `other` may hide some of the body boxed `mine`: its box overlaps it on the
/// view and reaches nearer the eye somewhere. A box that can't be read could stand anywhere.
fn may_hide(other: ViewBox, mine: ViewBox) -> bool {
    match (mine, other) {
        (Some((lo, hi)), Some((olo, ohi))) => olo[0] < hi[0] && lo[0] < ohi[0] && olo[1] < hi[1] && lo[1] < ohi[1] && olo[2] < hi[2],
        _ => true,
    }
}

/// Sends to the mesh each body `drawn` still draws exact that a body drawn from the mesh may hide
/// ([`may_hide`], on `boxes`): any body hides a reference part, and one that isn't a reference part
/// (as `reference` says) hides the model, as reference parts do when they `hide_model`.
fn in_front(reference: &[bool], hide_model: bool, boxes: &[ViewBox], drawn: &mut [std::result::Result<Vec<ViewLine>, Fallback>]) {
    let outside: Vec<usize> = (0..drawn.len()).filter(|i| drawn.get(*i).is_some_and(Result::is_err)).collect();
    let is_reference = |i: usize| reference.get(i) == Some(&true);
    let box_of = |i: usize| boxes.get(i).copied().flatten();
    for (i, slot) in drawn.iter_mut().enumerate().filter(|(_, slot)| slot.is_ok()) {
        let hides = |j: &usize| is_reference(i) || hide_model || !is_reference(*j);
        if outside.iter().filter(|j| hides(j)).any(|j| may_hide(box_of(*j), box_of(i))) {
            *slot = Err(Fallback::InFront);
        }
    }
}

/// A piece's `geometry` (on the view's plane) on paper, as lines and arcs, with its accuracy there.
/// A conic that is a circular arc becomes that arc, and arcs of one circle join; another conic (a
/// circle seen at an angle) is cut into chords within `chord` on paper, a proven bound. Anything else
/// is chords of its polyline, sampled, as is geometry already `chorded` by [`clipped`].
fn on_paper(geometry: &[Seg2], accuracy: CurveAccuracy, frame: &ViewFrame, chord: f64, chorded: bool) -> (Vec<Seg2>, CurveAccuracy) {
    let mut path: Vec<Seg2> = Vec::with_capacity(geometry.len());
    let (mut flattened, mut proven) = (chorded, !chorded);
    for seg in geometry {
        match frame.seg_to_paper(seg) {
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
    let accuracy = match (hlr::accuracy_on_paper(frame, accuracy), flattened, proven) {
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
        && let Some(whole) = joined_sweep(*sweep, turn)
    {
        *sweep = whole;
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
        assert!(prepare(&placed, &shown, &options(), &edges, &|| true, &|| {}, &mut prepared).is_none());
        assert!(prepared.defs.is_empty() && prepared.bodies.is_empty());
        let groups = prepare(&placed, &shown, &options(), &edges, &|| false, &|| {}, &mut prepared).unwrap();
        assert!(matches!(groups.as_slice(), [Ok(Group { def: 0, .. })]));
        assert_eq!((prepared.defs.len(), prepared.bodies.get(&shown.bodies[0].index)), (1, Some(&Ok(0))));
    }

    /// Bodies are prepared side by side, each reporting as it finishes: a frame of many bodies
    /// reports every one (not one count for the whole), the groups keep the bodies' order, and a body an
    /// earlier view prepared is neither prepared nor reported again (#20484).
    #[test]
    fn many_bodies_are_prepared_side_by_side_and_each_reported() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        const COUNT: usize = 40;
        let mut s = crate::Session::default();
        for i in 0..COUNT {
            let name = format!("Box {i}");
            s.execute(
                "solid.box",
                &serde_json::json!({"length": 10 + i, "width": 5, "height": 5, "corner": [30 * i, 0, 0], "body_name": name, "operation": "new"}),
            )
            .unwrap();
        }
        let shown = crate::drawing_view::shown(&s).unwrap();
        let placed = mesh_lines::placed(&s, &shown.state, |_| true, "test").unwrap();
        let ask = mesh_lines::Ask { tangent_edges: false, sources: true };
        let edges = |item: &Placed| mesh_lines::drawn_edges(&item.body.mesh(), ask);
        let mut prepared = PreparedBodies::default();
        let reported = AtomicUsize::new(0);
        let groups = prepare(
            &placed,
            &shown,
            &options(),
            &edges,
            &|| false,
            &|| {
                reported.fetch_add(1, Ordering::Relaxed);
            },
            &mut prepared,
        )
        .unwrap();
        assert_eq!((groups.len(), reported.load(Ordering::Relaxed), prepared.defs.len()), (COUNT, COUNT, COUNT));
        // Group i holds the def made for body i: the order is the bodies', not the order the workers finished in.
        let defs: Vec<usize> = groups.iter().filter_map(|g| g.as_ref().ok().map(|g| g.def)).collect();
        assert_eq!(defs, (0..COUNT).collect::<Vec<_>>());
        // Prepared again, nothing is made or reported.
        let again = prepare(
            &placed,
            &shown,
            &options(),
            &edges,
            &|| false,
            &|| {
                reported.fetch_add(1, Ordering::Relaxed);
            },
            &mut prepared,
        )
        .unwrap();
        assert_eq!((again.len(), reported.load(Ordering::Relaxed), prepared.defs.len()), (COUNT, COUNT, COUNT));
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

    /// Quarters whose rounded sweeps add up to a hair either side of a whole turn, as a hole's
    /// conics do (#212), join into one arc of exactly one turn: a circle, which a DXF writes as one.
    #[test]
    fn quarters_that_close_a_circle_join_into_exactly_one_turn() {
        let c = Vec2::new(0.0, 0.0);
        for (quarter, sense) in [(FRAC_PI_2.next_up(), 1.0), (FRAC_PI_2.next_down(), 1.0), (FRAC_PI_2.next_up(), -1.0), (FRAC_PI_2 - 1e-14, -1.0)] {
            let sweep = quarter * sense;
            let mut path = Vec::new();
            let mut start = 0.6131;
            for _ in 0..4 {
                joined(&mut path, Seg2::Arc { center: c, radius: 2.0, start, sweep });
                start += sweep;
            }
            let [Seg2::Arc { sweep: whole, .. }] = path.as_slice() else { panic!("{path:?}") };
            assert_eq!(*whole, std::f64::consts::TAU * sense, "quarters of {sweep}");
        }
    }
}
