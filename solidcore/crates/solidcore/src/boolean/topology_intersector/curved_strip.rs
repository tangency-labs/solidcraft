//! Not upstream (tangency-labs/solidcraft#18166, generalising #15718's
//! [`super::coincident_strip`] to a curved face): a surface/surface curve of a face pair
//! that runs, inside a face A, from an end of an edge E of A to A's boundary next to E's
//! other end, over a strip of A that lies within the intersection tolerance of the other
//! face's surface B, is no event: the pair's intersection there is E, which already lies
//! on B, and the curve is dropped from the pair before any edge or vertex definition is
//! made of it ([`drop_strip_crossings`]).
//!
//! A face A that leaves B tangentially along E (a rolling-ball blend leaving the cone it
//! rolls on, whose neighbour across E is B's coincident piece) lies within the tolerance
//! of B over a strip beside E. The surface/surface intersection of A and B is then fixed
//! only to that strip: the marching finds a curve C anywhere in it, from E's end X to the
//! point where it crosses A's boundary edge N next to E's other end V. C cuts a strip off
//! A that is within the tolerance of B (so of A's coincident neighbour) everywhere, and
//! leaves C's end on N and V as two vertices farther apart than any tolerance, which the
//! later merge cannot relate (`make_faceuse`: "the related vertex is not an end of the
//! edge use"). Within the tolerance, E and C are one intersection: E is kept (it is
//! imprinted on B by the coincident pair of B and A's neighbour), C is dropped, and C's
//! end is never made a vertex, so it and V need not be related.
//!
//! The decision is a certificate over A's strip, not a sample. The strip is the region of
//! A's parameter plane bounded by E's trim on A, C's UV curve on A over the curve's
//! interval, and the piece of N's trim on A between V and C's end. Each piece is cut into
//! equal parameter pieces, and each of those lies in the box of its certified hull points
//! (`Curve::piece_hull_points`, their coordinates' errors added); where two pieces meet,
//! the segment closing the gap between their ends lies in the box of their end boxes. So
//! the closed boundary lies in the union of those boxes. The span of the boxes in u is cut
//! into equal slabs: a vertical line through the strip meets it between points of its
//! boundary on that line, each in a box meeting the slab, so the strip over a slab lies in
//! the slab times the span in v of the boxes meeting it, cut to the surface's domain (A's
//! points are its surface's over its domain). A's surface over that box is the restriction
//! of its net (a B-spline face's own surface, or a procedural blend's certified net, whose
//! distance from the blend's definition is its bound): the net's sub-patch over the box
//! (extracted by knot insertion and checked to cover it) is enclosed against B's implicit
//! equation by `implicit::off_surface_bound` (the Bernstein coefficients of the equation,
//! their rounding and the extraction's counted, turned into a distance by the cone's least
//! radius over the patch). When the largest of those bounds plus the net's bound is within
//! the tolerance, every point of the strip is within the tolerance of B's surface (PROVEN
//! for the surface, given that the strip is what the three pieces bound).
//!
//! What is ARGUED, not proven: that the UV curves and trims are the 3D curves' images at
//! the same parameters (the surface/surface curve's UV curve is made with it; a trim is
//! its edge's at the edge's parameters). Each is checked at the ends used, where the
//! surface's point at the UV point must lie within the tolerance of the 3D point, plus the
//! vertex's tolerance at an edge's end vertex and the edge's inside it; a curve or trim that fails is not used and the pair is intersected as
//! upstream does. And that the strip lies over B's face, not only its surface: E and C
//! both lie on B's face (E imprinted by the coincident pair, C classified inside it).
//!
//! A true crossing is not dropped: where A crosses B along C (or leaves it past the
//! tolerance before C), A's points between E and C reach farther from B than the
//! tolerance, the bound exceeds it, and the curve stays.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`drop_strip_crossings`] (super) | none (own design) |
//!
//! Deviations: not upstream at all; always on. B is a plane or a cone (a cylinder
//! included), A a B-spline face or a procedural blend; every other pair is intersected as
//! upstream does. No recursion.

