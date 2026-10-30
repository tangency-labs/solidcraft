//! A drawing view's lines (W07 V3, #5866): every shown body's edges and silhouettes from the mesh
//! projection ([`crate::mesh_lines`]), each named by what drew it, in pieces of one visibility on
//! paper. They are approximate: sampled from meshes asked for chords of `chord_mm` on paper. An
//! edge that is a circle, or part of one, in a plane parallel to the view is drawn as its exact arc
//! ([`exact_arcs`]), cut where the mesh says its visibility changes. The exact lines (W06) replace
//! the rest body by body after the kernel swap (V8).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use solidcraft_doc::Mat;
use solidcraft_geom::drawing_view::ViewFrame;
use solidcraft_geom::hlr::{Source, Visibility};
use solidcraft_geom::paper::{Group, Item, MAX_RECORDS, ObjectKind};
use solidcraft_geom::section::CurveAccuracy;
use solidcraft_geom::{Aabb2, EPS, Plane, Seg2, Similarity2, Vec2, Vec3};

use super::ShownSet;
use crate::mesh_lines::{self, Ask, Origin, Owner, Scene, View};
use crate::params::bad;
use crate::{Result, Session};

/// The chord a view's lines are sampled at unless asked, in paper millimetres.
pub const CHORD_MM: f64 = 0.01;
/// The chords a view may ask for, in paper millimetres.
pub const CHORD_RANGE: std::ops::RangeInclusive<f64> = 0.001..=1.0;
/// The finest chord the mesh is asked for, in model millimetres: the view DXF's floor. There is no
/// coarsest, so a small-scale view (below 1:100) keeps its chord on paper.
pub const MIN_MODEL_CHORD_MM: f64 = 0.0001;

/// One source's lines in a view: a placed body's edge or silhouette.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewLine {
    /// The index of the body among the shown set's.
    pub shown: usize,
    /// The body is a reference part.
    pub reference: bool,
    pub source: Source,
    pub pieces: Vec<Piece>,
}

/// A run of a line at one visibility, on paper.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub visibility: Visibility,
    /// Paper millimetres, end to end.
    pub path: Vec<Seg2>,
    /// How closely `path` follows the true projected curve, on paper.
    pub accuracy: CurveAccuracy,
}

/// A view's lines, and what made them.
#[derive(Clone, Debug)]
pub struct Lines {
    pub lines: Vec<ViewLine>,
    /// Pieces drawn where another source lay too.
    pub covered: usize,
    /// The chord the mesh was asked for, in model millimetres.
    pub model_chord_mm: f64,
    pub stats: Stats,
}

/// What the mesh projection did, for cost: as the view DXF reports it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub bodies: usize,
    pub shared_meshes: usize,
    pub triangles: usize,
    pub segments: usize,
    pub prepare_ms: f64,
    pub projection_ms: f64,
    pub occlusion_ms: f64,
}

/// What a view asks of its lines.
#[derive(Clone, Copy, Debug)]
pub struct LineRequest {
    /// What the view's boundary holds, on its plane (model mm). Reference lines stop at it when the
    /// view shows model parts too: only then can they reach past it.
    pub shows: Aabb2,
    /// The model's hidden lines too.
    pub hidden: bool,
    /// Reference parts' hidden lines too.
    pub reference_hidden: bool,
    pub reference_hides_model: bool,
    /// The edges where faces meet smoothly too.
    pub tangent_edges: bool,
    /// The chord they are sampled at, on paper.
    pub chord_mm: f64,
}

