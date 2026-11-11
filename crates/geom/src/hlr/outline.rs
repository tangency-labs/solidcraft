//! Projected curves of one placed body with no hidden-line work: every edge and every face
//! silhouette, projected from its exact curve, whether it would be seen or not. This is what an
//! outline seen along the view (a shadow) is walked from ([`crate::footprint`]); it leaves out
//! the crossings, rays and coverage of [`super::hidden_lines`], which a shadow does not need.
//!
//! A freeform face seen edge-on everywhere (an extruded spline side seen along its extrusion)
//! is skipped: its projection is a curve its own boundary edges already draw, and contouring a
//! surface whose normal is perpendicular to the view all over finds no single silhouette.

use super::curve::C3;
use super::silhouette;
use super::{
    EdgeKind, HlrError, Piece, Placement, Prepared, Source, Track, TrackCurve, View, Visibility, piece_geometry, point_segment, same_analytic,
};
use crate::prelude::*;
use crate::section::CurveAccuracy;
use solidcore_record::Seg2;

/// Most chords one curve is sampled into.
const MAX_SAMPLES: usize = 1 << 14;

/// The projected edges and silhouettes of `def` placed by `placement`, seen through `view`:
/// lines and conic arcs where the projection is exact, chords within `relative` of the body's
/// size otherwise, with the largest distance (mm) of any chord from its exact curve.
/// Faces whose trim is not supported are an error: their silhouettes cannot be traced.
pub fn projected_curves(
    def: &Prepared,
    placement: &Placement,
    view: &View,
    relative: f64,
    cancel: &dyn Fn() -> bool,
) -> Result<(Vec<Seg2>, f64), HlrError> {
    if !placement.rigid() {
        return Err(HlrError::Invalid("placement (not a rotation and translation)"));
    }
    if !(relative.is_finite() && relative > 0.) {
        return Err(HlrError::Invalid("outline tolerance"));
    }
    let tolerance = relative * (def.hi - def.lo).len().max(1e-9);
    // The body's records sit moved by -offset (see `Prepared`): the placement adds it back.
    let p = Placement { translation: placement.apply(def.offset), ..*placement };
    let mut tracks: Vec<Track> = Vec::new();
    for (ei, e) in def.edges.iter().enumerate() {
        if e.kind == EdgeKind::Seam {
            continue;
        }
        tracks.push(track(TrackCurve::Edge(&e.curve), e.curve.domain(), Source::Edge { edge: ei, tangent: e.kind == EdgeKind::Tangent }, e.acc));
    }
    let dl = p.local_dir(view.d);
    // Faces on one surface share their silhouettes, keyed by the set's first face (as in
    // `hidden_lines`).
    let mut sets: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
    for (fi, f) in def.faces.iter().enumerate() {
        if f.trim.unsupported.is_some() {
            return Err(HlrError::Invalid("a face whose trim is not supported"));
        }
        if matches!(f.analytic, super::Analytic::Plane) || (f.freeform && f.surface.swept_along(dl)) {
            continue;
        }
        let first = f.group.iter().copied().filter(|j| def.faces.get(*j).is_some_and(|g| g.trim.unsupported.is_none())).min().unwrap_or(fi);
        let fits = def.faces.get(first).is_some_and(|l| same_analytic(&l.analytic, &f.analytic, 1e-13));
        sets.entry(if fits { first } else { fi }).or_default().push(fi);
    }
    for (fi, members) in sets {
        if cancel() {
            return Err(HlrError::Cancelled);
        }
        let Some(f) = def.faces.get(fi) else { continue };
        let parts: Vec<_> = members.iter().filter_map(|j| def.faces.get(*j)).map(|m| (&m.surface, &m.trim)).collect();
        let mut sils = Vec::new();
        silhouette::silhouettes(&parts, f.analytic, dl, &mut sils);
        for s in sils {
            for r in &s.ranges {
                tracks.push(track(TrackCurve::Owned(s.curve.clone()), *r, Source::Silhouette { face: fi }, s.acc));
            }
        }
    }
    let mut out = Vec::new();
    let mut sag: f64 = 0.;
    for (ti, t) in tracks.iter_mut().enumerate() {
        if ti % 64 == 0 && cancel() {
            return Err(HlrError::Cancelled);
        }
        sample(t, &p, view, tolerance);
        let piece = Piece {
            t: t.range,
            visibility: Visibility::Visible,
            covered: false,
            geometry: Vec::new(),
            accuracy: t.acc,
            points: t.samples.iter().map(|(_, s)| *s).collect(),
            sag: t.sags.iter().copied().fold(0., f64::max),
        };
        let (geometry, accuracy) = piece_geometry(t, &piece, &p, view);
        sag = sag.max(match accuracy {
            CurveAccuracy::Analytic => 0.,
            CurveAccuracy::Bounded { max_deviation_mm } => max_deviation_mm,
            CurveAccuracy::Sampled { max_sample_deviation_mm } => max_sample_deviation_mm,
        });
        out.extend(geometry);
    }
    Ok((out, sag))
}