use crate::curve::curve::p2::Curve;
use crate::error::Result;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::solution_array::SolutionObject;
use crate::intersect::curve_class::p1::CurveInterval;
use crate::intersect::point_class::{ClassGeometry, ClassObject, PointClassType};
use crate::kernel::Kernel;
use crate::math::{Point2, Point3};
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::surface::implicit::{off_surface_bound, AnalyticSupport, Implicit};
use crate::surface::surface::p2::SurfaceKind;
use crate::surface::surface::Surface;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p3 as eu3;
use crate::topology::{edge, vertex, vertexuse, Brep, EdgeId, EdgeuseId, FaceId, VertexId};
use crate::EntityRef;

use super::coincident_strip::FacePairIntersection;
use super::p2::IntersectorBrepP2;

const OP: &str = "boolean::topology_intersector::curved_strip::drop_strip_crossings";

/// Drops from each face pair that is not coincident every curve interval that runs over a
/// strip of one face within `tol` of the other face's surface, beside an edge of the first
/// face that the coincident pair of its neighbour and the other face imprints (module
/// docs). The interval is removed from both faces' classifications.
///
/// # Errors
///
/// Those of the curve and surface evaluations, the topology walks and the enclosure;
/// cancellation.
pub(super) fn drop_strip_crossings<O: ClassObject + SolutionObject + Ord>(
    k: &Kernel,
    brep: &mut impl IntersectorBrepP2<O>,
    intersections: &mut [FacePairIntersection<O>],
    tol: f64,
) -> Result<()> {
    let coincident: Vec<(O, O)> = intersections
        .iter()
        .filter(|x| x.2)
        .map(|x| (x.0, x.1))
        .collect();
    if coincident.is_empty() {
        return Ok(());
    }
    for (f1, f2, are_coincident, fi, _) in intersections.iter_mut() {
        if *are_coincident {
            continue;
        }
        k.check_cancelled(OP)?;
        let mut dropped: Vec<(usize, usize)> = Vec::new();
        for (ci, (c1, c2)) in fi.cc1.iter().zip(&fi.cc2).enumerate() {
            let Some(curve_h) = c1.curve() else {
                continue;
            };
            // the curve's UV curves on the two faces: the surface/surface curve lists
            // hold them beside the 3D curve
            let [cc_3d, cc_uv1, cc_uv2] = &fi.cc_curves;
            let at = cc_3d.iter().position(|&h| Some(h) == c1.curve());
            let uv_on = |list: &[O]| at.and_then(|j| list.get(j).copied());
            for ll in 0..c1.size().min(c2.size()) {
                let (i1, i2) = (c1.at(ll)?, c2.at(ll)?);
                // A is the face the curve runs inside; B the other
                let sides = [(*f2, *f1, i2, uv_on(cc_uv2)), (*f1, *f2, i1, uv_on(cc_uv1))];
                for (fa, fb, ia, uv) in sides {
                    if !shape_fits(ia) {
                        continue;
                    }
                    // (copies: the check may make trims on the B-rep that holds them)
                    let (Some(curve), Some(uv)) = (
                        ClassGeometry::curve(&*brep, curve_h).cloned(),
                        uv.and_then(|c| ClassGeometry::curve(&*brep, c)).cloned(),
                    ) else {
                        continue;
                    };
                    let strip = Strip {
                        fa,
                        fb,
                        interval: ia,
                        curve: &curve,
                        uv: &uv,
                        coincident: &coincident,
                        tol,
                    };
                    if strip.certified(k, brep)? {
                        dropped.push((ci, ll));
                        break;
                    }
                }
            }
        }
        for &(ci, ll) in dropped.iter().rev() {
            if let Some(c) = fi.cc1.get_mut(ci) {
                c.remove_at(ll)?;
            }
            if let Some(c) = fi.cc2.get_mut(ci) {
                c.remove_at(ll)?;
            }
        }
    }
    Ok(())
}

/// One candidate: the curve interval `interval` of `curve` as face `fa`'s classification
/// reads it, with the curve's UV curve `uv` on `fa`, against face `fb`.
struct Strip<'a, O> {
    fa: O,
    fb: O,
    interval: &'a CurveInterval<O>,
    curve: &'a Curve,
    uv: &'a Curve,
    coincident: &'a [(O, O)],
    tol: f64,
}