/// The lines of the bodies `shown` holds in the view `frame` places on paper, as `request` asks.
pub fn lines(s: &Session, shown: &ShownSet, frame: &ViewFrame, request: &LineRequest, cmd: &str) -> Result<Lines> {
    let LineRequest { shows, hidden, reference_hidden, reference_hides_model, tangent_edges, chord_mm } = *request;
    let scale = frame.scale();
    let model_chord_mm = chord_mm / scale.factor();
    if !(model_chord_mm.is_finite() && model_chord_mm >= MIN_MODEL_CHORD_MM) {
        let least = MIN_MODEL_CHORD_MM * scale.factor();
        return Err(bad(
            cmd,
            format!("at {scale} a chord_mm of {chord_mm} asks the mesh for chords finer than {MIN_MODEL_CHORD_MM} mm: give chord_mm {least} or more"),
        ));
    }
    let started = web_time::Instant::now();
    let names: BTreeSet<&str> = shown.bodies.iter().filter_map(|b| shown.body(b)).map(|b| b.name.as_str()).collect();
    let mut scene = mesh_lines::prepare(s, cmd, model_chord_mm, Ask { tangent_edges, sources: true }, |b| names.contains(b.name.as_str()))?;
    let prepare_ms = started.elapsed().as_secs_f64() * 1000.;
    if scene.placements.len() != shown.bodies.len() {
        return Err(bad(cmd, "two shown bodies have one name: the view can't tell their lines apart"));
    }
    // The scene places the shown bodies in the shown set's order, one placement each.
    for (placement, body) in scene.placements.iter_mut().zip(&shown.bodies) {
        placement.reference = body.reference;
    }
    scene.reference_hides_model = reference_hides_model;
    let has_model = shown.bodies.iter().any(|b| !b.reference);
    let is_reference = |owner: Owner| shown.bodies.get(owner.placement as usize).is_some_and(|b| b.reference);
    let drawing = mesh_lines::drawing(&scene, &[], &View::orthographic(&frame.basis())).map_err(|e| bad(cmd, e))?;
    let mut by_owner: BTreeMap<Owner, Vec<(Vec2, Vec2, bool)>> = BTreeMap::new();
    let mut covered = 0;
    for line in &drawing.lines {
        let Some(owner) = line.owner else { continue };
        let reference = is_reference(owner);
        if line.hidden && !(if reference { reference_hidden } else { hidden }) {
            continue;
        }
        let kept = if reference && has_model { within(line.a, line.b, &shows) } else { Some((line.a, line.b)) };
        let Some((a, b)) = kept else { continue };
        covered += usize::from(line.covered);
        by_owner.entry(owner).or_default().push((a, b, line.hidden));
    }
    // The merge drops spans up to its tolerance long, so where two stretches meet each can stop
    // that short of the corner.
    let join = 2.0 * drawing.eps;
    let runs: Vec<(Owner, Vec<(bool, Vec<Vec2>)>)> = by_owner.into_iter().map(|(owner, stretches)| (owner, chain(&stretches, join))).collect();
    let arcs = exact_arcs(shown, &scene, &frame.basis(), &runs);
    let sampled = frame.accuracy_on_paper(CurveAccuracy::Sampled { max_sample_deviation_mm: 0.0 });
    // A cut point lies on a chord, so off the true curve by up to the chord's sag.
    let near = |radius: f64| 2.0 * model_chord_mm + radius * 1e-9;
    let mut out = Vec::with_capacity(runs.len());
    let mut pieces_count = 0usize;
    for (owner, runs) in runs {
        let placement = scene.placements.get(owner.placement as usize).ok_or_else(|| bad(cmd, "a line names no placement"))?;
        let source = match owner.origin {
            Origin::Edge(edge) => Source::Edge { edge: edge as usize, tangent: placement.tangent.get(edge as usize).copied().unwrap_or(false) },
            Origin::Silhouette(face) => Source::Silhouette { face: face as usize },
        };
        let arc = arcs.get(&owner);
        let only = runs.len() == 1;
        let piece = |(is_hidden, points): &(bool, Vec<Vec2>)| {
            let visibility = if *is_hidden { Visibility::Hidden } else { Visibility::Visible };
            match arc.and_then(|a| on_arc(a, points, only, near, join)) {
                Some((part, whole)) => {
                    Piece { visibility, path: vec![frame.seg_to_paper(&part)], accuracy: if whole { CurveAccuracy::Analytic } else { sampled } }
                }
                None => Piece {
                    visibility,
                    path: points.windows(2).filter_map(|w| Some(frame.seg_to_paper(&Seg2::Line { a: *w.first()?, b: *w.get(1)? }))).collect(),
                    accuracy: sampled,
                },
            }
        };
        let pieces: Vec<Piece> = runs.iter().map(piece).collect();
        pieces_count = pieces_count.saturating_add(pieces.len());
        if pieces_count > MAX_RECORDS {
            return Err(bad(cmd, format!("the view has more than {MAX_RECORDS} line pieces: raise chord_mm, or choose a smaller view")));
        }
        out.push(ViewLine { shown: owner.placement as usize, reference: is_reference(owner), source, pieces });
    }
    let stats = Stats {
        bodies: scene.placements.len(),
        shared_meshes: scene.meshes.len(),
        triangles: scene.triangles,
        segments: drawing.segments,
        prepare_ms,
        projection_ms: drawing.projection_ms,
        occlusion_ms: drawing.occlusion_ms,
    };
    Ok(Lines { lines: out, covered, model_chord_mm, stats })
}

