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

// Moved from SolidCraft's `crates/geom/src/hlr` as it was, so the move reads line for line against
// it: the pedantic restyling these would ask for waits for a change of its own. A comparison that
// is not true takes the refusing branch, so a NaN is refused too.
#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    clippy::cast_lossless,
    clippy::single_match_else,
    clippy::manual_let_else,
    clippy::semicolon_if_nothing_returned,
    clippy::type_complexity,
    clippy::match_same_arms,
    clippy::items_after_statements,
    clippy::redundant_closure_for_method_calls,
    clippy::nonminimal_bool,
    clippy::option_if_let_else,
    clippy::map_unwrap_or,
    clippy::unnecessary_wraps,
    clippy::doc_markdown
)]

use super::vec::V3;
use super::{slab, EdgeKind, HlrError, Placement, Prepared};
use crate::math::Vec3;

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
    /// Stretches seen, in order along the edge.
    pub visible: Vec<[f64; 2]>,
    /// Stretches hidden, in order along the edge.
    pub hidden: Vec<[f64; 2]>,
    /// Rays cast for it.
    pub rays: usize,
    /// Stretches found hidden whole by one plane face without sampling them.
    pub covered: usize,
    /// Faces its rays were tested against.
    pub tests: usize,
    /// Of those, how many were spline surfaces.
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
    pub fn new(
        defs: &[&'a Prepared],
        scene: &[Placement],
        occludes: &[bool],
        cancel: &dyn Fn() -> bool,
    ) -> Result<Occluders<'a>, HlrError> {
        // The scene's centre, as a drawing finds it.
        let (mut lo, mut hi) = (
            Vec3::new(f64::MAX, f64::MAX, f64::MAX),
            Vec3::new(f64::MIN, f64::MIN, f64::MIN),
        );
        for p in scene {
            if !p.rigid() {
                return Err(HlrError::Invalid(
                    "placement (not a rotation and translation)",
                ));
            }
            let def = defs
                .get(p.body)
                .ok_or(HlrError::Invalid("placement body index"))?;
            for k in 0..8 {
                let w = p.apply(corner(def.lo, def.hi, k) + def.offset);
                if !(w.x.abs() <= super::MAX_COORDINATE
                    && w.y.abs() <= super::MAX_COORDINATE
                    && w.z.abs() <= super::MAX_COORDINATE)
                {
                    return Err(HlrError::Invalid("scene extent"));
                }
                lo = lo.min(w);
                hi = hi.max(w);
            }
        }
        let origin = if scene.is_empty() {
            Vec3::ZERO
        } else {
            (lo + hi) * 0.5
        };
        let local: Vec<Placement> = scene
            .iter()
            .map(|p| Placement {
                translation: p.apply(defs.get(p.body).map_or(Vec3::ZERO, |d| d.offset)) - origin,
                ..*p
            })
            .collect();
        let mut items = Vec::new();
        let mut boxes = Vec::new();
        let (mut lo, mut hi) = (
            Vec3::new(f64::MAX, f64::MAX, f64::MAX),
            Vec3::new(f64::MIN, f64::MIN, f64::MIN),
        );
        for (pi, p) in local.iter().enumerate() {
            if pi % 64 == 0 && cancel() {
                return Err(HlrError::Cancelled);
            }
            let Some(def) = defs.get(p.body) else {
                continue;
            };
            for k in 0..8 {
                let w = p.apply(corner(def.lo, def.hi, k));
                lo = lo.min(w);
                hi = hi.max(w);
            }
            if !occludes.get(pi).copied().unwrap_or(false) {
                continue;
            }
            for (fi, f) in def.faces.iter().enumerate() {
                let mut b = Box3::EMPTY;
                for k in 0..8 {
                    b.add(p.apply(corner(f.lo, f.hi, k)));
                }
                if let (Ok(pi), Ok(fi)) = (u32::try_from(pi), u32::try_from(fi)) {
                    items.push((pi, fi));
                    boxes.push(b);
                }
            }
        }
        let scale = if local.is_empty() {
            1.
        } else {
            (hi - lo).len().max(1e-6)
        };
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
    /// most 4096 chords): what is drawn of the edge, with each point's parameter.
    pub fn edge_points(
        &self,
        placement: usize,
        edge: usize,
        r: [f64; 2],
        tol: &dyn Fn(Vec3) -> f64,
    ) -> Vec<(f64, Vec3)> {
        let (Some(p), Some(e)) = (
            self.scene.get(placement),
            self.scene
                .get(placement)
                .and_then(|p| self.defs.get(p.body)?.edges.get(edge)),
        ) else {
            return Vec::new();
        };
        let at = |t: f64| p.apply(e.curve.at(t)) + self.origin;
        let straight = matches!(e.curve, super::curve::C3::Line { .. });
        // Start from the curve's own pieces (knot spans, quarter turns), then halve chords whose
        // middle strays.
        let mut ts: Vec<f64> = e
            .curve
            .breaks()
            .into_iter()
            .filter(|t| *t > r[0] && *t < r[1])
            .collect();
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
                    let (Some(a), Some(b)) = (w.first(), w.get(1)) else {
                        continue;
                    };
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
        pts.into_iter().filter(|(_, q)| q.is_finite()).collect()
    }

    /// The unit normals (world, either side) of the first two faces an edge bounds, at its
    /// parameter `t`; a zero vector for a face it has not or whose surface does not evaluate there.
    pub fn edge_normals(&self, placement: usize, edge: usize, t: f64) -> [Vec3; 2] {
        let mut out = [Vec3::ZERO; 2];
        let Some(p) = self.scene.get(placement) else {
            return out;
        };
        let Some(def) = self.defs.get(p.body) else {
            return out;
        };
        let Some(e) = def.edges.get(edge) else {
            return out;
        };
        let at = e.curve.at(t);
        let mut faces = e.faces.clone();
        faces.dedup();
        for (slot, fi) in out.iter_mut().zip(faces) {
            let Some(f) = def.faces.get(fi) else { continue };
            if let Some(n) = f
                .surface
                .inverse(at, None)
                .and_then(|uv| f.surface.normal(uv))
            {
                let [a, b, c] = p.rotation;
                *slot = a * n.x + b * n.y + c * n.z;
            }
        }
        out
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
        let p = self
            .scene
            .get(placement)
            .ok_or(HlrError::Invalid("placement"))?;
        let def = self
            .defs
            .get(p.body)
            .ok_or(HlrError::Invalid("placement body index"))?;
        let e = def.edges.get(edge).ok_or(HlrError::Invalid("edge index"))?;
        let eye = match eye {
            Eye::Along(d) => Eye::Along(d.normalized().ok_or(HlrError::Invalid("view direction"))?),
            Eye::At(q) if q.is_finite() => Eye::At(q - self.origin),
            Eye::At(_) => return Err(HlrError::Invalid("eye")),
        };
        // Skip the ray's own start on the edge's faces: rounding grows with where the body was
        // modelled and with the faces' size, as for a drawing.
        let size = e
            .faces
            .iter()
            .filter_map(|f| def.faces.get(*f))
            .map(|f| (f.hi - f.lo).len())
            .fold(0., f64::max);
        let own_eps = (1e-9 * size).max(1e-12 * def.offset.len());
        let at = |t: f64| p.apply(e.curve.at(t));
        let tests = std::cell::Cell::new([0usize; 2]);
        let seen = |t: f64| self.seen(placement, &e.faces, own_eps, at(t), eye, &tests);
        let scratch = std::cell::RefCell::new(Vec::new());
        let hides = |face: [u32; 2], t: f64| {
            let point = at(t);
            let (Some(toward), Ok(mut hits)) = (Self::toward(point, eye), scratch.try_borrow_mut())
            else {
                return false;
            };
            self.hides(
                (face[0], face[1]),
                (placement, &e.faces, own_eps),
                point,
                toward,
                &tests,
                &mut hits,
            )
        };
        let [t0, t1] = e.curve.domain();
        if !(t0.is_finite() && t1.is_finite() && t1 >= t0) {
            return Err(HlrError::Invalid("edge range"));
        }
        // Sample parameters: the curve cut into pieces by a coarse polyline, each piece cut again
        // so its samples are at most `spacing` apart where it stands.
        let straight = matches!(e.curve, super::curve::C3::Line { .. });
        let coarse = if straight { 1 } else { 64 };
        // Each coarse piece's length in sample spacings where it stands; the samples are shared out
        // by it, so an edge a few pixels long on the screen takes a few rays whatever its shape.
        let weights: Vec<f64> = (0..coarse)
            .map(|i| {
                let (a, b) = (
                    t0 + (t1 - t0) * i as f64 / coarse as f64,
                    t0 + (t1 - t0) * (i + 1) as f64 / coarse as f64,
                );
                let (pa, pb) = (at(a), at(b));
                let step = spacing(pa + self.origin).min(spacing(pb + self.origin));
                if step > 0. && step.is_finite() {
                    pa.dist(pb) / step
                } else {
                    0.
                }
            })
            .collect();
        let total: f64 = weights.iter().sum();
        let n = if total.is_finite() {
            (total.ceil() as usize).clamp(1, MAX_SAMPLES)
        } else {
            1
        };
        let mut params = Vec::with_capacity(n + 1);
        params.push(t0);
        let mut acc = 0.;
        for (i, w) in weights.iter().enumerate() {
            // The samples at whole multiples of `total / n` that fall in this piece, placed in it
            // evenly by its share.
            let (a, b) = (
                t0 + (t1 - t0) * i as f64 / coarse as f64,
                t0 + (t1 - t0) * (i + 1) as f64 / coarse as f64,
            );
            let (from, to) = (acc, acc + w);
            let first = ((from * n as f64 / total.max(f64::MIN_POSITIVE)).floor() as usize) + 1;
            let mut k = first;
            while k <= n {
                let at_k = k as f64 * total / n as f64;
                if at_k > to {
                    break;
                }
                if *w > 0. {
                    params.push(a + (b - a) * ((at_k - from) / w).clamp(0., 1.));
                }
                k += 1;
            }
            acc = to;
        }
        if params.last().is_none_or(|l| *l < t1) {
            params.push(t1);
        }
        params.dedup();
        let mut out = EdgePieces::default();
        let mut states: Vec<Option<Seen>> = vec![None; params.len()];
        if straight && params.len() > 2 {
            // A straight edge: from its ends inward, a stretch whose ends a plane face hides and
            // which that face covers whole is hidden without the samples between.
            let (first, last) = (0, params.len() - 1);
            for k in [first, last] {
                if let (Some(slot), Some(t)) = (states.get_mut(k), params.get(k)) {
                    *slot = Some(seen(*t));
                    out.rays += 1;
                }
            }
            let mut todo = vec![(first, last)];
            while let Some((lo, hi)) = todo.pop() {
                if hi - lo < 2 {
                    continue;
                }
                if cancel() {
                    return Err(HlrError::Cancelled);
                }
                let (Some(&Some(a)), Some(&Some(b))) = (states.get(lo), states.get(hi)) else {
                    continue;
                };
                if let (Seen::Hidden(fa), Seen::Hidden(fb)) = (a, b) {
                    // One plane face may hide the whole stretch: no samples needed.
                    let covered = fa == fb
                        && params
                            .get(lo)
                            .zip(params.get(hi))
                            .is_some_and(|(ta, tb)| self.covers(fa, at(*ta), at(*tb), eye));
                    if covered {
                        states
                            .iter_mut()
                            .take(hi)
                            .skip(lo + 1)
                            .for_each(|s| *s = Some(a));
                        out.covered += 1;
                        continue;
                    }
                }
                let mid = (lo + hi) / 2;
                if let (Some(slot), Some(t)) = (states.get_mut(mid), params.get(mid)) {
                    *slot = Some(seen(*t));
                    out.rays += 1;
                }
                todo.push((mid, hi));
                todo.push((lo, mid));
            }
        }
        let mut samples: Vec<(f64, Seen)> = Vec::with_capacity(params.len());
        for (i, (t, known)) in params.iter().zip(states).enumerate() {
            if i % 64 == 0 && cancel() {
                return Err(HlrError::Cancelled);
            }
            let s = match known {
                Some(s) => s,
                None => {
                    out.rays += 1;
                    seen(*t)
                }
            };
            samples.push((*t, s));
        }
        // Two neighbours hidden by different faces may have a gap between those faces where
        // the edge shows: look for it by halving toward where the hiding face changes.
        let mut found = Vec::new();
        for w in samples.windows(2) {
            let [(a, Seen::Hidden(fa)), (b, Seen::Hidden(fb))] = *w else {
                continue;
            };
            if fa == fb {
                continue;
            }
            gap(
                &seen,
                &hides,
                &|x, y| self.joined(x, y),
                (a, fa),
                (b, fb),
                GAP_HALVINGS,
                &mut found,
                &mut out.rays,
            );
        }
        if !found.is_empty() {
            samples.extend(found);
            samples.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        let mut start = t0;
        for k in 0..samples.len() {
            let Some(&(x, s)) = samples.get(k) else {
                continue;
            };
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

    /// Whether two faces (`[placement, face]`) are faces of one placed body that share an edge.
    fn joined(&self, [qa, fa]: [u32; 2], [qb, fb]: [u32; 2]) -> bool {
        qa == qb
            && self
                .scene
                .get(qa as usize)
                .and_then(|q| self.defs.get(q.body)?.faces.get(fa as usize))
                .is_some_and(|f| f.neighbours.binary_search(&(fb as usize)).is_ok())
    }

    /// The direction from `point` (scene coordinates) toward the viewer and how far the viewer
    /// is; `None` at the eye itself.
    fn toward(point: Vec3, eye: Eye) -> Option<(Vec3, f64)> {
        match eye {
            Eye::Along(d) => Some((-d, f64::INFINITY)),
            Eye::At(q) => (q - point).normalized().map(|d| (d, q.dist(point))),
        }
    }

    /// Whether `point` (scene coordinates) on placement `placement`'s faces `own` is seen.
    fn seen(
        &self,
        placement: usize,
        own: &[usize],
        own_eps: f64,
        point: Vec3,
        eye: Eye,
        tests: &std::cell::Cell<[usize; 2]>,
    ) -> Seen {
        let Some((dir, far)) = Self::toward(point, eye) else {
            return Seen::Visible;
        };
        let reach = |b: &Box3| slab(point, dir, b.lo, b.hi).is_some_and(|[l0, _]| l0 <= far);
        let hidden: std::cell::Cell<Option<[u32; 2]>> = std::cell::Cell::new(None);
        let mut hits = Vec::new();
        self.bvh.visit(
            |b| hidden.get().is_none() && reach(b),
            |i| {
                if hidden.get().is_some() {
                    return;
                }
                let Some(&face) = self.items.get(i as usize) else {
                    return;
                };
                if self.hides(
                    face,
                    (placement, own, own_eps),
                    point,
                    (dir, far),
                    tests,
                    &mut hits,
                ) {
                    hidden.set(Some([face.0, face.1]));
                }
            },
        );
        hidden.get().map_or(Seen::Visible, Seen::Hidden)
    }

    /// Whether face `(qi, fi)` hides `point` (scene coordinates, on placement `placement`'s faces
    /// `own`) from a viewer along `dir`, `far` away.
    fn hides(
        &self,
        (qi, fi): (u32, u32),
        (placement, own, own_eps): (usize, &[usize], f64),
        point: Vec3,
        (dir, far): (Vec3, f64),
        tests: &std::cell::Cell<[usize; 2]>,
        hits: &mut Vec<super::surface::Hit>,
    ) -> bool {
        let (qi, fi) = (qi as usize, fi as usize);
        let Some(q) = self.scene.get(qi) else {
            return false;
        };
        let Some(f) = self.defs.get(q.body).and_then(|d| d.faces.get(fi)) else {
            return false;
        };
        let (p0, dl) = (q.local(point), q.local_dir(dir));
        let Some([l0, l1]) = slab(p0, dl, f.lo, f.hi) else {
            return false;
        };
        let own_face = qi == placement && own.contains(&fi);
        let range = [
            l0.max(if own_face { own_eps } else { self.eps_other }),
            l1.min(far),
        ];
        if !(range[1] >= range[0]) {
            return false;
        }
        let [n, splines] = tests.get();
        tests.set([
            n + 1,
            splines + usize::from(matches!(f.surface, super::surface::S3::Nurbs(_))),
        ]);
        hits.clear();
        f.surface.ray_as(&f.analytic, p0, dl, range, hits);
        hits.iter().any(|h| {
            if h.grazing || h.lam <= self.touch {
                return false;
            }
            // Deep enough under the face, measured under its tangent plane at the hit so a point
            // resting on a face seen at a slant stays seen; or far enough along the ray, so a
            // round seen almost edge on (its tangent plane nearly along the ray) or a face
            // wrapping round in front still hides.
            let depth = f
                .surface
                .normal(h.uv)
                .map_or(h.lam, |n| h.lam * n.dot(dl).abs());
            if !(depth > self.touch || h.lam > self.graze) {
                return false;
            }
            // Inside the face, or on its boundary to within its trim's accuracy: where two faces
            // of a body meet the point is behind both, and at a body's outline it is hidden or
            // seen by less than that accuracy.
            let s = f.surface.point(h.uv);
            let near = f
                .trim()
                .near(s, 2. * f.trim().chord_error + 1e-7 * (f.hi - f.lo).len());
            f.trim().unsupported.is_none() && (near || f.trim().contains(h.uv) == Some(true))
        })
    }
}

impl Occluders<'_> {
    /// Whether plane face `face` (`[placement, face]`) hides the whole straight stretch from `a`
    /// to `b` (scene coordinates) seen from `eye`: both ends deeper behind its plane than
    /// [`Occluders::touch`], and the stretch's shadow on the plane (a segment, also seen from a
    /// point) inside the face and clear of its boundary by the trim's accuracy.
    fn covers(&self, [qi, fi]: [u32; 2], a: Vec3, b: Vec3, eye: Eye) -> bool {
        let Some(q) = self.scene.get(qi as usize) else {
            return false;
        };
        let Some(f) = self.defs.get(q.body).and_then(|d| d.faces.get(fi as usize)) else {
            return false;
        };
        let super::surface::S3::Plane { o, n, .. } = f.surface else {
            return false;
        };
        if f.trim().unsupported.is_some() {
            return false;
        }
        let tol = 2. * f.trim().chord_error + 1e-7 * (f.hi - f.lo).len();
        let shadow = |p: Vec3| -> Option<Vec3> {
            let dir = match eye {
                Eye::Along(d) => -d,
                Eye::At(e) => (e - p).normalized()?,
            };
            let (p, dl) = (q.local(p), q.local_dir(dir));
            let along = dl.dot(n);
            if along.abs() <= 1e-12 {
                return None;
            }
            let lam = (o - p).dot(n) / along;
            // In front of the point and deep enough behind the face.
            let depth = lam * along.abs();
            let behind_eye = match eye {
                Eye::Along(_) => false,
                Eye::At(e) => lam >= q.local(e).dist(p),
            };
            if behind_eye {
                return None;
            }
            (lam > self.touch && depth > self.touch).then_some(p + dl * lam)
        };
        let (Some(sa), Some(sb)) = (shadow(a), shadow(b)) else {
            return false;
        };
        if f.trim().near(sa, tol)
            || f.surface
                .inverse(sa, None)
                .and_then(|uv| f.trim().contains(uv))
                != Some(true)
        {
            return false;
        }
        f.trim().segment_clear(sa, sb, tol) == Some(true)
    }
}

/// Look between `a` and `b`, hidden by faces `fa` and `fb`, for a sample that is seen, halving
/// toward where the hiding face changes at most `halvings` times; the samples taken go to `found`.
fn gap(
    seen: &dyn Fn(f64) -> Seen,
    hides: &dyn Fn([u32; 2], f64) -> bool,
    joined: &dyn Fn([u32; 2], [u32; 2]) -> bool,
    (a, fa): (f64, [u32; 2]),
    (b, fb): (f64, [u32; 2]),
    halvings: u32,
    found: &mut Vec<(f64, Seen)>,
    rays: &mut usize,
) {
    // Faces of one body that share an edge, or that overlap there (either one also hides the
    // other's sample), leave no gap between the samples, as one face hiding both does not.
    if halvings == 0 || joined(fa, fb) || hides(fb, a) || hides(fa, b) {
        return;
    }
    let m = 0.5 * (a + b);
    let s = seen(m);
    *rays += 1;
    found.push((m, s));
    if let Seen::Hidden(fm) = s {
        if fm != fa {
            gap(
                seen,
                hides,
                joined,
                (a, fa),
                (m, fm),
                halvings - 1,
                found,
                rays,
            );
        }
        if fm != fb {
            gap(
                seen,
                hides,
                joined,
                (m, fm),
                (b, fb),
                halvings - 1,
                found,
                rays,
            );
        }
    }
}

fn push(out: &mut EdgePieces, s: Seen, r: [f64; 2]) {
    let list = if s.visible() {
        &mut out.visible
    } else {
        &mut out.hidden
    };
    if r[1] > r[0] {
        list.push(r);
    }
}

fn corner(lo: Vec3, hi: Vec3, k: usize) -> Vec3 {
    Vec3::new(
        if k & 1 == 0 { lo.x } else { hi.x },
        if k & 2 == 0 { lo.y } else { hi.y },
        if k & 4 == 0 { lo.z } else { hi.z },
    )
}

/// An axis-aligned box: the space placed corners, or a face's boxes, span.
#[derive(Clone, Copy, Debug)]
struct Box3 {
    lo: Vec3,
    hi: Vec3,
}

impl Box3 {
    /// No points; [`Box3::add`] gives the box of the points added.
    const EMPTY: Box3 = Box3 {
        lo: Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
        hi: Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
    };

    /// Grows the box to hold `p` (a point that is not finite is left out).
    fn add(&mut self, p: Vec3) {
        if p.is_finite() {
            self.lo = self.lo.min(p);
            self.hi = self.hi.max(p);
        }
    }

    /// The box around this one and `o`.
    fn union(&self, o: &Box3) -> Box3 {
        Box3 {
            lo: self.lo.min(o.lo),
            hi: self.hi.max(o.hi),
        }
    }

    fn is_empty(&self) -> bool {
        !(self.lo.x <= self.hi.x && self.lo.y <= self.hi.y && self.lo.z <= self.hi.z)
    }

    fn finite(&self) -> bool {
        self.lo.is_finite() && self.hi.is_finite()
    }

    fn center(&self) -> Vec3 {
        if self.is_empty() {
            Vec3::ZERO
        } else {
            (self.lo + self.hi) * 0.5
        }
    }

    fn size(&self) -> Vec3 {
        if self.is_empty() {
            Vec3::ZERO
        } else {
            self.hi - self.lo
        }
    }
}

/// Most items in a [`Bvh`] leaf.
const LEAF: usize = 4;

/// A node of a [`Bvh`].
#[derive(Clone, Copy, Debug)]
struct Node {
    bounds: Box3,
    /// A leaf: the first of its `count` items in [`Bvh::order`]. Otherwise its children are
    /// `first` and `first + 1`.
    first: u32,
    count: u32,
}

/// A hierarchy over boxes: every item in a leaf whose box, and every box above it, a ray enters,
/// found without looking at each item. Built once (median split on the widest axis of the box
/// centres, a few items per leaf) and only read afterwards.
#[derive(Clone, Debug, Default)]
struct Bvh {
    nodes: Vec<Node>,
    order: Vec<u32>,
}

impl Bvh {
    /// The hierarchy over `boxes` (empty boxes are left out).
    fn new(boxes: &[Box3]) -> Bvh {
        let mut order: Vec<u32> = (0..boxes.len().min(u32::MAX as usize))
            .filter(|&i| boxes.get(i).is_some_and(|b| !b.is_empty() && b.finite()))
            .map(|i| i as u32)
            .collect();
        if order.is_empty() {
            return Bvh::default();
        }
        let item = |i: u32| boxes.get(i as usize).copied().unwrap_or(Box3::EMPTY);
        let centre = |i: u32| item(i).center();
        let mut nodes = vec![Node {
            bounds: Box3::EMPTY,
            first: 0,
            count: 0,
        }];
        // (node, start, end) in `order` still to be filled in.
        let mut todo = vec![(0usize, 0usize, order.len())];
        while let Some((n, start, end)) = todo.pop() {
            let Some(span) = order.get_mut(start..end) else {
                continue;
            };
            let mut bounds = Box3::EMPTY;
            let mut centres = Box3::EMPTY;
            for &i in span.iter() {
                bounds = bounds.union(&item(i));
                centres.add(centre(i));
            }
            let size = centres.size();
            let axis = if size.x >= size.y && size.x >= size.z {
                0
            } else if size.y >= size.z {
                1
            } else {
                2
            };
            let extent = [size.x, size.y, size.z][axis];
            if span.len() <= LEAF || !(extent > 0.0) {
                if let Some(node) = nodes.get_mut(n) {
                    *node = Node {
                        bounds,
                        first: start as u32,
                        count: span.len() as u32,
                    };
                }
                continue;
            }
            let key = |i: &u32| {
                let c = centre(*i);
                [c.x, c.y, c.z][axis]
            };
            let mid = span.len() / 2;
            span.select_nth_unstable_by(mid, |a, b| key(a).total_cmp(&key(b)));
            let left = nodes.len();
            nodes.push(Node {
                bounds: Box3::EMPTY,
                first: 0,
                count: 0,
            });
            nodes.push(Node {
                bounds: Box3::EMPTY,
                first: 0,
                count: 0,
            });
            if let Some(node) = nodes.get_mut(n) {
                *node = Node {
                    bounds,
                    first: left as u32,
                    count: 0,
                };
            }
            todo.push((left, start, start + mid));
            todo.push((left + 1, start + mid, end));
        }
        Bvh { nodes, order }
    }

    /// Calls `item` with every item in a leaf whose box, and every box above it, `enter` accepts.
    fn visit(&self, mut enter: impl FnMut(&Box3) -> bool, mut item: impl FnMut(u32)) {
        let mut stack = vec![0usize];
        while let Some(n) = stack.pop() {
            let Some(node) = self.nodes.get(n) else {
                continue;
            };
            if !enter(&node.bounds) {
                continue;
            }
            if node.count > 0 {
                let (a, b) = (
                    node.first as usize,
                    node.first as usize + node.count as usize,
                );
                for &i in self.order.get(a..b).unwrap_or(&[]) {
                    item(i);
                }
            } else {
                stack.push(node.first as usize + 1);
                stack.push(node.first as usize);
            }
        }
    }
}
