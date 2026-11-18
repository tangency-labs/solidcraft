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
//! The decision is a certificate over A's strip, not a sample. A's surface `S` is its net
//! (a B-spline face's own surface, or a blend's or sweep's certified net) plus the net's
//! certified distance from its definition, `ε` (0 for a plain B-spline face). The strip is
//! the region of A's parameter plane bounded by E's trim on A, C's UV curve on A over the
//! curve's interval, and the piece of N's trim on A between V and C's end. E's trim and
//! N's are A's own boundary (a face is the region its trims bound), so that region is A's
//! strip beside E whatever the edges' 3D curves are. Two of the pieces are also tied to
//! their 3D curves at the same parameters by [`curve_uv_gap_bound`] (`sup |C(t) −
//! S(W(t))|` enclosed over the whole piece in rounded interval arithmetic, plus `ε`):
//! C's UV curve within the tolerance of C (PROVEN: the curve dropped is the one whose UV
//! image bounds the strip), and N's trim over the piece within the tolerance plus N's
//! tolerance of N's curve (PROVEN: the piece the event check below reads in N's 3D
//! parameters is the strip's side). E needs no such tie and has none: its trim is checked
//! only at its ends against E's vertices (MEASURED on the #18166 join: E's trim lies a
//! certified 3.77e-5 from E's curve, over E's 1e-5 tolerance, a defect of that B-rep the
//! certificate does not rest on). E's trim is assumed to share E's parameter interval:
//! it is cut over that interval, and its two ends are the ones checked against E's
//! vertices, so a trim parametrised otherwise is refused in practice by those checks (the
//! same assumption as for N's trim over N's piece). Each piece is cut into equal
//! parameter pieces, and each of those lies in the box of its certified hull points (`Curve::piece_hull_points`, their coordinates' errors added); where two
//! pieces meet, the segment closing the gap between their ends lies in the box of their
//! end boxes. So the closed boundary lies in the union of those boxes. The span of the
//! boxes in u is cut into equal slabs: a vertical line through the strip meets it between
//! points of its boundary on that line, each in a box meeting the slab, so the strip over
//! a slab lies in the slab times the span in v of the boxes meeting it. Every box must lie
//! in the net's domain (within the extraction's snapping), so no piece of the boundary
//! runs past a seam. A's surface over that box is the restriction of its net: the net's
//! sub-patch over the box (extracted by knot insertion and checked to cover it) is
//! enclosed against B's implicit equation by `implicit::off_surface_bound` (the Bernstein
//! coefficients of the equation, their rounding and the extraction's counted, turned into
//! a distance by the cone's least radius over the patch). When the largest of those bounds
//! plus `ε` is within the tolerance, every point of the strip is within the tolerance of
//! B's surface (PROVEN for the surface, given that the strip is what the three pieces
//! bound).
//!
//! The region is the strip's, not its complement (PROVEN by the checks that refuse
//! otherwise): V's end of N is found by vertex identity, and an N that starts and ends at
//! V is refused ([`end_at`]); the two UV points where two pieces meet (one 3D point) must
//! be less than half the net's domain apart in each direction, so a junction across a
//! periodic seam (where they are a period apart) is refused ([`junction`]); and every box
//! lies in the net's domain. In the parameter plane the three pieces and the junction
//! segments are then one closed curve in the domain, which bounds one region.
//!
//! The drop removes C's end P on N as an event. It is refused while any other interval of
//! any face pair in the intersection ends on N inside the certified piece of N, or at N's
//! end away from V when the piece reaches it ([`event_in_piece`]): such an event would be
//! left on N with nothing from A to relate it to (the class of failure this rule fixes).
//!
//! What is ARGUED, not proven: that the strip lies over B's face, not only its surface: E
//! and C both lie on B's face (E imprinted by the coincident pair, C classified inside
//! it).
//!
//! A true crossing is not dropped: where A crosses B along C (or leaves it past the
//! tolerance before C), A's points between E and C reach farther from B than the
//! tolerance, the bound exceeds it, and the curve stays (the unit test
//! `a_strip_off_b_keeps_its_curve`).
//!
//! Nothing is made on the B-rep while deciding: the trims used must already exist (the
//! curve's classification against A makes them), so a refused candidate leaves the
//! B-rep as upstream's path finds it.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`drop_strip_crossings`] (super) | none (own design) |
//!
//! Deviations: not upstream at all; always on. B is a plane or a cone (a cylinder
//! included), A a B-spline face (with or without a kept definition) or a procedural
//! blend; every other pair is intersected as upstream does. No recursion.