/// The part of the line from `a` to `b` inside `area`, if any.
fn within(a: Vec2, b: Vec2, area: &Aabb2) -> Option<(Vec2, Vec2)> {
    let (mut lo, mut hi) = (0.0, 1.0);
    let inside = mesh_lines::clip(&mut lo, &mut hi, a.x - area.min.x, b.x - area.min.x)
        && mesh_lines::clip(&mut lo, &mut hi, area.max.x - a.x, area.max.x - b.x)
        && mesh_lines::clip(&mut lo, &mut hi, a.y - area.min.y, b.y - area.min.y)
        && mesh_lines::clip(&mut lo, &mut hi, area.max.y - a.y, area.max.y - b.y);
    inside.then(|| (a + (b - a) * lo, a + (b - a) * hi))
}

/// A visible line's width on paper until D2 sets widths by kind of line: the thick line of ISO
/// 128's 0.5 mm line group.
pub const VISIBLE_MM: f64 = 0.5;
/// A reference part's line on paper until D2 does: the thin line of the same group.
pub const REFERENCE_MM: f64 = 0.25;

/// `lines` as the paper scene group of the view `object`: each visible piece one stroke, closed when
/// it ends where it starts, reference parts' [`REFERENCE_MM`] wide and under the model's,
/// [`VISIBLE_MM`] wide. Hidden pieces are left out until the paper scene has dashed lines (V7b).
pub fn paper_group(lines: &Lines, object: u64) -> Group {
    let stroke = |piece: &Piece, width_mm: f64| {
        let path: Vec<Seg2> = piece.path.iter().copied().filter_map(drawable).collect();
        let ends_meet = path.first().zip(path.last()).is_some_and(|(first, last)| last.end().dist(first.start()) <= EPS);
        // One straight line never closes on itself, however short.
        let closed = ends_meet && !matches!(path.as_slice(), [Seg2::Line { .. }]);
        (!path.is_empty()).then_some(Item::Stroke { path, closed, width_mm })
    };
    let strokes = |reference: bool, width_mm: f64| {
        let pieces = lines.lines.iter().filter(move |l| l.reference == reference).flat_map(|l| &l.pieces);
        pieces.filter(|p| p.visibility == Visibility::Visible).filter_map(move |p| stroke(p, width_mm))
    };
    Group { object, kind: ObjectKind::View, items: strokes(true, REFERENCE_MM).chain(strokes(false, VISIBLE_MM)).collect() }
}

/// `seg`, or nothing where it has no length to draw: a line whose ends are one point. An arc too
/// short to sweep is drawn as its chord.
fn drawable(seg: Seg2) -> Option<Seg2> {
    match seg {
        Seg2::Arc { sweep, .. } if sweep.abs() < 1e-9 => drawable(Seg2::Line { a: seg.start(), b: seg.end() }),
        Seg2::Line { a, b } if a == b => None,
        _ => Some(seg),
    }
}

