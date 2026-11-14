//! Which parts of a placed body's edges a viewer sees, from the exact faces of every body that
//! can hide them: an interactive view's hidden lines, as opposed to a drawing's
//! ([`super::hidden_lines`]). Nothing is projected or split at crossings: each edge is sampled
//! along its exact curve, a ray goes from each sample toward the viewer (along the view for an
//! orthographic view, to the eye for a perspective one) and is tested against the trimmed faces
//! its path passes near, found through a hierarchy of the faces' boxes. Where two neighbouring
//! samples disagree, the change is found by bisection.
//!
//! A face hides a point only when the point lies deeper under it than [`Occluders::touch`]
//! (under its tangent plane where the ray meets it) or the ray meets it farther out than
//! [`Occluders::graze`], so an edge lying on its own faces, on a face
//! it ends on or on another body's face it rests against is seen. A hit on a face's boundary
//! hides (behind the meeting of two faces, or within the trim's accuracy of a body's outline); a
//! grazing hit and a face whose trim is not understood decide nothing.
//! A hidden stretch shorter than the sample spacing between two seen samples can be missed.

use super::{EdgeKind, HlrError, Placement, Prepared, slab};
use crate::Aabb3;
use crate::bvh::Bvh;
use crate::prelude::*;
use solidcore_record::Vec3;

/// Where the view is seen from.
#[derive(Clone, Copy, Debug)]
pub enum Eye {
    /// An orthographic view looking along this direction.
    Along(Vec3),
    /// A perspective view from this point.
    At(Vec3),
}

/// What a sample of an edge is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Seen {
    Visible,
    /// Hidden by this face of this placement.
    Hidden([u32; 2]),
}

impl Seen {
    fn visible(self) -> bool {
        self == Seen::Visible
    }
}

/// Halvings when two neighbouring samples are hidden by different faces: a visible gap between
/// them as narrow as this many halvings of the sample spacing is found.
const GAP_HALVINGS: u32 = 5;

/// The bodies of a view that can hide edges, by face, with the placements whose edges are asked
/// about. Built once for a view's bodies and read from any number of threads.
pub struct Occluders<'a> {
    defs: Vec<&'a Prepared>,
    /// Each placement with its body's own offset and the scene's centre folded in.
    scene: Vec<Placement>,
    /// The scene's centre: work is done relative to it, so rounding does not grow with the
    /// distance from the origin.
    origin: Vec3,
    /// `(placement, face)` of each item in `bvh`.
    items: Vec<(u32, u32)>,
    bvh: Bvh,
    /// How far under a face (mm, under its tangent plane where the ray meets it) a point must
    /// lie to be hidden by it.
    pub touch: f64,
    /// How far along the ray (mm) a face hides a point whatever its tangent plane: a point that
    /// rests on a face seen within a degree or two of edge on stays seen; a round seen edge on,
    /// or a face wrapping round in front, hides.
    pub graze: f64,
    /// Hits on another body nearer than this (mm along the ray) are rounding: ignored.
    eps_other: f64,
}

/// An edge's visible and hidden parameter ranges, in order along it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EdgePieces {
    pub visible: Vec<[f64; 2]>,
    pub hidden: Vec<[f64; 2]>,
    /// Rays cast for it.
    pub rays: usize,
    /// Faces its rays were tested against, and how many of those were spline surfaces.
    pub tests: usize,
    pub spline_tests: usize,
}

/// Default for [`Occluders::touch`]: a point less than this deep under a face is on it (contact
/// between parts as modelled, a fitted record's rounding).
pub const TOUCH_MM: f64 = 0.05;

/// Default for [`Occluders::graze`].
pub const GRAZE_MM: f64 = 0.5;

/// Most samples along one edge (before bisection).
const MAX_SAMPLES: usize = 2048;