/// A curve piece in A's parameter plane: its hull points and their error.
impl<O: ClassObject + SolutionObject + Ord> Strip<'_, O> {
    /// Whether the strip the interval cuts off A beside an edge E is certified within
    /// the tolerance of B (module docs): the edges E beside it are found on the B-rep as
    /// it is, their trims and the far edge's on A made where missing (as a query of the
    /// face makes them), and each strip enclosed.
    fn certified(&self, k: &Kernel, brep: &mut impl IntersectorBrepP2<O>) -> Result<bool> {
        let Some(implicit) = brep
            .face_surface(self.fb)
            .flatten()
            .and_then(|s| ClassGeometry::surface(&*brep, s))
            .and_then(AnalyticSupport::of)
            .map(|s| s.implicit())
        else {
            return Ok(false);
        };
        // B is a plane or a cone (a cylinder included)
        if !matches!(implicit, Implicit::Plane { .. } | Implicit::Cone { .. }) {
            return Ok(false);
        }
        let candidates = self.candidates(k, &*brep)?;
        for cand in candidates {
            k.check_cancelled(OP)?;
            make_trim(k, brep, self.fa, cand.e)?;
            if let Far::Edge(n, _) = cand.far {
                make_trim(k, brep, self.fa, n)?;
            }
            let Some((b, EntityRef::Face(fa))) = ClassGeometry::topology_entity(&*brep, self.fa)
            else {
                return Ok(false);
            };
            let Some(surface) = b.face(fa)?.surface.as_ref() else {
                return Ok(false);
            };
            let Some((net, net_bound)) = net_of(surface) else {
                return Ok(false);
            };
            if self.strip_beside(k, b, fa, surface, &net, net_bound, &implicit, cand)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The edges E of A the strip may lie beside: E ends at the curve's end X (a vertex),
    /// its other face is coincident with B in this intersection, and the curve's other
    /// end is E's other end V or lies on an edge N of A at V.
    fn candidates(&self, k: &Kernel, brep: &impl IntersectorBrepP2<O>) -> Result<Vec<Candidate>> {
        let mut out = Vec::new();
        let Some((b, EntityRef::Face(fa))) = ClassGeometry::topology_entity(brep, self.fa) else {
            return Ok(out);
        };
        let ivl = self.interval;
        let t = ivl.interval;
        let ends = [
            (&ivl.start, t.min(), &ivl.end, t.max()),
            (&ivl.end, t.max(), &ivl.start, t.min()),
        ];
        for (x_class, tx, n_class, tn) in ends {
            if x_class.point_class() != PointClassType::Vertex {
                continue;
            }
            let Some(x) = x_class.object().and_then(|o| vertex_of(brep, b, o)) else {
                continue;
            };
            let far = match n_class.point_class() {
                PointClassType::Edge => match n_class.object().and_then(|o| edge_of(brep, b, o)) {
                    Some(n) => Far::Edge(n, n_class.t_param()),
                    None => continue,
                },
                PointClassType::Vertex => {
                    match n_class.object().and_then(|o| vertex_of(brep, b, o)) {
                        Some(v) => Far::Vertex(v),
                        None => continue,
                    }
                }
                _ => continue,
            };
            for e in vertex::edges(b, x, Some(fa))? {
                k.check_cancelled(OP)?;
                if let Some(v) = self.beside(brep, b, fa, e, x, far)? {
                    out.push(Candidate {
                        e,
                        v,
                        far,
                        t: [tx, tn],
                    });
                }
            }
        }
        Ok(out)
    }

    /// E's other end V when `e` may bound the strip ([`Strip::candidates`]).
    fn beside(
        &self,
        brep: &impl IntersectorBrepP2<O>,
        b: &Brep,
        fa: FaceId,
        e: EdgeId,
        x: VertexId,
        far: Far,
    ) -> Result<Option<VertexId>> {
        // E's neighbour across it is a face coincident with B in this intersection
        let faces = edge::faces(b, e, None)?;
        let Some(&neighbour) = faces.iter().find(|&&f| f != fa) else {
            return Ok(None);
        };
        if faces.iter().filter(|&&f| f == fa).count() != 1 {
            return Ok(None);
        }
        let neighbour_is_coincident = self.coincident.iter().any(|&(g1, g2)| {
            let other = if g1 == self.fb {
                g2
            } else if g2 == self.fb {
                g1
            } else {
                return false;
            };
            matches!(
                ClassGeometry::topology_entity(brep, other),
                Some((ob, EntityRef::Face(f))) if core::ptr::eq(ob, b) && f == neighbour
            )
        });
        if !neighbour_is_coincident {
            return Ok(None);
        }
        let Some(v) = edge::other_vertex(b, e, Some(x))? else {
            return Ok(None);
        };
        if v == x {
            return Ok(None);
        }
        let reaches_v = match far {
            Far::Vertex(w) => w == v,
            Far::Edge(n, _) => {
                n != e
                    && (edge::start_vertex(b, n)? == Some(v) || edge::end_vertex(b, n)? == Some(v))
            }
        };
        Ok(reaches_v.then_some(v))
    }

    /// Whether the strip beside the candidate's edge E is certified: the boxes of the
    /// pieces of the curve's UV curve, E's trim and N's trim between V and the curve's
    /// end, and of the gaps where they meet, and the net's bound over each slab of them
    /// (module docs).
    #[allow(clippy::too_many_arguments)]
    fn strip_beside(
        &self,
        k: &Kernel,
        b: &Brep,
        fa: FaceId,
        surface: &Surface,
        net: &BSplineSurface,
        net_bound: f64,
        implicit: &Implicit,
        cand: Candidate,
    ) -> Result<bool> {
        let Candidate {
            e,
            v,
            far,
            t: [tx, tn],
        } = cand;
        let e_ivl = b.edge(e)?.interval;

        // the curve's UV curve over its interval, checked at both ends
        for t in [tx, tn] {
            let p = self.curve.evaluate_point(k, t)?;
            if !uv_meets(k, surface, self.uv, t, p, self.tol)? {
                return Ok(false);
            }
        }
        let Some(c_boxes) = piece_boxes(self.uv, Extent1d::from_min_max(tx.min(tn), tx.max(tn)))
        else {
            return Ok(false);
        };
        // (the box at the curve's end at X, and at its far end)
        let (c_at_x, c_far) = ends(&c_boxes, tx <= tn);

        // E's trim on A over the whole edge, checked at both ends
        let Some(e_trim) = trim_on(b, e, fa)? else {
            return Ok(false);
        };
        for t in [e_ivl.min(), e_ivl.max()] {
            if !trim_meets(k, b, e, surface, e_trim, t, self.tol)? {
                return Ok(false);
            }
        }
        let Some(e_boxes) = piece_boxes(e_trim, e_ivl) else {
            return Ok(false);
        };
        // (the box at E's end at V, and at its end at X)
        let (e_at_v, e_at_x) = ends(&e_boxes, edge::start_vertex(b, e)? == Some(v));

        let mut boxes: Vec<Extent2d> = Vec::new();
        // the gap where the curve meets E at X
        boxes.push(union(c_at_x, e_at_x));
        // the piece of N's trim between V and the curve's far end
        match far {
            Far::Vertex(_) => boxes.push(union(c_far, e_at_v)),
            Far::Edge(n, tc) => {
                let rec = b.edge(n)?;
                let (Some(n_curve), n_ivl) = (rec.curve.as_ref(), rec.interval) else {
                    return Ok(false);
                };
                let Some(n_trim) = trim_on(b, n, fa)? else {
                    return Ok(false);
                };
                // V's parameter on N: the end of N's interval at V
                let vp = vertex::point(b, v)?;
                let (a0, a1) = (
                    n_curve.evaluate_point(k, n_ivl.min())?,
                    n_curve.evaluate_point(k, n_ivl.max())?,
                );
                let tv = if a0.distance(vp) <= a1.distance(vp) {
                    n_ivl.min()
                } else {
                    n_ivl.max()
                };
                for t in [tv, tc] {
                    if !trim_meets(k, b, n, surface, n_trim, t, self.tol)? {
                        return Ok(false);
                    }
                }
                if !(n_ivl.min() <= tc && tc <= n_ivl.max()) {
                    return Ok(false);
                }
                let Some(n_boxes) =
                    piece_boxes(n_trim, Extent1d::from_min_max(tv.min(tc), tv.max(tc)))
                else {
                    return Ok(false);
                };
                let (n_at_v, n_far) = ends(&n_boxes, tv <= tc);
                // the gaps where N's piece meets E at V and the curve at its far end
                boxes.push(union(n_at_v, e_at_v));
                boxes.push(union(n_far, c_far));
                boxes.extend(n_boxes);
            }
        }
        boxes.extend(c_boxes);
        boxes.extend(e_boxes);

        // the strip over each slab of the boxes' span in u, enclosed
        let mut worst = 0.0_f64;
        for slab in slabs(&boxes) {
            let Some(slab) = slab else {
                return Ok(false);
            };
            let Some(bound) = net_bound_over(net, &slab, implicit)? else {
                return Ok(false);
            };
            worst = worst.max(bound);
            // (not a number is not within)
            let within = (worst + net_bound) * (1.0 + 4.0 * f64::EPSILON) <= self.tol;
            if !within {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// The number of equal parameter pieces each boundary piece's hull is taken over, and of
/// equal slabs in u the strip is enclosed over.
const PIECES: u32 = 32;

/// The boxes of the hulls of `uv` over [`PIECES`] equal pieces of `ivl` in parameter
/// order, each widened by its points' errors: every point of `uv` over `ivl` lies in one.
/// `None` when a hull cannot be taken or is not finite.
fn piece_boxes(uv: &Curve, ivl: Extent1d) -> Option<Vec<Extent2d>> {
    let (lo, hi) = (ivl.min(), ivl.max());
    if !(lo <= hi && lo.is_finite() && hi.is_finite()) {
        return None;
    }
    let mut boxes = Vec::new();
    let mut a = lo;
    for i in 1..=PIECES {
        // (the last piece ends at `hi` exactly: the pieces cover the interval)
        let b = if i == PIECES {
            hi
        } else {
            lo + (hi - lo) * (f64::from(i) / f64::from(PIECES))
        };
        let b = b.max(a);
        let mut piece = Extent2d::new();
        for (q, err) in uv.piece_hull_points(&Extent1d::from_min_max(a, b))? {
            if !(q.x.is_finite() && q.y.is_finite() && err.is_finite()) {
                return None;
            }
            piece.add_point(Point2::new(q.x - err, q.y - err));
            piece.add_point(Point2::new(q.x + err, q.y + err));
        }
        let held = piece.min().x <= piece.max().x;
        if !held {
            return None;
        }
        boxes.push(piece);
        a = b;
    }
    Some(boxes)
}

/// The boxes at a piece's two ends: (at its parameter's start, at its end) when
/// `start_first`, else swapped.
fn ends(boxes: &[Extent2d], start_first: bool) -> (Extent2d, Extent2d) {
    let first = boxes.first().copied().unwrap_or_default();
    let last = boxes.last().copied().unwrap_or_default();
    if start_first {
        (first, last)
    } else {
        (last, first)
    }
}

/// The box of two boxes: it holds every segment between their points (where two pieces'
/// ends meet across a gap).
fn union(a: Extent2d, b: Extent2d) -> Extent2d {
    let mut u = a;
    u.add_point(b.min());
    u.add_point(b.max());
    u
}

/// The strip cut into [`PIECES`] equal slabs of the boxes' span in u, each the slab times
/// the span in v of the boxes meeting it (`None` for a slab no box meets). A vertical
/// line through the strip meets it between points of its boundary on that line, and each
/// such point lies in a box meeting the slab, so the strip over a slab lies in its box.
fn slabs(boxes: &[Extent2d]) -> Vec<Option<Extent2d>> {
    let mut span = Extent2d::new();
    for bx in boxes {
        span.add_point(bx.min());
        span.add_point(bx.max());
    }
    let (u0, u1) = (span.min().x, span.max().x);
    let mut out = Vec::new();
    let mut a = u0;
    for i in 1..=PIECES {
        let b = if i == PIECES {
            u1
        } else {
            u0 + (u1 - u0) * (f64::from(i) / f64::from(PIECES))
        };
        let b = b.max(a);
        let mut v: Option<(f64, f64)> = None;
        for bx in boxes {
            if bx.min().x <= b && a <= bx.max().x {
                let (lo, hi) = v.unwrap_or_else(|| (bx.min().y, bx.max().y));
                v = Some((lo.min(bx.min().y), hi.max(bx.max().y)));
            }
        }
        out.push(v.map(|(lo, hi)| Extent2d::from_bounds(a, lo, b, hi)));
        a = b;
    }
    out
}

/// An edge E the strip may lie beside: E, its end V away from the curve's end at E,
/// the curve's far end, and the curve's parameters at its two ends (at E's end first).
#[derive(Clone, Copy)]
struct Candidate {
    e: EdgeId,
    v: VertexId,
    far: Far,
    t: [f64; 2],
}

/// Where the curve ends away from E: on A's boundary edge N at N's parameter, or at a
/// vertex.
#[derive(Clone, Copy)]
enum Far {
    Edge(EdgeId, f64),
    Vertex(VertexId),
}

/// A's net and its certified distance from A's surface: a B-spline surface is its own
/// net (0), a procedural blend its net with the net's bound; `None` for every other kind.
fn net_of(surface: &Surface) -> Option<(BSplineSurface, f64)> {
    match surface.kind() {
        SurfaceKind::BSpline(b) => Some((b.clone(), 0.0)),
        SurfaceKind::ProceduralBlend(pb) => {
            let bound = pb.bound();
            (bound >= 0.0 && bound.is_finite()).then(|| (pb.net().clone(), bound))
        }
        _ => None,
    }
}

/// The bound of [`off_surface_bound`] over the net's sub-patch covering `box_uv` cut to
/// the net's domain: the sub-patch is extracted over the box widened by the extraction's
/// snapping to knots and must cover the box, else `None`.
fn net_bound_over(
    net: &BSplineSurface,
    box_uv: &Extent2d,
    implicit: &Implicit,
) -> Result<Option<f64>> {
    let natural = net.natural_uv_domain();
    let (n_lo, n_hi) = (natural.min(), natural.max());
    // (A's points are its surface's over its domain: the box is cut to the domain, the
    // hulls' errors may reach past it)
    let (lo, hi) = (box_uv.min(), box_uv.max());
    let (lo, hi) = (
        Point2::new(lo.x.max(n_lo.x), lo.y.max(n_lo.y)),
        Point2::new(hi.x.min(n_hi.x), hi.y.min(n_hi.y)),
    );
    if !(lo.x <= hi.x && lo.y <= hi.y) {
        return Ok(None);
    }
    // (`copy_sub_patch` snaps a bound to a knot within 1e-8: widen past it, and check)
    let widen = |a: f64| 2.0e-8 * (1.0 + a.abs());
    let wide = Extent2d::from_bounds(
        lo.x - widen(lo.x),
        lo.y - widen(lo.y),
        hi.x + widen(hi.x),
        hi.y + widen(hi.y),
    );
    let Ok(sub) = net.copy_sub_patch(&wide) else {
        return Ok(None);
    };
    let got = sub.natural_uv_domain();
    let (g_lo, g_hi) = (got.min(), got.max());
    if !(g_lo.x <= lo.x && g_lo.y <= lo.y && hi.x <= g_hi.x && hi.y <= g_hi.y) {
        return Ok(None);
    }
    off_surface_bound(&sub, implicit)
}

/// Whether edge `e`'s UV trim `trim` on `surface` meets the edge at parameter `t`: at an
/// end of the edge's interval it meets the end's vertex within `tol` plus the vertex's
/// tolerance (the edge's curve and its trims each end within that of the vertex), inside
/// the interval the edge's curve within `tol` plus the edge's tolerance.
///
/// # Errors
///
/// Those of the topology walk and of the evaluations.
fn trim_meets(
    k: &Kernel,
    b: &Brep,
    e: EdgeId,
    surface: &Surface,
    trim: &Curve,
    t: f64,
    tol: f64,
) -> Result<bool> {
    let rec = b.edge(e)?;
    let (Some(curve), ivl, edge_tol) = (rec.curve.as_ref(), rec.interval, rec.tolerance) else {
        return Ok(false);
    };
    let end = if t == ivl.min() {
        edge::start_vertex(b, e)?
    } else if t == ivl.max() {
        edge::end_vertex(b, e)?
    } else {
        None
    };
    match end {
        Some(v) => uv_meets(
            k,
            surface,
            trim,
            t,
            vertex::point(b, v)?,
            tol + vertex::tolerance(b, v)?,
        ),
        None => uv_meets(
            k,
            surface,
            trim,
            t,
            curve.evaluate_point(k, t)?,
            tol + edge_tol,
        ),
    }
}

/// Whether the surface's point at `uv_curve(t)` lies within `reach` of `p`.
fn uv_meets(
    k: &Kernel,
    surface: &Surface,
    uv_curve: &Curve,
    t: f64,
    p: Point3,
    reach: f64,
) -> Result<bool> {
    let q = uv_curve.evaluate_point(k, t)?;
    let s = surface.evaluate_point(k, Point2::new(q.x, q.y))?;
    Ok(s.distance(p) <= reach)
}

/// Whether a curve interval can be a strip's curve on its face: it runs inside the face
/// and one of its ends is a vertex.
fn shape_fits<O: ClassObject>(ivl: &CurveInterval<O>) -> bool {
    ivl.mid.point_class() == PointClassType::Face
        && (ivl.start.point_class() == PointClassType::Vertex
            || ivl.end.point_class() == PointClassType::Vertex)
}

/// Makes the UV trim curve of edge `e`'s single edge use on face `fa_h` where it has none
/// (as a query of the face makes it: `GetOrCreateUVTrimCurve`). A trim that cannot be
/// made is left missing ([`trim_on`] then finds none).
///
/// # Errors
///
/// Those of the topology walk and of making the trim.
fn make_trim<O: ClassObject>(
    k: &Kernel,
    brep: &mut impl IntersectorBrepP2<O>,
    fa_h: O,
    e: EdgeId,
) -> Result<()> {
    let Some((b, EntityRef::Face(fa))) = brep.topology_entity_for_query(fa_h) else {
        return Ok(());
    };
    let Some(eu) = single_edgeuse_on(b, e, fa)? else {
        return Ok(());
    };
    if eu1::uv_trim_curve(b, eu)?.0.is_some() {
        return Ok(());
    }
    // (a failed status leaves no trim: the strip is then not certified)
    let _fetch = eu3::get_or_create_uv_trim_curve(k, b, eu, false, false, true)?;
    Ok(())
}

/// An edge use of edge `e` on face `fa`, where the edge meets the face once: its uses
/// on the face are one use and its mate (the face's two sides). `None` for none, or for
/// a use besides those (a seam, the face on both sides of the edge).
fn single_edgeuse_on(b: &Brep, e: EdgeId, fa: FaceId) -> Result<Option<EdgeuseId>> {
    let mut found: Option<(EdgeuseId, Option<EdgeuseId>)> = None;
    for eu in edge::edgeuses(b, e, None, None)? {
        if eu1::face(b, eu)? != Some(fa) {
            continue;
        }
        match found {
            None => found = Some((eu, eu1::mate(b, eu)?)),
            Some((first, mate)) => {
                if eu != first && Some(eu) != mate {
                    return Ok(None);
                }
            }
        }
    }
    Ok(found.map(|(eu, _)| eu))
}

/// The UV trim curve on face `fa` of edge `e`'s single edge use there.
fn trim_on(b: &Brep, e: EdgeId, fa: FaceId) -> Result<Option<&Curve>> {
    match single_edgeuse_on(b, e, fa)? {
        // (its own or its mate's: both on the face)
        Some(eu) => Ok(eu1::uv_trim_curve(b, eu)?.0),
        None => Ok(None),
    }
}

/// The vertex a classification object of A's B-rep names (a vertex or its use).
fn vertex_of<O: ClassObject>(brep: &impl ClassGeometry<O>, b: &Brep, o: O) -> Option<VertexId> {
    match ClassGeometry::topology_entity(brep, o)? {
        (ob, EntityRef::Vertex(v)) if core::ptr::eq(ob, b) => Some(v),
        (ob, EntityRef::Vertexuse(vu)) if core::ptr::eq(ob, b) => vertexuse::vertex(b, vu).ok()?,
        _ => None,
    }
}

/// The edge a classification object of A's B-rep names (an edge or its use).
fn edge_of<O: ClassObject>(brep: &impl ClassGeometry<O>, b: &Brep, o: O) -> Option<EdgeId> {
    match ClassGeometry::topology_entity(brep, o)? {
        (ob, EntityRef::Edge(e)) if core::ptr::eq(ob, b) => Some(e),
        (ob, EntityRef::Edgeuse(eu)) if core::ptr::eq(ob, b) => eu1::edge(b, eu).ok()?,
        _ => None,
    }
}