fn track<'a>(curve: TrackCurve<'a>, range: [f64; 2], source: Source, acc: CurveAccuracy) -> Track<'a> {
    Track { placement: 0, curve, range, source, acc, own: Vec::new(), own_eps: 0., samples: Vec::new(), sags: Vec::new(), splits: Vec::new() }
}

/// Samples a track on screen until each chord is within `tolerance` of its exact curve (a
/// spline's chords also of its control points' hull), with each chord's measured sag.
fn sample(t: &mut Track, p: &Placement, view: &View, tolerance: f64) {
    let c = t.c3();
    let screen = |x: f64| view.screen(p.apply(c.at(x)));
    let mut ts: Vec<f64> = c.breaks().into_iter().filter(|x| *x > t.range[0] && *x < t.range[1]).collect();
    ts.push(t.range[0]);
    ts.push(t.range[1]);
    ts.sort_by(f64::total_cmp);
    ts.dedup();
    let per = if matches!(c, C3::Line { .. } | C3::Polyline(_)) { 1 } else { 4 };
    let mut params = Vec::new();
    for w in ts.windows(2) {
        let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
        for i in 0..per {
            params.push(a + (b - a) * i as f64 / per as f64);
        }
    }
    params.push(t.range[1]);
    let mut samples: Vec<(f64, [f64; 2])> = params.into_iter().map(|x| (x, screen(x))).collect();
    let hull = |a: &(f64, [f64; 2]), b: &(f64, [f64; 2])| {
        c.hull([a.0, b.0]).map_or(0., |h| h.iter().map(|x| point_segment(view.screen(p.apply(*x)), a.1, b.1)).fold(0., f64::max))
    };
    let sag = |a: &(f64, [f64; 2]), b: &(f64, [f64; 2])| match c {
        C3::Line { .. } | C3::Polyline(_) => 0.,
        _ => point_segment(screen(0.5 * (a.0 + b.0)), a.1, b.1).max(hull(a, b)),
    };
    if !matches!(c, C3::Line { .. } | C3::Polyline(_)) {
        for _ in 0..16 {
            let mut next = Vec::with_capacity(samples.len() * 2);
            let mut split = false;
            for w in samples.windows(2) {
                let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
                next.push(*a);
                if sag(a, b) > tolerance && samples.len() + next.len() < MAX_SAMPLES {
                    let m = 0.5 * (a.0 + b.0);
                    next.push((m, screen(m)));
                    split = true;
                }
            }
            if let Some(last) = samples.last() {
                next.push(*last);
            }
            samples = next;
            if !split {
                break;
            }
        }
    }
    let fitted = match t.acc {
        CurveAccuracy::Bounded { max_deviation_mm } => max_deviation_mm,
        CurveAccuracy::Sampled { max_sample_deviation_mm } if matches!(c, C3::Polyline(_)) => max_sample_deviation_mm,
        _ => 0.,
    };
    t.sags = samples.windows(2).map(|w| fitted + w.first().zip(w.get(1)).map_or(0., |(a, b)| sag(a, b))).collect();
    t.samples = samples;
}