use crate::curve::curve::p2::Curve;
use crate::error::Result;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::solution_array::SolutionObject;
use crate::intersect::curve_class::p1::CurveInterval;
use crate::intersect::point_class::{
    ClassGeometry, ClassObject, PointClassType, PointClassification,
};
use crate::kernel::Kernel;
use crate::math::{Point2, Point3};
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::query::curve_surface_gap::curve_uv_gap_bound;
use crate::surface::implicit::{off_surface_bound, AnalyticSupport, Implicit};
use crate::surface::surface::p2::SurfaceKind;
use crate::surface::surface::Surface;
use crate::topology::edgeuse::p1 as eu1;
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
/// Returns the number of intervals dropped.
///
/// # Errors
///
/// Those of the curve and surface evaluations, the topology walks and the enclosure;
/// cancellation.
pub(super) fn drop_strip_crossings<O: ClassObject + SolutionObject + Ord>(
    k: &Kernel,
    brep: &impl IntersectorBrepP2<O>,
    intersections: &mut [FacePairIntersection<O>],
    tol: f64,
) -> Result<usize> {
    let coincident: Vec<(O, O)> = intersections
        .iter()
        .filter(|x| x.2)
        .map(|x| (x.0, x.1))
        .collect();
    if coincident.is_empty() {
        return Ok(0);
    }
    // (decided over the intersection as it stands, then removed: the event check reads
    // every pair)
    let mut dropped: Vec<[usize; 3]> = Vec::new();
    for (pi, (f1, f2, are_coincident, fi, _)) in intersections.iter().enumerate() {
        if *are_coincident {
            continue;
        }
        k.check_cancelled(OP)?;
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
                    let (Some(curve), Some(uv)) = (
                        ClassGeometry::curve(brep, curve_h),
                        uv.and_then(|c| ClassGeometry::curve(brep, c)),
                    ) else {
                        continue;
                    };
                    let strip = Strip {
                        fa,
                        fb,
                        interval: ia,
                        curve,
                        uv,
                        coincident: &coincident,
                        all: intersections,
                        own: [pi, ci, ll],
                        tol,
                    };
                    if strip.certified(k, brep)? {
                        dropped.push([pi, ci, ll]);
                        break;
                    }
                }
            }
        }
    }
    for &[pi, ci, ll] in dropped.iter().rev() {
        let Some((_, _, _, fi, _)) = intersections.get_mut(pi) else {
            continue;
        };
        if let Some(c) = fi.cc1.get_mut(ci) {
            c.remove_at(ll)?;
        }
        if let Some(c) = fi.cc2.get_mut(ci) {
            c.remove_at(ll)?;
        }
    }
    Ok(dropped.len())
}

/// Test-only seam ([`crate::kernel::StripProbe`]): makes each run the kernel's probe asks
/// for on a copy of the operation's real intersection list `intersections` at the drop's
/// call, and records the number of intervals each drops.
///
/// # Errors
///
/// Those of [`drop_strip_crossings`].
#[cfg(test)]
pub(super) fn probe<O: ClassObject + SolutionObject + Ord>(
    k: &Kernel,
    brep: &impl IntersectorBrepP2<O>,
    intersections: &[FacePairIntersection<O>],
) -> Result<()> {
    let runs = match k.strip_probe.lock() {
        Ok(mut p) => core::mem::take(&mut p.runs),
        Err(_) => return Ok(()),
    };
    for run in runs {
        if let Ok(mut p) = k.strip_probe.lock() {
            p.active = Some(run);
        }
        let mut copy = intersections.to_vec();
        let got = drop_strip_crossings(k, brep, &mut copy, run.tol);
        if let Ok(mut p) = k.strip_probe.lock() {
            p.active = None;
            if let Ok(n) = got {
                p.dropped.push(n);
            }
        }
        got?;
    }
    Ok(())
}

/// The active probe run's perturbations ([`crate::kernel::StripProbeRun`]): what is added
/// to the strip's bound, the curve's gap and N's piece's gap, and whether a synthetic
/// event lies on N's piece. None outside the crate's unit tests.
#[cfg(test)]
fn perturbed(k: &Kernel) -> ([f64; 3], bool) {
    k.strip_probe
        .lock()
        .ok()
        .and_then(|p| p.active)
        .map_or(([0.0; 3], false), |r| (r.extra, r.event_on_n))
}

/// See the test build's version: no perturbation.
#[cfg(not(test))]
fn perturbed(_k: &Kernel) -> ([f64; 3], bool) {
    ([0.0; 3], false)
}

/// One candidate: the curve interval `interval` of `curve` as face `fa`'s classification
/// reads it, with the curve's UV curve `uv` on `fa`, against face `fb`; `all` the whole
/// intersection and `own` the interval's place in it (pair, curve, interval).
struct Strip<'a, O> {
    fa: O,
    fb: O,
    interval: &'a CurveInterval<O>,
    curve: &'a Curve,
    uv: &'a Curve,
    coincident: &'a [(O, O)],
    all: &'a [FacePairIntersection<O>],
    own: [usize; 3],
    tol: f64,
}

/// Face A: its B-rep and id, its surface, the surface's net and the net's certified
/// distance from the surface `ε`.
struct FaceA<'b> {
    b: &'b Brep,
    fa: FaceId,
    surface: &'b Surface,
    net: BSplineSurface,
    net_bound: f64,
}