/// The edges `runs` draw that are one circular arc (or a whole circle) in a plane parallel to the
/// view, as that arc in the view's plane coordinates, by owner. `planar_edges` is asked only for
/// curved edges, and once for the placements of one shared mesh turned the same way: a part placed
/// again that way only shifts on the view's plane.
fn exact_arcs(shown: &ShownSet, scene: &Scene, basis: &Plane, runs: &[(Owner, Vec<(bool, Vec<Vec2>)>)]) -> HashMap<Owner, Seg2> {
    let mut curved: BTreeMap<u32, BTreeSet<usize>> = BTreeMap::new();
    for (owner, pieces) in runs {
        if let Origin::Edge(edge) = owner.origin
            && pieces.iter().any(|(_, points)| points.len() > 2)
        {
            curved.entry(owner.placement).or_default().insert(edge as usize);
        }
    }
    let class = |placement: u32| scene.placements.get(placement as usize).map(|p| (p.mesh, turn(&p.transform)));
    let mut classes: BTreeMap<(usize, [u64; 9]), (u32, BTreeSet<usize>)> = BTreeMap::new();
    for (&placement, edges) in &curved {
        if let Some(key) = class(placement) {
            classes.entry(key).or_insert((placement, BTreeSet::new())).1.extend(edges);
        }
    }
    // Each class's arcs, read on its first placement's body in place.
    let mut found: BTreeMap<(usize, [u64; 9]), (u32, HashMap<usize, Seg2>)> = BTreeMap::new();
    for (key, (first, edges)) in classes {
        let Some(body) = shown.bodies.get(first as usize).and_then(|b| shown.body(b)) else { continue };
        let edges: Vec<usize> = edges.into_iter().collect();
        let exact = body.body.planar_edges(basis, &edges);
        let arcs = edges
            .iter()
            .zip(exact)
            .filter_map(|(edge, segs)| match segs.as_deref() {
                Some([arc @ Seg2::Arc { .. }]) => Some((*edge, *arc)),
                _ => None,
            })
            .collect();
        found.insert(key, (first, arcs));
    }
    let mut out = HashMap::new();
    for (&placement, edges) in &curved {
        let Some((first, arcs)) = class(placement).and_then(|key| found.get(&key)) else { continue };
        let (Some(here), Some(there)) = (scene.placements.get(placement as usize), scene.placements.get(*first as usize)) else { continue };
        let shift = shift(&here.transform) - shift(&there.transform);
        let Some(by) = Similarity2::new(1.0, 0.0, Vec2::new(shift.dot(basis.x), shift.dot(basis.y))) else { continue };
        for edge in edges {
            if let Some(arc) = arcs.get(edge) {
                out.insert(Owner { placement, origin: Origin::Edge(*edge as u32) }, arc.similar(&by));
            }
        }
    }
    out
}

/// A placement's turn: its matrix's rotation, bit for bit.
fn turn(m: &Mat) -> [u64; 9] {
    [m[0][0], m[0][1], m[0][2], m[1][0], m[1][1], m[1][2], m[2][0], m[2][1], m[2][2]].map(f64::to_bits)
}

/// A placement's shift.
fn shift(m: &Mat) -> Vec3 {
    Vec3::new(m[3][0], m[3][1], m[3][2])
}

/// The part of `arc` (an edge's exact curve on the view's plane) that a piece's `points` run
/// along, and whether it is all of the edge (`only`: the piece is the edge's one run). None when a
/// point lies further than `near(radius)` from the circle. The ends are the points' own, so a piece
/// cut where the visibility changes ends where the mesh cut it; ends within `join` of the edge's own
/// are its ends (the merge stops up to that short of them).
fn on_arc(arc: &Seg2, points: &[Vec2], only: bool, near: impl Fn(f64) -> f64, join: f64) -> Option<(Seg2, bool)> {
    let Seg2::Arc { center, radius, start, sweep } = *arc else { return None };
    let (first, last) = (*points.first()?, *points.last()?);
    if !(radius > 0.0 && sweep != 0.0) || points.iter().any(|p| (p.dist(center) - radius).abs() > near(radius)) {
        return None;
    }
    let full = std::f64::consts::TAU;
    let sense = sweep.signum();
    // How far round from the arc's start, in its own sense: 0 up to a full turn.
    let along = |p: Vec2| (((p.y - center.y).atan2(p.x - center.x) - start) * sense).rem_euclid(full);
    if only && points.len() > 2 && first.dist(last) <= near(radius) {
        // One closed run: the whole circle.
        return ((sweep.abs() - full).abs() <= 1e-9).then_some((*arc, true));
    }
    let (a, b) = (along(first), along(last));
    let forward = (b - a).rem_euclid(full);
    // The run's middle point says which way round it goes; two points take the shorter way.
    let ahead = match points.get(points.len() / 2).filter(|_| points.len() > 2) {
        Some(mid) => (along(*mid) - a).rem_euclid(full) <= forward,
        None => forward <= full - forward,
    };
    let (from, span) = if ahead { (a, forward) } else { (b, full - forward) };
    let whole = only && from.min(full - from) * radius <= join && (span - sweep.abs()).abs() * radius <= 2.0 * join;
    // A run along all of the edge is the edge itself, to the last bit. A part starts within half a
    // turn of 0, wherever round the edge it begins.
    let begins = (start + sense * from + std::f64::consts::PI).rem_euclid(full) - std::f64::consts::PI;
    Some(if whole { (*arc, true) } else { (Seg2::Arc { center, radius, start: begins, sweep: sense * span }, false) })
}

