//! A drawing view's lines (W07 V3, #5866): every shown body's edges and silhouettes from the mesh
//! projection ([`crate::mesh_lines`]), each named by what drew it, in pieces of one visibility on
//! paper. They are approximate: sampled from meshes asked for chords of `chord_mm` on paper. The
//! exact lines (W06) replace them body by body after the kernel swap (V8).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use solidcraft_geom::drawing_view::ViewFrame;
use solidcraft_geom::hlr::{Source, Visibility};
use solidcraft_geom::paper::MAX_RECORDS;
use solidcraft_geom::section::CurveAccuracy;
use solidcraft_geom::{Seg2, Vec2};

use super::ShownSet;
use crate::mesh_lines::{self, Ask, Origin, Owner, View};
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

/// The lines of the bodies `shown` holds in the view `frame` places on paper: the hidden ones too
/// when `hidden`, the tangent edges too when `tangent_edges`, sampled at `chord_mm` on paper.
pub fn lines(s: &Session, shown: &ShownSet, frame: &ViewFrame, hidden: bool, tangent_edges: bool, chord_mm: f64, cmd: &str) -> Result<Lines> {
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
    let scene = mesh_lines::prepare(s, cmd, model_chord_mm, Ask { tangent_edges, sources: true }, |b| names.contains(b.name.as_str()))?;
    let prepare_ms = started.elapsed().as_secs_f64() * 1000.;
    if scene.placements.len() != shown.bodies.len() {
        return Err(bad(cmd, "two shown bodies have one name: the view can't tell their lines apart"));
    }
    let drawing = mesh_lines::drawing(&scene, &[], &View::orthographic(&frame.basis())).map_err(|e| bad(cmd, e))?;
    // The scene places the shown bodies in the shown set's order, one placement each.
    let mut by_owner: BTreeMap<Owner, Vec<(Vec2, Vec2, bool)>> = BTreeMap::new();
    let mut covered = 0;
    for line in drawing.lines.iter().filter(|l| hidden || !l.hidden) {
        let Some(owner) = line.owner else { continue };
        covered += usize::from(line.covered);
        by_owner.entry(owner).or_default().push((line.a, line.b, line.hidden));
    }
    // The merge drops spans up to its tolerance long, so where two stretches meet each can stop
    // that short of the corner.
    let join = 2.0 * drawing.eps;
    let accuracy = frame.accuracy_on_paper(CurveAccuracy::Sampled { max_sample_deviation_mm: 0.0 });
    let mut out = Vec::with_capacity(by_owner.len());
    let mut pieces_count = 0usize;
    for (owner, stretches) in by_owner {
        let placement = scene.placements.get(owner.placement as usize).ok_or_else(|| bad(cmd, "a line names no placement"))?;
        let source = match owner.origin {
            Origin::Edge(edge) => Source::Edge { edge: edge as usize, tangent: placement.tangent.get(edge as usize).copied().unwrap_or(false) },
            Origin::Silhouette(face) => Source::Silhouette { face: face as usize },
        };
        let pieces: Vec<Piece> = chain(&stretches, join)
            .into_iter()
            .map(|(is_hidden, points)| Piece {
                visibility: if is_hidden { Visibility::Hidden } else { Visibility::Visible },
                path: points.windows(2).filter_map(|w| Some(frame.seg_to_paper(&Seg2::Line { a: *w.first()?, b: *w.get(1)? }))).collect(),
                accuracy,
            })
            .collect();
        pieces_count = pieces_count.saturating_add(pieces.len());
        if pieces_count > MAX_RECORDS {
            return Err(bad(cmd, format!("the view has more than {MAX_RECORDS} line pieces: raise chord_mm, or choose a smaller view")));
        }
        out.push(ViewLine { shown: owner.placement as usize, source, pieces });
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