impl<O: ClassObject + SolutionObject + Ord> Strip<'_, O> {
    /// Whether the strip the interval cuts off A beside an edge E is certified within
    /// the tolerance of B (module docs): the edges E beside it are found on the B-rep as
    /// it is and each strip enclosed. Nothing is made on the B-rep.
    fn certified(&self, k: &Kernel, brep: &impl IntersectorBrepP2<O>) -> Result<bool> {
        let Some(implicit) = brep
            .face_surface(self.fb)
            .flatten()
            .and_then(|s| ClassGeometry::surface(brep, s))
            .and_then(AnalyticSupport::of)
            .map(|s| s.implicit())
        else {
            return Ok(false);
        };
        // B is a plane or a cone (a cylinder included)
        if !matches!(implicit, Implicit::Plane { .. } | Implicit::Cone { .. }) {
            return Ok(false);
        }
        let Some((b, EntityRef::Face(fa))) = ClassGeometry::topology_entity(brep, self.fa) else {
            return Ok(false);
        };
        let Some(surface) = b.face(fa)?.surface.as_ref() else {
            return Ok(false);
        };
        let Some((net, net_bound)) = net_of(surface) else {
            return Ok(false);
        };
        let a = FaceA {
            b,
            fa,
            surface,
            net,
            net_bound,
        };
        for cand in self.candidates(k, brep, &a)? {
            k.check_cancelled(OP)?;
            if self.strip_beside(k, brep, &a, &implicit, cand)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The edges E of A the strip may lie beside: E ends at the curve's end X (a vertex),
    /// its other face is coincident with B in this intersection, and the curve's other
    /// end is E's other end V or lies on an edge N of A at V.
    fn candidates(
        &self,
        k: &Kernel,
        brep: &impl IntersectorBrepP2<O>,
        a: &FaceA<'_>,
    ) -> Result<Vec<Candidate>> {
        let mut out = Vec::new();
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
            let Some(x) = x_class.object().and_then(|o| vertex_of(brep, a.b, o)) else {
                continue;
            };
            let far = match n_class.point_class() {
                PointClassType::Edge => {
                    match n_class.object().and_then(|o| edge_of(brep, a.b, o)) {
                        Some(n) => Far::Edge(n, n_class.t_param()),
                        None => continue,
                    }
                }
                PointClassType::Vertex => {
                    match n_class.object().and_then(|o| vertex_of(brep, a.b, o)) {
                        Some(v) => Far::Vertex(v),
                        None => continue,
                    }
                }
                _ => continue,
            };
            for e in vertex::edges(a.b, x, Some(a.fa))? {
                k.check_cancelled(OP)?;
                if let Some(v) = self.beside(brep, a, e, x, far)? {
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
        a: &FaceA<'_>,
        e: EdgeId,
        x: VertexId,
        far: Far,
    ) -> Result<Option<VertexId>> {
        let b = a.b;
        // E's neighbour across it is a face coincident with B in this intersection
        let faces = edge::faces(b, e, None)?;
        let Some(&neighbour) = faces.iter().find(|&&f| f != a.fa) else {
            return Ok(None);
        };
        if faces.iter().filter(|&&f| f == a.fa).count() != 1 {
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

    /// Whether the strip beside the candidate's edge E is certified (module docs): the
    /// boxes of the pieces of the curve's UV curve, E's trim and N's trim between V and
    /// the curve's end, and of the junctions where they meet; no other event on N's
    /// piece; the net's bound over each slab of the boxes; and each piece's certified gap
    /// to its 3D curve.
    fn strip_beside(
        &self,
        k: &Kernel,
        brep: &impl IntersectorBrepP2<O>,
        a: &FaceA<'_>,
        implicit: &Implicit,
        cand: Candidate,
    ) -> Result<bool> {
        let Candidate {
            e,
            v,
            far,
            t: [tx, tn],
        } = cand;
        let b = a.b;
        let domain = a.net.natural_uv_domain();
        let c_range = [tx.min(tn), tx.max(tn)];

        // the curve's UV curve over its interval (its ends checked first: a cheap filter,
        // the gap below is the certificate)
        for t in [tx, tn] {
            let p = self.curve.evaluate_point(k, t)?;
            if !uv_meets(k, a.surface, self.uv, t, p, self.tol)? {
                return Ok(false);
            }
        }
        let Some(c_boxes) = piece_boxes(self.uv, Extent1d::from_min_max(c_range[0], c_range[1]))
        else {
            return Ok(false);
        };
        // (the box at the curve's end at X, and at its far end)
        let (c_at_x, c_far) = ends(&c_boxes, tx <= tn);
        let c_x_uv = uv_at(k, self.uv, tx)?;
        let c_far_uv = uv_at(k, self.uv, tn)?;

        // E's trim on A over the whole edge (A's own boundary there), checked at its ends
        // against E's vertices
        let e_ivl = b.edge(e)?.interval;
        let Some(e_trim) = trim_on(b, e, a.fa)? else {
            return Ok(false);
        };
        for t in [e_ivl.min(), e_ivl.max()] {
            if !trim_meets(k, b, e, a.surface, e_trim, t, self.tol)? {
                return Ok(false);
            }
        }
        let Some(e_boxes) = piece_boxes(e_trim, e_ivl) else {
            return Ok(false);
        };
        // (V's end of E by identity: E is not closed, `beside` refuses V = X)
        let Some(e_v_at_start) = end_at(edge::start_vertex(b, e)?, edge::end_vertex(b, e)?, v)
        else {
            return Ok(false);
        };
        let (e_at_v, e_at_x) = ends(&e_boxes, e_v_at_start);
        let (e_v_param, e_x_param) = if e_v_at_start {
            (e_ivl.min(), e_ivl.max())
        } else {
            (e_ivl.max(), e_ivl.min())
        };
        let e_v_uv = uv_at(k, e_trim, e_v_param)?;
        let e_x_uv = uv_at(k, e_trim, e_x_param)?;

        let mut boxes: Vec<Extent2d> = Vec::new();
        // the junction where the curve meets E at X
        let Some(jx) = junction(c_at_x, e_at_x, c_x_uv, e_x_uv, &domain) else {
            return Ok(false);
        };
        boxes.push(jx);
        // the piece of N's trim between V and the curve's far end
        let mut n_piece: Option<(&Curve, &Curve, [f64; 2], f64)> = None;
        match far {
            Far::Vertex(_) => {
                let Some(jv) = junction(c_far, e_at_v, c_far_uv, e_v_uv, &domain) else {
                    return Ok(false);
                };
                boxes.push(jv);
            }
            Far::Edge(n, tc) => {
                let rec = b.edge(n)?;
                let (Some(n_curve), n_ivl, n_tol) =
                    (rec.curve.as_ref(), rec.interval, rec.tolerance)
                else {
                    return Ok(false);
                };
                let Some(n_trim) = trim_on(b, n, a.fa)? else {
                    return Ok(false);
                };
                // V's parameter on N, by identity: an N that starts and ends at V (closed)
                // has two, and either piece may be the strip's: refused
                let Some(v_at_start) =
                    end_at(edge::start_vertex(b, n)?, edge::end_vertex(b, n)?, v)
                else {
                    return Ok(false);
                };
                let (tv, tw) = if v_at_start {
                    (n_ivl.min(), n_ivl.max())
                } else {
                    (n_ivl.max(), n_ivl.min())
                };
                if !(n_ivl.min() <= tc && tc <= n_ivl.max()) {
                    return Ok(false);
                }
                for t in [tv, tc] {
                    if !trim_meets(k, b, n, a.surface, n_trim, t, self.tol)? {
                        return Ok(false);
                    }
                }
                let piece = Extent1d::from_min_max(tv.min(tc), tv.max(tc));
                // no other event on N's piece (module docs)
                let w = if v_at_start {
                    edge::end_vertex(b, n)?
                } else {
                    edge::start_vertex(b, n)?
                };
                if self.other_event_on(k, brep, b, n, v, w, tw, piece)? {
                    return Ok(false);
                }
                let Some(n_boxes) = piece_boxes(n_trim, piece) else {
                    return Ok(false);
                };
                let (n_at_v, n_far) = ends(&n_boxes, tv <= tc);
                // the junctions where N's piece meets E at V and the curve at its far end
                let Some(jv) = junction(n_at_v, e_at_v, uv_at(k, n_trim, tv)?, e_v_uv, &domain)
                else {
                    return Ok(false);
                };
                let Some(jc) = junction(n_far, c_far, uv_at(k, n_trim, tc)?, c_far_uv, &domain)
                else {
                    return Ok(false);
                };
                boxes.push(jv);
                boxes.push(jc);
                boxes.extend(n_boxes);
                n_piece = Some((n_curve, n_trim, [piece.min(), piece.max()], n_tol));
            }
        }
        boxes.extend(c_boxes);
        boxes.extend(e_boxes);

        // the strip over each slab of the boxes' span in u, enclosed
        // (`extra` is the test seam's perturbation, 0 outside the unit tests)
        let (extra, _) = perturbed(k);
        if !strip_within(&a.net, a.net_bound + extra[0], &boxes, implicit, self.tol)? {
            return Ok(false);
        }

        // the dropped curve and N's piece are their 3D curves' images on S at the same
        // parameters (module docs): the curve within the tolerance, N's piece within it
        // plus N's tolerance
        let floor = 0.01 * self.tol;
        if !gap_within(
            k,
            a,
            self.curve,
            self.uv,
            c_range,
            self.tol - extra[1],
            floor,
        )? {
            return Ok(false);
        }
        if let Some((n_curve, n_trim, range, n_tol)) = n_piece {
            if !gap_within(
                k,
                a,
                n_curve,
                n_trim,
                range,
                self.tol + n_tol - extra[2],
                floor,
            )? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Whether any interval of the intersection other than this one ends on edge `n` of
    /// A's B-rep `b` inside `piece`, or at `n`'s end `w` away from `v` (at parameter `tw`)
    /// when `piece` reaches it ([`event_in_piece`]).
    #[allow(clippy::too_many_arguments)]
    fn other_event_on(
        &self,
        k: &Kernel,
        brep: &impl IntersectorBrepP2<O>,
        b: &Brep,
        n: EdgeId,
        v: VertexId,
        w: Option<VertexId>,
        tw: f64,
        piece: Extent1d,
    ) -> Result<bool> {
        for (pi, (_, _, _, fi, _)) in self.all.iter().enumerate() {
            for (ci, (c1, c2)) in fi.cc1.iter().zip(&fi.cc2).enumerate() {
                for cc in [c1, c2] {
                    for ll in 0..cc.size() {
                        if [pi, ci, ll] == self.own {
                            continue;
                        }
                        let ivl = cc.at(ll)?;
                        for end in [&ivl.start, &ivl.end] {
                            let at = end_on(brep, b, end);
                            if event_in_piece(at, n, v, w, tw, piece) {
                                return Ok(true);
                            }
                        }
                    }
                }
            }
        }
        // (the test seam's synthetic end on N, at its piece's middle: none otherwise)
        let synthetic = EndOn::Edge(n, 0.5 * (piece.min() + piece.max()));
        Ok(perturbed(k).1 && event_in_piece(synthetic, n, v, w, tw, piece))
    }
}

/// Where an interval's end lies on A's B-rep: on an edge at its parameter, at a vertex,
/// or elsewhere (another B-rep, a face, nothing).
#[derive(Clone, Copy, Debug)]
enum EndOn {
    Edge(EdgeId, f64),
    Vertex(VertexId),
    Elsewhere,
}

/// [`EndOn`] of a point classification read on A's B-rep `b`.
fn end_on<O: ClassObject>(
    brep: &impl ClassGeometry<O>,
    b: &Brep,
    end: &PointClassification<O>,
) -> EndOn {
    match end.point_class() {
        PointClassType::Edge => match end.object().and_then(|o| edge_of(brep, b, o)) {
            Some(e) => EndOn::Edge(e, end.t_param()),
            None => EndOn::Elsewhere,
        },
        PointClassType::Vertex => match end.object().and_then(|o| vertex_of(brep, b, o)) {
            Some(x) => EndOn::Vertex(x),
            None => EndOn::Elsewhere,
        },
        _ => EndOn::Elsewhere,
    }
}

/// Whether an interval end `at` is an event on N's certified `piece` other than at V: on
/// edge `n` at a parameter inside `piece` (its ends included; a parameter that is not a
/// number counts as inside), or at N's end `w` away from V (parameter `tw`) when `piece`
/// reaches `tw`. An end at V itself is E's end, kept as an event by the drop.
fn event_in_piece(
    at: EndOn,
    n: EdgeId,
    v: VertexId,
    w: Option<VertexId>,
    tw: f64,
    piece: Extent1d,
) -> bool {
    let inside = |t: f64| !(t < piece.min() || t > piece.max());
    match at {
        EndOn::Edge(e, t) => e == n && inside(t),
        EndOn::Vertex(x) => x != v && Some(x) == w && inside(tw),
        EndOn::Elsewhere => false,
    }
}

/// Which end of an edge running from `start` to `end` is the vertex `v`: `Some(true)` at
/// its start, `Some(false)` at its end; `None` when neither is, or both are (a closed
/// edge, whose two pieces either may be the strip's).
fn end_at(start: Option<VertexId>, end: Option<VertexId>, v: VertexId) -> Option<bool> {
    match (start == Some(v), end == Some(v)) {
        (true, false) => Some(true),
        (false, true) => Some(false),
        _ => None,
    }
}

/// The box closing the junction of two pieces whose ends' boxes are `a` and `b` and whose
/// UV points there are `pa` and `pb` (one 3D point): the box of both boxes, which holds
/// the segment between the points. `None` when the points are half the net's `domain` or
/// more apart in u or in v, or are not numbers: across a periodic seam they are a period
/// apart, and the segment is not the strip's boundary.
fn junction(
    a: Extent2d,
    b: Extent2d,
    pa: Point2,
    pb: Point2,
    domain: &Extent2d,
) -> Option<Extent2d> {
    let (lo, hi) = (domain.min(), domain.max());
    let near =
        (pa.x - pb.x).abs() < 0.5 * (hi.x - lo.x) && (pa.y - pb.y).abs() < 0.5 * (hi.y - lo.y);
    near.then(|| union(a, b))
}

/// The UV point of a UV curve or trim at `t`.
fn uv_at(k: &Kernel, uv: &Curve, t: f64) -> Result<Point2> {
    let q = uv.evaluate_point(k, t)?;
    Ok(Point2::new(q.x, q.y))
}

/// Whether `bound` plus the net's `net_bound`, grown by its roundings, is within `reach`
/// (not a number is not within).
fn within(bound: f64, net_bound: f64, reach: f64) -> bool {
    (bound + net_bound) * (1.0 + 4.0 * f64::EPSILON) <= reach
}

/// Whether the 3D curve `curve` lies within `reach` of A's surface at its UV curve `uv`'s
/// points over `range`: [`curve_uv_gap_bound`] against A's net, plus the net's bound.
/// Either curve not a B-spline (or a kind derived from one), or a gap not enclosed, is
/// not within.
fn gap_within(
    k: &Kernel,
    a: &FaceA<'_>,
    curve: &Curve,
    uv: &Curve,
    range: [f64; 2],
    reach: f64,
    floor: f64,
) -> Result<bool> {
    let (Some(c), Some(w)) = (curve.b_spline_base(), uv.b_spline_base()) else {
        return Ok(false);
    };
    let Some(gap) = curve_uv_gap_bound(k, c, w, a.surface, range, floor)? else {
        return Ok(false);
    };
    Ok(within(gap, a.net_bound, reach))
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

/// Whether A's net plus its bound `net_bound` lies within `tol` of B's surface
/// (`implicit`) over the strip whose boundary lies in `boxes` (module docs): every slab
/// of [`slabs`] meets a box, lies in the net's domain, and its sub-patch's
/// [`off_surface_bound`] plus `net_bound`, grown by its roundings, is within `tol`.
fn strip_within(
    net: &BSplineSurface,
    net_bound: f64,
    boxes: &[Extent2d],
    implicit: &Implicit,
    tol: f64,
) -> Result<bool> {
    let mut worst = 0.0_f64;
    for slab in slabs(boxes) {
        let Some(slab) = slab else {
            return Ok(false);
        };
        let Some(bound) = net_bound_over(net, &slab, implicit)? else {
            return Ok(false);
        };
        worst = worst.max(bound);
        if !within(worst, net_bound, tol) {
            return Ok(false);
        }
    }
    Ok(true)
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

/// A's net and its certified distance `ε` from A's surface: a B-spline surface is its own
/// net, `ε` the bound of the definition it is kept as the net of (a round's, a corner's, a
/// draft's, a section blend's or a sweep's) or 0 for a plain one; a procedural blend its
/// net with the net's bound. `None` for every other kind, and for a bound that is not a
/// finite number at least 0.
fn net_of(surface: &Surface) -> Option<(BSplineSurface, f64)> {
    let net = match surface.kind() {
        SurfaceKind::BSpline(b) => b,
        SurfaceKind::ProceduralBlend(pb) => pb.net(),
        _ => return None,
    };
    let bound = match surface.procedural_definition() {
        Some(definition) => definition.bound(),
        None if matches!(surface.kind(), SurfaceKind::BSpline(_)) => 0.0,
        // (a procedural blend always has its definition)
        None => return None,
    };
    (bound >= 0.0 && bound.is_finite()).then(|| (net.clone(), bound))
}

/// The bound of [`off_surface_bound`] over the net's sub-patch covering `box_uv`: the box
/// must lie in the net's domain within the extraction's snapping (a box past it would
/// hold points of no face, or of the surface across a seam), and the sub-patch is
/// extracted over the box widened by that snapping and must cover the box; else `None`.
fn net_bound_over(
    net: &BSplineSurface,
    box_uv: &Extent2d,
    implicit: &Implicit,
) -> Result<Option<f64>> {
    let natural = net.natural_uv_domain();
    let (n_lo, n_hi) = (natural.min(), natural.max());
    // (`copy_sub_patch` snaps a bound to a knot within 1e-8: widen past it, and check)
    let widen = |a: f64| 2.0e-8 * (1.0 + a.abs());
    let (lo, hi) = (box_uv.min(), box_uv.max());
    let inside = n_lo.x - widen(n_lo.x) <= lo.x
        && n_lo.y - widen(n_lo.y) <= lo.y
        && hi.x <= n_hi.x + widen(n_hi.x)
        && hi.y <= n_hi.y + widen(n_hi.y);
    if !inside {
        return Ok(None);
    }
    // (A's points are its surface's over its domain: the box is cut to the domain, the
    // hulls' errors may reach past it)
    let (lo, hi) = (
        Point2::new(lo.x.max(n_lo.x), lo.y.max(n_lo.y)),
        Point2::new(hi.x.min(n_hi.x), hi.y.min(n_hi.y)),
    );
    if !(lo.x <= hi.x && lo.y <= hi.y) {
        return Ok(None);
    }
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
    off_surface_bound(&sub, implicit, net)
}

/// Whether edge `e`'s UV trim `trim` on `surface` meets the edge at parameter `t`: at an
/// end of the edge's interval it meets the end's vertex within `tol` plus the vertex's
/// tolerance (the edge's curve and its trims each end within that of the vertex), inside
/// the interval the edge's curve within `tol` plus the edge's tolerance. A cheap filter
/// before the certified gap ([`gap_within`]).
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

/// The UV trim curve on face `fa` of edge `e`'s single edge use there, as the B-rep holds
/// it (none is made: a missing trim refuses the strip).
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

#[cfg(test)]
mod tests {
    use super::*;

    const N: EdgeId = EdgeId::from_index(7);
    const V: VertexId = VertexId::from_index(1);
    const W: VertexId = VertexId::from_index(2);

    /// V's end of an edge is found by identity; a closed edge (both ends V) and an edge
    /// not at V are refused, whatever their points' distances.
    #[test]
    fn the_end_at_v_is_found_by_identity_and_a_closed_edge_is_refused() {
        assert_eq!(end_at(Some(V), Some(W), V), Some(true));
        assert_eq!(end_at(Some(W), Some(V), V), Some(false));
        assert_eq!(end_at(Some(V), Some(V), V), None);
        assert_eq!(end_at(Some(W), Some(W), V), None);
        assert_eq!(end_at(None, None, V), None);
    }

    /// A junction of two UV points of one 3D point: near points (a ten-thousandth of the
    /// domain apart) close it with the box of both end boxes; points across a periodic
    /// seam (u 0.001 and 0.999 of a domain 0..1: a period less 0.002 apart), or not
    /// numbers, are refused.
    #[test]
    fn a_junction_across_a_seam_is_refused() {
        let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let a = Extent2d::from_bounds(0.40, 0.10, 0.45, 0.20);
        let b = Extent2d::from_bounds(0.45, 0.20, 0.50, 0.30);
        let got = junction(
            a,
            b,
            Point2::new(0.45, 0.2),
            Point2::new(0.4501, 0.2),
            &domain,
        )
        .unwrap();
        assert_eq!(got.min(), Point2::new(0.40, 0.10));
        assert_eq!(got.max(), Point2::new(0.50, 0.30));
        let seam_u = junction(
            a,
            b,
            Point2::new(0.001, 0.2),
            Point2::new(0.999, 0.2),
            &domain,
        );
        assert!(seam_u.is_none());
        let seam_v = junction(a, b, Point2::new(0.4, 0.0), Point2::new(0.4, 1.0), &domain);
        assert!(seam_v.is_none());
        let nan = junction(
            a,
            b,
            Point2::new(f64::NAN, 0.2),
            Point2::new(0.4, 0.2),
            &domain,
        );
        assert!(nan.is_none());
    }

    /// An interval end on N inside the certified piece (its ends included), or at N's far
    /// end when the piece reaches it, is an event that refuses the drop; an end on N past
    /// the piece, at V, at N's far end the piece does not reach, on another edge or
    /// elsewhere is not; a parameter that is not a number refuses.
    #[test]
    fn an_event_on_the_piece_of_n_refuses_the_drop() {
        let piece = Extent1d::from_min_max(0.0, 0.25);
        let other = EdgeId::from_index(8);
        let at = |end| event_in_piece(end, N, V, Some(W), 1.0, piece);
        assert!(at(EndOn::Edge(N, 0.1)));
        assert!(at(EndOn::Edge(N, 0.0)));
        assert!(at(EndOn::Edge(N, 0.25)));
        assert!(at(EndOn::Edge(N, f64::NAN)));
        assert!(!at(EndOn::Edge(N, 0.5)));
        assert!(!at(EndOn::Edge(other, 0.1)));
        assert!(!at(EndOn::Vertex(V)));
        assert!(!at(EndOn::Vertex(W)));
        assert!(!at(EndOn::Elsewhere));
        // (the piece reaching N's far end W: an end at W is on it)
        let whole = Extent1d::from_min_max(0.0, 1.0);
        assert!(event_in_piece(EndOn::Vertex(W), N, V, Some(W), 1.0, whole));
    }

    /// The bilinear patch z = 1e-3 x (x, y in 0..1) against the plane z = 0, over the
    /// strip x in 0..0.5 (its boundary's boxes): the strip reaches 5e-4 from the plane, so
    /// it is within 1e-3 and the curve beside it is dropped, but a strip off B by more
    /// than the tolerance keeps its curve: held to 4e-4, or within 1e-3 with a net bound of
    /// 6e-4 added, it is not within.
    #[test]
    fn a_strip_off_b_keeps_its_curve() {
        use crate::nurbs::b_spline_curve::KnotType;
        use crate::nurbs::b_spline_surface::{BSplineSurfaceForm, CanonicalSurfaceData};
        let net = BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 1,
            v_degree: 1,
            control_points: &[
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
                Point3::new(1.0, 0.0, 1e-3),
                Point3::new(1.0, 1.0, 1e-3),
            ],
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &[2, 2],
            v_multiplicities: &[2, 2],
            u_knots: &[0.0, 1.0],
            v_knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: Some(&[1.0; 4]),
            uv_domain: None,
        })
        .unwrap();
        let plane = Implicit::Plane {
            origin: crate::math::Vec3::ZERO,
            normal: crate::math::Vec3::Z,
        };
        // (the strip's boundary: its sides x = 0 and x = 0.5, and its ends y = 0 and 1)
        let boxes = [
            Extent2d::from_bounds(0.0, 0.0, 0.0, 1.0),
            Extent2d::from_bounds(0.5, 0.0, 0.5, 1.0),
            Extent2d::from_bounds(0.0, 0.0, 0.5, 0.0),
            Extent2d::from_bounds(0.0, 1.0, 0.5, 1.0),
        ];
        assert!(strip_within(&net, 0.0, &boxes, &plane, 1e-3).unwrap());
        assert!(!strip_within(&net, 0.0, &boxes, &plane, 4e-4).unwrap());
        assert!(!strip_within(&net, 6e-4, &boxes, &plane, 1e-3).unwrap());
        // (not a number is not within)
        assert!(!strip_within(&net, f64::NAN, &boxes, &plane, 1e-3).unwrap());
    }

    /// [`gap_within`] of a line 4.2e-7 above the plane patch z = 0 at the UV line under it
    /// (the same parameters): within a reach of 5e-6, not within 1e-7 (the curve is not
    /// the image of its UV curve there), nor within 5e-6 once a net bound of 5e-6 is added.
    #[test]
    fn a_curve_off_its_uv_image_is_not_tied() {
        use crate::nurbs::b_spline_curve::{BSplineCurve, KnotType};
        use crate::nurbs::b_spline_surface::{BSplineSurfaceForm, CanonicalSurfaceData};
        use crate::nurbs::crv_basic::Curve as NurbCurve;
        use crate::nurbs::math::{CPoint, NOW};
        let k = Kernel::default();
        let net = BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 1,
            v_degree: 1,
            control_points: &[
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
            ],
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &[2, 2],
            v_multiplicities: &[2, 2],
            u_knots: &[0.0, 1.0],
            v_knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: Some(&[1.0; 4]),
            uv_domain: None,
        })
        .unwrap();
        let surface = Surface::from(net.clone());
        let line = |z: f64| {
            let c = NurbCurve::new(
                vec![CPoint::new(0.2, 0.3, z, NOW), CPoint::new(0.7, 0.6, z, NOW)],
                1,
                vec![0.0, 0.0, 1.0, 1.0],
            )
            .unwrap();
            Curve::from(BSplineCurve::from_nurb(c, 3, false))
        };
        let (curve, uv) = (line(4.2e-7), line(0.0));
        let b = Brep::new();
        let face = |net_bound| FaceA {
            b: &b,
            fa: FaceId::from_index(0),
            surface: &surface,
            net: net.clone(),
            net_bound,
        };
        let tied =
            |a: &FaceA<'_>, reach| gap_within(&k, a, &curve, &uv, [0.0, 1.0], reach, 1e-9).unwrap();
        assert!(tied(&face(0.0), 5e-6));
        assert!(!tied(&face(0.0), 1e-7));
        assert!(!tied(&face(5e-6), 5e-6));
    }

    /// A box past the net's domain by more than the extraction's snapping (a boundary
    /// piece running across a seam, or off the face) has no bound.
    #[test]
    fn a_box_past_the_net_has_no_bound() {
        use crate::nurbs::b_spline_curve::KnotType;
        use crate::nurbs::b_spline_surface::{BSplineSurfaceForm, CanonicalSurfaceData};
        let net = BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 1,
            v_degree: 1,
            control_points: &[
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
            ],
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &[2, 2],
            v_multiplicities: &[2, 2],
            u_knots: &[0.0, 1.0],
            v_knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: Some(&[1.0; 4]),
            uv_domain: None,
        })
        .unwrap();
        let plane = Implicit::Plane {
            origin: crate::math::Vec3::ZERO,
            normal: crate::math::Vec3::Z,
        };
        let inside = Extent2d::from_bounds(0.25, 0.25, 0.75, 0.75);
        let on = net_bound_over(&net, &inside, &plane).unwrap().unwrap();
        assert!((0.0..=1e-13).contains(&on), "{on:e}");
        let past = Extent2d::from_bounds(0.25, 0.25, 1.01, 0.75);
        assert_eq!(net_bound_over(&net, &past, &plane).unwrap(), None);
    }

    /// The fixture of `tests/wr_614_coincident_strip_crossing.rs`
    /// (`curved_strip_join_union_is_one_valid_solid`): the body and the kept part, read
    /// with every fix on.
    fn curved_strip_join(k: &mut Kernel) -> (Brep, Brep) {
        use crate::geom::attribute::AttributeStore;
        use crate::topology::api_files::read_brep_from_file;
        use crate::topology::brep::p10::TopologyEnv;
        use crate::topology::topology::NotifyRecords;
        let mut load = |bytes: &[u8]| {
            let fresh = Brep::new();
            let mut store = AttributeStore::new();
            let mut records = NotifyRecords::new();
            let key_of = |e| fresh.owner_identity(e);
            let mut env = TopologyEnv {
                all_attributes: &[],
                attributes: &mut store,
                key_of: &key_of,
                records: &mut records,
                next_key: 1 << 50,
            };
            let mut b =
                read_brep_from_file(k, Some(bytes.to_vec()), false, false, &mut env).unwrap();
            b.refresh_cached_topology();
            b
        };
        let body = load(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/data/curved_strip_join_body.smb"
        )));
        let kept = load(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/data/curved_strip_join_kept.smb"
        )));
        (body, kept)
    }

    /// The drop decided on the fixture's real intersection list (the kernel's test seam
    /// runs [`drop_strip_crossings`] on a copy of it where the union's topology intersector
    /// calls it): the dropped curve C goes at the intersector's tolerance 5e-6 (one
    /// interval), and stays (none) when any one check refuses:
    /// - at 1e-7, below C's certified gap (MEASURED 4.22e-7);
    /// - at 1e-8, below the net's bound `ε` (MEASURED 1.21e-8);
    /// - at 5e-6 with 1 added to the strip's enclosure ([`strip_within`]), to C's gap or
    ///   to N's piece's gap ([`gap_within`]), or with a synthetic interval end on N at the
    ///   middle of its piece ([`Strip::other_event_on`]).
    ///
    /// The union itself goes on with the real drop at the real tolerance.
    #[test]
    fn the_strip_drop_keeps_the_curve_when_a_check_refuses() {
        use crate::boolean::api_brep::boolean_union;
        use crate::kernel::{StripProbeRun, UpstreamFixes};
        let mut k = Kernel {
            fixes: UpstreamFixes::all(),
            ..Kernel::default()
        };
        let (body, kept) = curved_strip_join(&mut k);
        let run = |tol: f64, extra: [f64; 3], event_on_n: bool| StripProbeRun {
            tol,
            extra,
            event_on_n,
        };
        let runs = vec![
            run(5e-6, [0.0; 3], false),
            run(1e-7, [0.0; 3], false),
            run(1e-8, [0.0; 3], false),
            run(5e-6, [1.0, 0.0, 0.0], false),
            run(5e-6, [0.0, 1.0, 0.0], false),
            run(5e-6, [0.0, 0.0, 1.0], false),
            run(5e-6, [0.0; 3], true),
        ];
        k.strip_probe.lock().unwrap().runs = runs;
        boolean_union(&mut k, body, kept).unwrap();
        let dropped = core::mem::take(&mut k.strip_probe.lock().unwrap().dropped);
        assert_eq!(dropped, [1, 0, 0, 0, 0, 0, 0]);
    }

    /// [`net_of`]'s bound is the bound of the definition a B-spline surface keeps (a screw
    /// sweep's wall, the net of its sweep), as it is a procedural blend's (the fixture's
    /// blend faces, A among them: MEASURED 1.21e-8), and 0 for a plain B-spline surface.
    #[test]
    fn a_kept_definition_gives_the_net_its_bound() {
        use crate::nurbs::b_spline_curve::KnotType;
        use crate::nurbs::b_spline_surface::{BSplineSurfaceForm, CanonicalSurfaceData};
        use crate::topology::brep::p1::faces;
        let mut k = Kernel::default();
        let profile: Vec<Curve> = [(4.1, 0.0), (5.0, 0.0), (5.0, 0.9), (4.1, 0.9)]
            .iter()
            .zip([(5.0, 0.0), (5.0, 0.9), (4.1, 0.9), (4.1, 0.0)].iter())
            .map(|(&(ar, az), &(br, bz))| {
                Curve::from(crate::curve::line::Line::from_points(
                    Point3::new(ar, 0.0, az),
                    Point3::new(br, 0.0, bz),
                    3,
                ))
            })
            .collect();
        let screw = crate::sweep::screw::screw_sweep(
            &mut k,
            &profile,
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 1.0),
            1.0,
            1.0,
            1e-4,
        )
        .unwrap();
        let (body, kept) = curved_strip_join(&mut k);
        let mut checked = [0, 0];
        for b in [&screw, &body, &kept] {
            for f in faces(b, None).unwrap() {
                let Some(surface) = b.face(f).unwrap().surface.as_ref() else {
                    continue;
                };
                let Some(definition) = surface.procedural_definition() else {
                    continue;
                };
                let (_, bound) = net_of(surface).unwrap();
                assert!(bound > 0.0, "{bound:e}");
                assert_eq!(bound.to_bits(), definition.bound().to_bits());
                match surface.kind() {
                    SurfaceKind::BSpline(_) => checked[0] += 1,
                    SurfaceKind::ProceduralBlend(_) => checked[1] += 1,
                    other => panic!("{other:?}"),
                }
            }
        }
        assert!(checked[0] > 0 && checked[1] > 0, "{checked:?}");
        let plain = BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 1,
            v_degree: 1,
            control_points: &[
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
            ],
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &[2, 2],
            v_multiplicities: &[2, 2],
            u_knots: &[0.0, 1.0],
            v_knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: Some(&[1.0; 4]),
            uv_domain: None,
        })
        .unwrap();
        let (_, bound) = net_of(&Surface::from(plain)).unwrap();
        assert_eq!(bound, 0.0);
    }
}