/// One source's stretches joined end to end where they meet, within `join`, at one visibility:
/// each run's visibility (hidden or not) and points. Stretches come in any order and either way round.
fn chain(stretches: &[(Vec2, Vec2, bool)], join: f64) -> Vec<(bool, Vec<Vec2>)> {
    let cell = |p: Vec2| [(p.x / (join * 4.0)).floor() as i64, (p.y / (join * 4.0)).floor() as i64];
    let mut ends: HashMap<[i64; 2], Vec<(usize, bool)>> = HashMap::new();
    for (i, (a, b, _)) in stretches.iter().enumerate() {
        ends.entry(cell(*a)).or_default().push((i, false));
        ends.entry(cell(*b)).or_default().push((i, true));
    }
    let mut used = vec![false; stretches.len()];
    // An unused stretch of visibility `hidden` with an end at `p`, and its other end.
    let next = |p: Vec2, hidden: bool, used: &mut Vec<bool>| -> Option<Vec2> {
        let [cx, cy] = cell(p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &(i, at_b) in ends.get(&[cx + dx, cy + dy]).into_iter().flatten() {
                    let Some((a, b, h)) = stretches.get(i) else { continue };
                    let (here, there) = if at_b { (*b, *a) } else { (*a, *b) };
                    if *h == hidden && used.get(i) == Some(&false) && here.dist(p) <= join {
                        if let Some(u) = used.get_mut(i) {
                            *u = true;
                        }
                        return Some(there);
                    }
                }
            }
        }
        None
    };
    let mut runs = Vec::new();
    for (i, (a, b, hidden)) in stretches.iter().enumerate() {
        if used.get(i) != Some(&false) {
            continue;
        }
        if let Some(u) = used.get_mut(i) {
            *u = true;
        }
        let mut forward = vec![*a, *b];
        while let Some(p) = forward.last().copied().and_then(|end| next(end, *hidden, &mut used)) {
            forward.push(p);
        }
        let mut backward = Vec::new();
        while let Some(p) = backward.last().or(forward.first()).copied().and_then(|start| next(start, *hidden, &mut used)) {
            backward.push(p);
        }
        backward.reverse();
        backward.extend(forward);
        runs.push((*hidden, backward));
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines_of(pieces: Vec<Piece>) -> Lines {
        let line = ViewLine { shown: 0, reference: false, source: Source::Edge { edge: 0, tangent: false }, pieces };
        Lines { lines: vec![line], covered: 0, model_chord_mm: 0.01, stats: Stats::default() }
    }

    fn piece(visibility: Visibility, path: Vec<Seg2>) -> Piece {
        Piece { visibility, path, accuracy: CurveAccuracy::Sampled { max_sample_deviation_mm: 0.0 } }
    }

    /// Each visible piece is one stroke of the view's kind, 0.5 mm wide; a whole circle and a run
    /// that comes back to its start are closed; a line with no length is left out of its stroke,
    /// and a piece with nothing left is no stroke; an arc too short to sweep is its chord; hidden
    /// pieces aren't drawn yet.
    #[test]
    fn visible_pieces_are_strokes_and_closed_ones_join() {
        let (a, b, c) = (Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 5.0));
        let circle = Seg2::Arc { center: a, radius: 5.0, start: 0.0, sweep: std::f64::consts::TAU };
        let lines = lines_of(vec![
            piece(Visibility::Visible, vec![Seg2::Line { a, b }, Seg2::Line { a: b, b }, Seg2::Line { a: b, b: c }]),
            piece(Visibility::Visible, vec![circle]),
            piece(Visibility::Visible, vec![Seg2::Line { a, b }, Seg2::Line { a: b, b: c }, Seg2::Line { a: c, b: a }]),
            piece(Visibility::Visible, vec![Seg2::Line { a: c, b: c }]),
            piece(Visibility::Visible, vec![Seg2::Arc { center: a, radius: 5.0, start: 0.0, sweep: 1e-13 }]),
            piece(Visibility::Hidden, vec![Seg2::Line { a, b: c }]),
        ]);
        let group = paper_group(&lines, 7);
        assert_eq!((group.object, group.kind), (7, ObjectKind::View));
        let strokes: Vec<(usize, bool, f64)> = group
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Stroke { path, closed, width_mm } => Some((path.len(), *closed, *width_mm)),
                Item::Fill { .. } | Item::Text(_) => None,
            })
            .collect();
        assert_eq!(strokes, [(2, false, 0.5), (1, true, 0.5), (3, true, 0.5), (1, false, 0.5)]);
        let chord = group.items.last().and_then(|i| match i {
            Item::Stroke { path, .. } => path.first().copied(),
            Item::Fill { .. } | Item::Text(_) => None,
        });
        assert!(matches!(chord, Some(Seg2::Line { .. })), "{chord:?}");
        let scene = solidcraft_geom::paper::Scene::new(Vec2::new(420.0, 297.0), vec![group]);
        assert!(scene.validate().is_ok());
    }

    /// Reference lines come first, whatever order the lines are in, so the model's paint over them,
    /// and each role has its width.
    #[test]
    fn reference_strokes_are_thin_and_under_the_models() {
        let (a, b) = (Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0));
        let line = |reference: bool| ViewLine {
            shown: 0,
            reference,
            source: Source::Edge { edge: 0, tangent: false },
            pieces: vec![piece(Visibility::Visible, vec![Seg2::Line { a, b }])],
        };
        let lines = Lines { lines: vec![line(false), line(true), line(false)], covered: 0, model_chord_mm: 0.01, stats: Stats::default() };
        let widths: Vec<f64> = paper_group(&lines, 7)
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Stroke { width_mm, .. } => Some(*width_mm),
                _ => None,
            })
            .collect();
        assert_eq!(widths, [REFERENCE_MM, VISIBLE_MM, VISIBLE_MM]);
    }

    /// A view's group past the paper scene's 100,000 records is caught by the scene's own check.
    #[test]
    fn a_group_past_the_scenes_records_is_refused_by_its_check() {
        let one = |i: usize| piece(Visibility::Visible, vec![Seg2::Line { a: Vec2::new(i as f64 * 1e-3, 0.0), b: Vec2::new(i as f64 * 1e-3, 1.0) }]);
        let lines = lines_of((0..MAX_RECORDS).map(one).collect());
        let scene = solidcraft_geom::paper::Scene::new(Vec2::new(420.0, 297.0), vec![paper_group(&lines, 0)]);
        assert!(scene.validate().is_err());
    }

    /// A quarter of a 5 mm circle round the origin, from 0 to 90 degrees.
    const QUARTER: Seg2 = Seg2::Arc { center: Vec2 { x: 0.0, y: 0.0 }, radius: 5.0, start: 0.0, sweep: std::f64::consts::FRAC_PI_2 };

    fn at(degrees: f64) -> Vec2 {
        let a = degrees.to_radians();
        Vec2::new(5.0 * a.cos(), 5.0 * a.sin())
    }

    fn near(_: f64) -> f64 {
        1e-6
    }

    /// A run along all of an arc is the arc itself, exact, also when its ends stop within the join of
    /// the arc's; a run cut short ends where its points do and isn't all of the edge, whichever way
    /// round its points come; a run off the circle has no arc.
    #[test]
    fn a_run_on_an_arc_is_drawn_as_its_part_of_the_arc() {
        let (part, whole) = on_arc(&QUARTER, &[at(0.0), at(45.0), at(90.0)], true, near, 1e-9).unwrap();
        assert!(whole && part == QUARTER, "{part:?}");
        for points in [[at(10.0), at(30.0), at(60.0)], [at(60.0), at(30.0), at(10.0)]] {
            let (part, whole) = on_arc(&QUARTER, &points, false, near, 1e-9).unwrap();
            let Seg2::Arc { start, sweep, .. } = part else { panic!("{part:?}") };
            assert!(!whole && (start - 10f64.to_radians()).abs() < 1e-12 && (sweep - 50f64.to_radians()).abs() < 1e-12, "{part:?}");
        }
        let (part, _) = on_arc(&QUARTER, &[at(80.0), at(70.0)], false, near, 1e-9).unwrap();
        let Seg2::Arc { start, sweep, .. } = part else { panic!("{part:?}") };
        assert!((start - 70f64.to_radians()).abs() < 1e-12 && (sweep - 10f64.to_radians()).abs() < 1e-12, "{part:?}");
        assert!(on_arc(&QUARTER, &[at(0.0), Vec2::new(4.0, 4.0), at(90.0)], true, near, 1e-9).is_none());
        // Ends that stop within the join of the arc's own, as the merge leaves them, are its ends.
        let short = |mm: f64| [at((mm / 5.0).to_degrees()), at(45.0), at(90.0 - (mm / 5.0).to_degrees())];
        assert_eq!(on_arc(&QUARTER, &short(1e-7), true, near, 1e-6), Some((QUARTER, true)));
        assert!(on_arc(&QUARTER, &short(1e-5), true, near, 1e-6).is_some_and(|(_, whole)| !whole));
    }

    /// A whole circle drawn as one closed run is the circle; a clockwise circle cut into a run that
    /// crosses its start comes out clockwise across the start.
    #[test]
    fn a_closed_run_is_the_whole_circle_and_cuts_cross_the_start() {
        let circle = Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: 5.0, start: 0.0, sweep: -std::f64::consts::TAU };
        let ring: Vec<Vec2> = (0..=8).map(|i| at(-45.0 * i as f64)).collect();
        assert_eq!(on_arc(&circle, &ring, true, near, 1e-9), Some((circle, true)));
        // Closed, but beside the edge's other runs, or on a circle that is only part of its edge:
        // not the whole edge.
        assert!(on_arc(&circle, &ring, false, near, 1e-9).is_some_and(|(_, whole)| !whole));
        assert!(on_arc(&QUARTER, &ring, true, near, 1e-9).is_none());
        let (part, whole) = on_arc(&circle, &[at(20.0), at(0.0), at(-20.0)], false, near, 1e-9).unwrap();
        let Seg2::Arc { start, sweep, .. } = part else { panic!("{part:?}") };
        assert!(!whole && (start - 20f64.to_radians()).abs() < 1e-12 && (sweep + 40f64.to_radians()).abs() < 1e-12, "{part:?}");
    }

    /// A line keeps what lies in the box: all of it, its middle, one end, or nothing; one along an
    /// edge of the box is inside.
    #[test]
    fn a_line_is_cut_at_a_box() {
        let area = Aabb2 { min: Vec2::new(0.0, 0.0), max: Vec2::new(10.0, 10.0) };
        let p = Vec2::new;
        assert_eq!(within(p(1.0, 1.0), p(9.0, 9.0), &area), Some((p(1.0, 1.0), p(9.0, 9.0))));
        assert_eq!(within(p(-5.0, 5.0), p(15.0, 5.0), &area), Some((p(0.0, 5.0), p(10.0, 5.0))));
        assert_eq!(within(p(5.0, 5.0), p(5.0, 20.0), &area), Some((p(5.0, 5.0), p(5.0, 10.0))));
        assert_eq!(within(p(11.0, 0.0), p(20.0, 5.0), &area), None);
        assert_eq!(within(p(-1.0, 12.0), p(12.0, 12.0), &area), None);
        // Past a corner, though each end is inside the box along one axis.
        assert_eq!(within(p(8.0, 13.0), p(13.0, 8.0), &area), None);
        assert_eq!(within(p(10.0, 2.0), p(10.0, 8.0), &area), Some((p(10.0, 2.0), p(10.0, 8.0))));
    }

    /// Stretches of one source in any order and either way round join into one run per visibility
    /// that meets end to end, across a gap within the join; a wider gap starts another run.
    #[test]
    fn stretches_join_end_to_end_at_one_visibility() {
        let p = |x: f64, y: f64| Vec2::new(x, y);
        let stretches = [
            (p(2.0, 0.0), p(1.0, 0.0), false),
            (p(0.0, 0.0), p(1.0, 0.0), false),
            (p(2.0, 0.0), p(3.0, 1.0), false),
            (p(3.0, 1.0), p(4.0, 1.0), true),
            (p(4.0 + 5e-7, 1.0), p(5.0, 1.0), true),
            (p(9.0, 9.0), p(10.0, 9.0), false),
        ];
        let mut runs = chain(&stretches, 1e-6);
        runs.sort_by(|a, b| a.1.len().cmp(&b.1.len()).then(a.0.cmp(&b.0)));
        let lengths: Vec<(bool, usize)> = runs.iter().map(|(h, pts)| (*h, pts.len())).collect();
        assert_eq!(lengths, vec![(false, 2), (true, 3), (false, 4)]);
        let long = &runs[2].1;
        let ends = [long[0], long[3]];
        assert!(ends.contains(&p(0.0, 0.0)) && ends.contains(&p(3.0, 1.0)), "{long:?}");
    }
}