impl<'a> Occluders<'a> {
    /// The faces of the placements `occludes` marks hide edges; every placement can be asked
    /// about. Placements must be rigid.
    pub fn new(defs: &[&'a Prepared], scene: &[Placement], occludes: &[bool], cancel: &dyn Fn() -> bool) -> Result<Occluders<'a>, HlrError> {
        // The scene's centre, as a drawing finds it.
        let (mut lo, mut hi) = (Vec3::new(f64::MAX, f64::MAX, f64::MAX), Vec3::new(f64::MIN, f64::MIN, f64::MIN));
        for p in scene {
            if !p.rigid() {
                return Err(HlrError::Invalid("placement (not a rotation and translation)"));
            }
            let def = defs.get(p.body).ok_or(HlrError::Invalid("placement body index"))?;
            for k in 0..8 {
                let w = p.apply(corner(def.lo, def.hi, k) + def.offset);
                if !(w.x.abs() <= super::MAX_COORDINATE && w.y.abs() <= super::MAX_COORDINATE && w.z.abs() <= super::MAX_COORDINATE) {
                    return Err(HlrError::Invalid("scene extent"));
                }
                lo = lo.min(w);
                hi = hi.max(w);
            }
        }
        let origin = if scene.is_empty() { Vec3::ZERO } else { (lo + hi) * 0.5 };
        let local: Vec<Placement> =
            scene.iter().map(|p| Placement { translation: p.apply(defs.get(p.body).map_or(Vec3::ZERO, |d| d.offset)) - origin, ..*p }).collect();
        let mut items = Vec::new();
        let mut boxes = Vec::new();
        let (mut lo, mut hi) = (Vec3::new(f64::MAX, f64::MAX, f64::MAX), Vec3::new(f64::MIN, f64::MIN, f64::MIN));
        for (pi, p) in local.iter().enumerate() {
            if pi % 64 == 0 && cancel() {
                return Err(HlrError::Cancelled);
            }
            let Some(def) = defs.get(p.body) else { continue };
            for k in 0..8 {
                let w = p.apply(corner(def.lo, def.hi, k));
                lo = lo.min(w);
                hi = hi.max(w);
            }
            if !occludes.get(pi).copied().unwrap_or(false) {
                continue;
            }
            for (fi, f) in def.faces.iter().enumerate() {
                let mut b = Aabb3::EMPTY;
                for k in 0..8 {
                    b.add(p.apply(corner(f.lo, f.hi, k)));
                }
                if let (Ok(pi), Ok(fi)) = (u32::try_from(pi), u32::try_from(fi)) {
                    items.push((pi, fi));
                    boxes.push(b);
                }
            }
        }
        let scale = if local.is_empty() { 1. } else { (hi - lo).len().max(1e-6) };
        Ok(Occluders {
            defs: defs.to_vec(),
            scene: local,
            origin,
            bvh: Bvh::new(&boxes),
            items,
            touch: TOUCH_MM,
            graze: GRAZE_MM,
            eps_other: 1e-12 * scale,
        })
    }

    /// Whether edge `edge` of placement `placement` is drawn in a view of the scene (seams
    /// between pieces of one surface are not), and its kind.
    pub fn edge_kind(&self, placement: usize, edge: usize) -> Option<EdgeKind> {
        let p = self.scene.get(placement)?;
        self.defs.get(p.body)?.edges.get(edge).map(|e| e.kind)
    }

    /// The parameter range of an edge's curve.
    pub fn edge_domain(&self, placement: usize, edge: usize) -> Option<[f64; 2]> {
        let p = self.scene.get(placement)?;
        Some(self.defs.get(p.body)?.edges.get(edge)?.curve.domain())
    }

    /// The point of an edge at parameter `t`, in world coordinates.
    pub fn edge_point(&self, placement: usize, edge: usize, t: f64) -> Option<Vec3> {
        let p = self.scene.get(placement)?;
        let e = self.defs.get(p.body)?.edges.get(edge)?;
        Some(p.apply(e.curve.at(t)) + self.origin)
    }

    /// Points along an edge's parameter range `r` in world coordinates, close enough that no
    /// chord between neighbours is farther than `tol(point)` from the curve at its middle (at
    /// most 4096 chords): what is drawn of the edge.
    pub fn edge_points(&self, placement: usize, edge: usize, r: [f64; 2], tol: &dyn Fn(Vec3) -> f64) -> Vec<Vec3> {
        let (Some(p), Some(e)) = (self.scene.get(placement), self.scene.get(placement).and_then(|p| self.defs.get(p.body)?.edges.get(edge))) else {
            return Vec::new();
        };
        let at = |t: f64| p.apply(e.curve.at(t)) + self.origin;
        let straight = matches!(e.curve, super::curve::C3::Line { .. });
        // Start from the curve's own pieces (knot spans, quarter turns), then halve chords whose
        // middle strays.
        let mut ts: Vec<f64> = e.curve.breaks().into_iter().filter(|t| *t > r[0] && *t < r[1]).collect();
        ts.push(r[0]);
        ts.push(r[1]);
        ts.sort_by(f64::total_cmp);
        ts.dedup();
        let mut pts: Vec<(f64, Vec3)> = ts.into_iter().map(|t| (t, at(t))).collect();
        if !straight {
            for _ in 0..12 {
                let mut next = Vec::with_capacity(pts.len() * 2);
                let mut split = false;
                for w in pts.windows(2) {
                    let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
                    next.push(*a);
                    let m = 0.5 * (a.0 + b.0);
                    let q = at(m);
                    if q.dist_to_segment(a.1, b.1) > tol(q) && next.len() < 4096 {
                        next.push((m, q));
                        split = true;
                    }
                }
                if let Some(last) = pts.last() {
                    next.push(*last);
                }
                pts = next;
                if !split {
                    break;
                }
            }
        }
        pts.into_iter().map(|(_, q)| q).filter(|q| q.is_finite()).collect()
    }

    /// The visible and hidden stretches of edge `edge` of placement `placement` seen from
    /// `eye`. Samples are at most `spacing(point)` apart (mm, world: a few pixels on the
    /// screen); a change between samples is located to a sixteenth of that.
    pub fn edge_pieces(
        &self,
        placement: usize,
        edge: usize,
        eye: Eye,
        spacing: &dyn Fn(Vec3) -> f64,
        cancel: &dyn Fn() -> bool,
    ) -> Result<EdgePieces, HlrError> {
        let p = self.scene.get(placement).ok_or(HlrError::Invalid("placement"))?;
        let def = self.defs.get(p.body).ok_or(HlrError::Invalid("placement body index"))?;
        let e = def.edges.get(edge).ok_or(HlrError::Invalid("edge index"))?;
        let eye = match eye {
            Eye::Along(d) => Eye::Along(d.normalized().ok_or(HlrError::Invalid("view direction"))?),
            Eye::At(q) if q.is_finite() => Eye::At(q - self.origin),
            Eye::At(_) => return Err(HlrError::Invalid("eye")),
        };
        // Skip the ray's own start on the edge's faces: rounding grows with where the body was
        // modelled and with the faces' size, as for a drawing.
        let size = e.faces.iter().filter_map(|f| def.faces.get(*f)).map(|f| (f.hi - f.lo).len()).fold(0., f64::max);
        let own_eps = (1e-9 * size).max(1e-12 * def.offset.len());
        let at = |t: f64| p.apply(e.curve.at(t));
        let tests = std::cell::Cell::new([0usize; 2]);
        let seen = |t: f64| self.seen(placement, &e.faces, own_eps, at(t), eye, &tests);
        let [t0, t1] = e.curve.domain();
        if !(t0.is_finite() && t1.is_finite() && t1 >= t0) {
            return Err(HlrError::Invalid("edge range"));
        }
        // Sample parameters: the curve cut into pieces by a coarse polyline, each piece cut again
        // so its samples are at most `spacing` apart where it stands.
        let coarse = if matches!(e.curve, super::curve::C3::Line { .. }) { 1 } else { 64 };
        let mut params = vec![t0];
        for i in 0..coarse {
            let (a, b) = (t0 + (t1 - t0) * i as f64 / coarse as f64, t0 + (t1 - t0) * (i + 1) as f64 / coarse as f64);
            let (pa, pb) = (at(a), at(b));
            let step = spacing(pa + self.origin).min(spacing(pb + self.origin));
            let n = if step > 0. && step.is_finite() { ((pa.dist(pb) / step).ceil() as usize).clamp(1, MAX_SAMPLES) } else { 1 };
            for k in 1..=n {
                params.push(a + (b - a) * k as f64 / n as f64);
            }
            if params.len() > MAX_SAMPLES {
                break;
            }
        }
        if params.last().is_some_and(|l| *l < t1) {
            params.push(t1);
        }
        let mut out = EdgePieces::default();
        let mut samples: Vec<(f64, Seen)> = Vec::with_capacity(params.len());
        for (i, t) in params.iter().enumerate() {
            if i % 64 == 0 && cancel() {
                return Err(HlrError::Cancelled);
            }
            samples.push((*t, seen(*t)));
            out.rays += 1;
        }
        // Two neighbours hidden by different faces may have a gap between those faces where
        // the edge shows: look for it by halving toward where the hiding face changes.
        let mut found = Vec::new();
        for w in samples.windows(2) {
            if let [(a, Seen::Hidden(fa)), (b, Seen::Hidden(fb))] = *w
                && fa != fb
            {
                gap(&seen, (a, fa), (b, fb), GAP_HALVINGS, &mut found, &mut out.rays);
            }
        }
        if !found.is_empty() {
            samples.extend(found);
            samples.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        let mut start = t0;
        for k in 0..samples.len() {
            let Some(&(x, s)) = samples.get(k) else { continue };
            match samples.get(k + 1) {
                Some(&(nx, next)) if next.visible() != s.visible() => {
                    let (mut lo, mut hi) = (x, nx);
                    for _ in 0..4 {
                        let mid = 0.5 * (lo + hi);
                        out.rays += 1;
                        if seen(mid).visible() == s.visible() {
                            lo = mid;
                        } else {
                            hi = mid;
                        }
                    }
                    let cut = 0.5 * (lo + hi);
                    push(&mut out, s, [start, cut]);
                    start = cut;
                }
                Some(_) => {}
                None => push(&mut out, s, [start, t1]),
            }
        }
        [out.tests, out.spline_tests] = tests.get();
        Ok(out)
    }

    /// Whether `point` (scene coordinates) on placement `placement`'s faces `own` is seen.
    fn seen(&self, placement: usize, own: &[usize], own_eps: f64, point: Vec3, eye: Eye, tests: &std::cell::Cell<[usize; 2]>) -> Seen {
        let (dir, far) = match eye {
            Eye::Along(d) => (-d, f64::INFINITY),
            Eye::At(q) => match (q - point).normalized() {
                Some(d) => (d, q.dist(point)),
                None => return Seen::Visible,
            },
        };
        let reach = |b: &Aabb3| slab(point, dir, b.min, b.max).is_some_and(|[l0, _]| l0 <= far);
        let hidden: std::cell::Cell<Option<[u32; 2]>> = std::cell::Cell::new(None);
        let mut hits = Vec::new();
        self.bvh.visit(
            |b| hidden.get().is_none() && reach(b),
            |i| {
                if hidden.get().is_some() {
                    return;
                }
                let Some(&(qi, fi)) = self.items.get(i as usize) else { return };
                let (qi, fi) = (qi as usize, fi as usize);
                let Some(q) = self.scene.get(qi) else { return };
                let Some(f) = self.defs.get(q.body).and_then(|d| d.faces.get(fi)) else { return };
                let (p0, dl) = (q.local(point), q.local_dir(dir));
                let Some([l0, l1]) = slab(p0, dl, f.lo, f.hi) else { return };
                let own_face = qi == placement && own.contains(&fi);
                let range = [l0.max(if own_face { own_eps } else { self.eps_other }), l1.min(far)];
                if !(range[1] >= range[0]) {
                    return;
                }
                let [n, splines] = tests.get();
                tests.set([n + 1, splines + usize::from(matches!(f.surface, super::surface::S3::Nurbs(_)))]);
                hits.clear();
                f.surface.ray_as(&f.analytic, p0, dl, range, &mut hits);
                for h in &hits {
                    if h.grazing || h.lam <= self.touch {
                        continue;
                    }
                    // Deep enough under the face, measured under its tangent plane at the hit so a
                    // point resting on a face seen at a slant stays seen; or far enough along the
                    // ray, so a round seen almost edge on (its tangent plane nearly along the ray)
                    // or a face wrapping round in front still hides.
                    let depth = f.surface.normal(h.uv).map_or(h.lam, |n| h.lam * n.dot(dl).abs());
                    if !(depth > self.touch || h.lam > self.graze) {
                        continue;
                    }
                    // Inside the face, or on its boundary to within its trim's accuracy: where two
                    // faces of a body meet the point is behind both, and at a body's outline it
                    // is hidden or seen by less than that accuracy.
                    let s = f.surface.point(h.uv);
                    let near = f.trim.near(s, 2. * f.trim.chord_error + 1e-7 * (f.hi - f.lo).len());
                    if f.trim.unsupported.is_none() && (near || f.trim.contains(h.uv) == Some(true)) {
                        hidden.set(Some([qi as u32, fi as u32]));
                        return;
                    }
                }
            },
        );
        hidden.get().map_or(Seen::Visible, Seen::Hidden)
    }
}

/// Look between `a` and `b`, hidden by faces `fa` and `fb`, for a sample that is seen, halving
/// toward where the hiding face changes at most `halvings` times; the samples taken go to `found`.
fn gap(
    seen: &dyn Fn(f64) -> Seen,
    (a, fa): (f64, [u32; 2]),
    (b, fb): (f64, [u32; 2]),
    halvings: u32,
    found: &mut Vec<(f64, Seen)>,
    rays: &mut usize,
) {
    if halvings == 0 {
        return;
    }
    let m = 0.5 * (a + b);
    let s = seen(m);
    *rays += 1;
    found.push((m, s));
    if let Seen::Hidden(fm) = s {
        if fm != fa {
            gap(seen, (a, fa), (m, fm), halvings - 1, found, rays);
        }
        if fm != fb {
            gap(seen, (m, fm), (b, fb), halvings - 1, found, rays);
        }
    }
}

fn push(out: &mut EdgePieces, s: Seen, r: [f64; 2]) {
    let list = if s.visible() { &mut out.visible } else { &mut out.hidden };
    if r[1] > r[0] {
        list.push(r);
    }
}

fn corner(lo: Vec3, hi: Vec3, k: usize) -> Vec3 {
    Vec3::new(if k & 1 == 0 { lo.x } else { hi.x }, if k & 2 == 0 { lo.y } else { hi.y }, if k & 4 == 0 { lo.z } else { hi.z })
}
