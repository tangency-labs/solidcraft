//! Contact of non-parallel opposing constant-width straight chamfer bands on a
//! planar support.
//!
//! Upstream: not in SMLib. On a shared planar face, two straight constant-width
//! bands with opposing inward normals (`n_a · n_b < 0`) meet along their
//! weighted bisector line `d_a(X) / w_a = d_b(X) / w_b = s(X)`. Its normal
//! `w_b n_a - w_a n_b` has squared length strictly above `w_a^2 + w_b^2`, so
//! the construction is unconditionally well-conditioned all the way down to
//! arbitrarily small angles between the edges.
//!
//! The scale `s(u)` is affine along the bisector. Intersecting with each edge's
//! end boundaries (a planar end face's line on the shared plane for an
//! unblended planar cap, or the edge's interior perpendicular half-plane past
//! its endpoint tolerance otherwise), subtracting closed selected corner strips,
//! and splitting by exact crossings with the shared face's boundary edges yields
//! the leftmost on-face bisector point whose scale decides both overlap and the
//! largest fitting size.
//!
//! Deviations: no station sampling, no parallel-angle tolerance, and no retry
//! builds. Unsupported selected corners name no contact, matching `family`.

use super::{inward, Band, BandContact, CornerBand, OP};
use crate::blend::fillet_executive::FitKind;
use crate::curve::curve::{Curve, CurveKind};
use crate::error::blend::BandInput;
use crate::error::{Result, SolidError};
use crate::intersect::api_intersectors::curve_crossings;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, EFF_ZERO};
use crate::primitive::api_curves::create_line_segment;
use crate::surface::surface::SurfaceKind;
use crate::topology::{edge, face, vertex, Brep, EdgeId, FaceId, VertexId};

/// A 1D interval on the bisector parameter `u`, tracking whether each finite
/// endpoint is closed (certified planar cap) or open (interior bound or
/// subtracted corner strip).
#[derive(Clone, Copy, Debug)]
struct Interval {
    lo: f64,
    hi: f64,
    lo_closed: bool,
    hi_closed: bool,
}

impl Interval {
    fn all() -> Self {
        Self {
            lo: f64::NEG_INFINITY,
            hi: f64::INFINITY,
            lo_closed: false,
            hi_closed: false,
        }
    }

    fn is_empty(&self) -> bool {
        if self.lo.is_nan() || self.hi.is_nan() {
            return true;
        }
        self.lo > self.hi || (self.lo == self.hi && !(self.lo_closed && self.hi_closed))
    }

    /// Intersect with `c0 + c1 * u >= 0` (if `closed`) or `> 0` (otherwise).
    fn clip_linear(mut self, c0: f64, c1: f64, closed: bool) -> Option<Self> {
        if !(c0.is_finite() && c1.is_finite()) {
            return None;
        }
        if c1 == 0.0 {
            let ok = if closed { c0 >= 0.0 } else { c0 > 0.0 };
            return ok.then_some(self);
        }
        let bound = -c0 / c1;
        if !bound.is_finite() {
            return None;
        }
        if c1 > 0.0 {
            if bound > self.lo {
                self.lo = bound;
                self.lo_closed = closed;
            } else if bound == self.lo {
                self.lo_closed = self.lo_closed && closed;
            }
        } else if bound < self.hi {
            self.hi = bound;
            self.hi_closed = closed;
        } else if bound == self.hi {
            self.hi_closed = self.hi_closed && closed;
        }
        (!self.is_empty()).then_some(self)
    }

    /// Subtract a closed interval `[cut_lo, cut_hi]` from `self`, yielding 0, 1,
    /// or 2 surviving pieces with open boundaries where the cut occurred.
    fn subtract_closed(self, cut_lo: f64, cut_hi: f64, out: &mut Vec<Self>) {
        if cut_lo > cut_hi || cut_hi < self.lo || cut_lo > self.hi {
            out.push(self);
            return;
        }
        if cut_lo == self.hi && !self.hi_closed {
            out.push(self);
            return;
        }
        if cut_hi == self.lo && !self.lo_closed {
            out.push(self);
            return;
        }
        if cut_lo > self.lo {
            let left = Self {
                lo: self.lo,
                hi: cut_lo,
                lo_closed: self.lo_closed,
                hi_closed: false,
            };
            if !left.is_empty() {
                out.push(left);
            }
        }
        if cut_hi < self.hi {
            let right = Self {
                lo: cut_hi,
                hi: self.hi,
                lo_closed: false,
                hi_closed: self.hi_closed,
            };
            if !right.is_empty() {
                out.push(right);
            }
        }
    }
}

/// One end boundary of a straight chamfer band on the shared planar face.
#[derive(Clone, Copy)]
pub(super) struct EndBoundary {
    origin: Point3,
    /// Unit vector in the shared plane pointing into the band's side of the cap.
    inward: Vec3,
    /// Open exclusion distance from `origin` along `inward` (`0.0` for a closed
    /// planar cap; the edge tolerance for an uncertified open end).
    offset: f64,
    edge_tol: f64,
    closed: bool,
    /// For a convex planar cap whose edge runs into the band, its unit direction
    /// from `origin` and finite length, so a crossing past that cap's end is
    /// excluded as an open bound rather than treated as a closed cap contact.
    convex_cap: Option<(Vec3, f64)>,
}

/// Precomputed opposing-bisector geometry for one straight planar rail, built
/// once per `(solver, face)` so non-parallel candidate pruning runs in `O(1)`
/// stack arithmetic.
#[derive(Clone, Copy)]
pub(super) struct PreparedOpposing {
    pub(super) face: FaceId,
    pub(super) plane_normal: Vec3,
    pub(super) p0: Point3,
    pub(super) p1: Point3,
    pub(super) inward: Vec3,
    pub(super) width: f64,
    pub(super) boundaries: [EndBoundary; 2],
}

/// Constant inward unit normal of a straight band in the shared plane, taking a
/// fast path for planar other supports so candidate filtering needs no drops.
fn straight_inward(k: &Kernel, band: &Band, mid: Point3, tangent: Vec3) -> Result<Option<Vec3>> {
    let raw = match band.other.kind() {
        SurfaceKind::Plane(plane) => {
            let normal = plane.evaluate_normal() * band.inward_sign;
            let projected = normal - band.plane_normal * normal.dot(band.plane_normal);
            if projected.length() <= EFF_ZERO {
                return Ok(None);
            }
            projected.unitize()?
        }
        _ => inward(k, band, mid)?,
    };
    if raw.dot(tangent).abs() > EFF_ZERO {
        return Ok(None);
    }
    let perp = band.plane_normal.cross(tangent);
    if perp.length() <= EFF_ZERO {
        return Ok(None);
    }
    let unit = perp.unitize()?;
    Ok(Some(if unit.dot(raw) >= 0.0 { unit } else { -unit }))
}

fn faces_share_edge_at_vertex(
    target: &Brep,
    vertex_id: VertexId,
    face_a: FaceId,
    face_b: FaceId,
) -> Result<bool> {
    for e in vertex::edges(target, vertex_id, None)? {
        let faces = edge::faces(target, e, None)?;
        if faces.contains(&face_a) && faces.contains(&face_b) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Classify one endpoint of a straight band: either a certified planar end face
/// meeting the shared face along a transverse straight edge, or an open interior
/// boundary excluding the endpoint's tolerance neighborhood.
#[allow(clippy::too_many_arguments)]
fn end_boundary(
    k: &Kernel,
    target: &Brep,
    band: &Band,
    vertex_id: Option<VertexId>,
    end_pt: Point3,
    other_pt: Point3,
    band_inward: Vec3,
    blended: &[EdgeId],
) -> Result<EndBoundary> {
    let edge_tol = edge::tolerance(target, band.edge)?;
    let from_end = (other_pt - end_pt).unitize()?;
    let open = EndBoundary {
        origin: end_pt,
        inward: from_end,
        offset: edge_tol,
        edge_tol,
        closed: false,
        convex_cap: None,
    };
    let Some(v) = vertex_id else {
        return Ok(open);
    };
    // Any selected adjacent edge at this vertex places a moving corner rather
    // than a fixed end cap.
    let incident_all = vertex::edges(target, v, None)?;
    if incident_all
        .iter()
        .any(|&e| e != band.edge && blended.contains(&e))
    {
        return Ok(open);
    }
    let on_face: Vec<EdgeId> = vertex::edges(target, v, Some(band.face))?
        .into_iter()
        .filter(|&e| e != band.edge)
        .collect();
    let [cap_edge] = on_face.as_slice() else {
        return Ok(open);
    };
    let Some(curve) = edge::evaluation_curve(target, *cap_edge)? else {
        return Ok(open);
    };
    if !matches!(curve.kind(), CurveKind::Line(_)) {
        return Ok(open);
    }
    let cap_ivl = edge::interval(target, *cap_edge)?;
    let p0 = curve.evaluate_point(k, cap_ivl.min())?;
    let p1 = curve.evaluate_point(k, cap_ivl.max())?;
    let (cap_near, cap_far) = if p0.distance(end_pt) <= p1.distance(end_pt) {
        (p0, p1)
    } else {
        (p1, p0)
    };
    let cap_vec = cap_far - cap_near;
    let cap_len = cap_vec.length();
    if !(cap_len.is_finite() && cap_len > EFF_ZERO) {
        return Ok(open);
    }
    let cap_dir = cap_vec / cap_len;
    let other_faces: Vec<FaceId> = edge::faces(target, *cap_edge, None)?
        .into_iter()
        .filter(|&f| f != band.face)
        .collect();
    let [end_face] = other_faces.as_slice() else {
        return Ok(open);
    };
    let Some(end_surface) = face::p2::surface(target, *end_face)? else {
        return Ok(open);
    };
    if !matches!(end_surface.kind(), SurfaceKind::Plane(_)) {
        return Ok(open);
    }
    if let Some(band_other_face) = band.other_face {
        if !faces_share_edge_at_vertex(target, v, band_other_face, *end_face)? {
            return Ok(open);
        }
    }
    let perp = band.plane_normal.cross(cap_dir);
    if perp.length() <= EFF_ZERO {
        return Ok(open);
    }
    let mut cap_inward = perp.unitize()?;
    let align = from_end.dot(cap_inward);
    if align.abs() <= EFF_ZERO {
        return Ok(open);
    }
    if align < 0.0 {
        cap_inward = -cap_inward;
    }
    let convex_cap = (cap_dir.dot(band_inward) > 0.0).then_some((cap_dir, cap_len));
    Ok(EndBoundary {
        origin: end_pt,
        inward: cap_inward,
        offset: 0.0,
        edge_tol,
        closed: true,
        convex_cap,
    })
}

/// Prepare the opposing-bisector geometry of a straight planar band once per
/// `(solver, face)` rail. Returns `None` if the edge direction tilts out of the
/// shared plane or lacks a constant inward normal (leaving exact-parallel pairs
/// unaffected in `planar_bands::prepare_straight_band`).
pub(super) fn prepare(
    k: &Kernel,
    target: &Brep,
    band: &Band,
    blended: &[EdgeId],
) -> Result<Option<PreparedOpposing>> {
    if !matches!(band.curve.kind(), CurveKind::Line(_)) {
        return Ok(None);
    }
    let p0 = band.curve.evaluate_point(k, band.interval.min())?;
    let p1 = band.curve.evaluate_point(k, band.interval.max())?;
    let span = p1 - p0;
    let length = span.length();
    if !(length.is_finite() && length > EFF_ZERO) {
        return Ok(None);
    }
    let tangent = span / length;
    if tangent.dot(band.plane_normal).abs() > EFF_ZERO {
        return Ok(None);
    }
    let mid = p0 + span * 0.5;
    let Some(band_inward) = straight_inward(k, band, mid, tangent)? else {
        return Ok(None);
    };
    let boundaries = [
        end_boundary(
            k,
            target,
            band,
            edge::start_vertex(target, band.edge)?,
            p0,
            p1,
            band_inward,
            blended,
        )?,
        end_boundary(
            k,
            target,
            band,
            edge::end_vertex(target, band.edge)?,
            p1,
            p0,
            band_inward,
            blended,
        )?,
    ];
    Ok(Some(PreparedOpposing {
        face: band.face,
        plane_normal: band.plane_normal,
        p0,
        p1,
        inward: band_inward,
        width: band.width,
        boundaries,
    }))
}

struct OpposingBisector {
    a0: Point3,
    a1: Point3,
    b0: Point3,
    b1: Point3,
    na: Vec3,
    nb: Vec3,
    x0: Point3,
    u_dir: Vec3,
    s0: f64,
    s_slope: f64,
    domain: Interval,
}

impl OpposingBisector {
    fn from_prepared(a: &PreparedOpposing, b: &PreparedOpposing) -> Option<Self> {
        if a.face != b.face {
            return None;
        }
        let na = a.inward;
        let nb = b.inward;
        let cos = na.dot(nb);
        if !cos.is_finite() || cos >= 0.0 {
            return None;
        }
        let m = na * b.width - nb * a.width;
        let m_len_sq = m.dot(m);
        let m_len = m_len_sq.sqrt();
        if !(m_len_sq.is_finite() && m_len.is_finite() && m_len > 0.0) {
            return None;
        }
        let mut u_dir = a.plane_normal.cross(m) / m_len;
        let mut s_slope = a.plane_normal.dot(na.cross(nb)) / m_len;
        if !(u_dir.x.is_finite()
            && u_dir.y.is_finite()
            && u_dir.z.is_finite()
            && s_slope.is_finite())
        {
            return None;
        }
        if s_slope < 0.0 {
            u_dir = -u_dir;
            s_slope = -s_slope;
        }
        let mid_a = a.p0 + (a.p1 - a.p0) * 0.5;
        let mid_b = b.p0 + (b.p1 - b.p0) * 0.5;
        let seed = mid_a + (mid_b - mid_a) * 0.5;
        let mut x0 = seed - a.plane_normal * (seed - a.p0).dot(a.plane_normal);
        for _ in 0..2 {
            let residual = b.width * (x0 - a.p0).dot(na) - a.width * (x0 - b.p0).dot(nb);
            x0 -= m * (residual / m_len_sq);
        }
        let da0 = (x0 - a.p0).dot(na);
        let db0 = (x0 - b.p0).dot(nb);
        let s0 = 0.5 * (da0 / a.width + db0 / b.width);
        if !s0.is_finite() {
            return None;
        }
        let [a_start, a_end] = a.boundaries;
        let [b_start, b_end] = b.boundaries;
        let boundaries = [a_start, a_end, b_start, b_end];
        let mut domain = Interval::all().clip_linear(s0, s_slope, false)?;
        for bound in &boundaries {
            let c0 = (x0 - bound.origin).dot(bound.inward) - bound.offset;
            let c1 = u_dir.dot(bound.inward);
            // If the bisector crosses a finite convex cap line past the cap
            // edge's far end, exclude only that crossing point (open bound) so
            // valid interior bisector points farther from the cap remain kept.
            let mut closed = bound.closed;
            if closed && c1 != 0.0 {
                if let Some((cap_dir, cap_len)) = bound.convex_cap {
                    let u_cap = -c0 / c1;
                    if u_cap.is_finite() {
                        let p_cap = x0 + u_dir * u_cap;
                        if (p_cap - bound.origin).dot(cap_dir) > cap_len + bound.edge_tol {
                            closed = false;
                        }
                    }
                }
            }
            domain = domain.clip_linear(c0, c1, closed)?;
        }
        if !(domain.lo.is_finite() && domain.hi.is_finite()) {
            return None;
        }
        Some(Self {
            a0: a.p0,
            a1: a.p1,
            b0: b.p0,
            b1: b.p1,
            na,
            nb,
            x0,
            u_dir,
            s0,
            s_slope,
            domain,
        })
    }

    fn point(&self, u: f64) -> Point3 {
        self.x0 + self.u_dir * u
    }

    fn scale_at(&self, u: f64) -> f64 {
        self.s0 + self.s_slope * u
    }
}

/// Pure `O(1)` stack-only necessary condition for two prepared non-parallel
/// straight bands to overlap at their requested widths.
pub(super) fn overlap_candidate_prepared(a: &PreparedOpposing, b: &PreparedOpposing) -> bool {
    let Some(bisector) = OpposingBisector::from_prepared(a, b) else {
        return false;
    };
    let s_min = bisector.scale_at(bisector.domain.lo);
    s_min.is_finite() && s_min < 1.0
}

/// Subtract the closed strip `|(X(u) - origin) · n_c| <= width_c * s(u)` from
/// every interval in `kept`.
fn subtract_corner_strip(
    kept: &mut Vec<Interval>,
    bisector: &OpposingBisector,
    plane_normal: Vec3,
    line: (Point3, Vec3, f64),
) -> Result<()> {
    let (origin, tangent, width_c) = line;
    let perp = plane_normal.cross(tangent);
    if perp.length() <= EFF_ZERO {
        return Ok(());
    }
    let nc = perp.unitize()?;
    let f0 = (bisector.x0 - origin).dot(nc);
    let f1 = bisector.u_dir.dot(nc);
    let g0 = width_c * bisector.s0;
    let g1 = width_c * bisector.s_slope;
    // Forbidden set: -g(u) <= f(u) <= g(u), i.e., f(u)+g(u) >= 0 AND g(u)-f(u) >= 0.
    let Some(cut) = Interval::all()
        .clip_linear(f0 + g0, f1 + g1, true)
        .and_then(|iv| iv.clip_linear(g0 - f0, g1 - f1, true))
    else {
        return Ok(());
    };
    let mut next = Vec::new();
    for interval in core::mem::take(kept) {
        interval.subtract_closed(cut.lo, cut.hi, &mut next);
    }
    *kept = next;
    Ok(())
}

/// Find the leftmost on-face bisector parameter `u` across `kept` by splitting
/// each retained interval at exact crossings with `face_id`'s boundary edges
/// (outer loop and inner holes) and classifying each resulting piece.
fn first_on_face_parameter(
    k: &Kernel,
    target: &mut Brep,
    face_id: FaceId,
    bisector: &OpposingBisector,
    kept: &[Interval],
    tolerance: f64,
) -> Result<Option<f64>> {
    let face_edges = face::p4::edges(target, face_id, None, false, None)?;
    let mut edge_curves = Vec::new();
    for edge_id in face_edges {
        if let Some(curve) = edge::evaluation_curve(target, edge_id)? {
            let ivl = edge::interval(target, edge_id)?;
            edge_curves.push((curve, ivl));
        }
    }
    let cross_tol = tolerance.max(EFF_ZERO);
    for &iv in kept {
        if iv.lo == iv.hi {
            if iv.lo_closed
                && iv.hi_closed
                && !super::super::sample_off_face(k, target, face_id, bisector.point(iv.lo))?
            {
                return Ok(Some(iv.lo));
            }
            continue;
        }
        let start = bisector.point(iv.lo);
        let end = bisector.point(iv.hi);
        let mut cuts = Vec::new();
        if start.distance(end) > EFF_ZERO {
            let seg = Curve::from(create_line_segment(start, end)?);
            let seg_ivl = seg.natural_interval();
            for (edge_curve, edge_ivl) in &edge_curves {
                let crossings =
                    curve_crossings(k, &seg, &seg_ivl, edge_curve, edge_ivl, None, cross_tol)?;
                for (t_seg, _) in crossings {
                    let pt = seg.evaluate_point(k, t_seg)?;
                    let u = (pt - bisector.x0).dot(bisector.u_dir);
                    if u.is_finite() && u > iv.lo && u < iv.hi {
                        cuts.push(u);
                    }
                }
            }
        }
        cuts.sort_by(f64::total_cmp);
        cuts.push(iv.hi);
        let mut sub_lo = iv.lo;
        let mut sub_lo_is_iv_lo = true;
        for sub_hi in cuts {
            if sub_hi <= sub_lo {
                continue;
            }
            let mid = 0.5 * (sub_lo + sub_hi);
            let mid_on_face =
                !super::super::sample_off_face(k, target, face_id, bisector.point(mid))?;
            if mid_on_face {
                if bisector.s_slope == 0.0 {
                    return Ok(Some(mid));
                }
                let can_use_sub_lo = !sub_lo_is_iv_lo || iv.lo_closed;
                if can_use_sub_lo
                    && !super::super::sample_off_face(k, target, face_id, bisector.point(sub_lo))?
                {
                    return Ok(Some(sub_lo));
                }
                let span = sub_hi - sub_lo;
                let step = (span * 0.5).min(tolerance * 1e-2).max(span * 1e-9);
                let probe = sub_lo + step;
                if probe > sub_lo
                    && probe < sub_hi
                    && !super::super::sample_off_face(k, target, face_id, bisector.point(probe))?
                {
                    return Ok(Some(probe));
                }
                return Ok(Some(mid));
            }
            sub_lo = sub_hi;
            sub_lo_is_iv_lo = false;
        }
        if iv.hi_closed
            && !super::super::sample_off_face(k, target, face_id, bisector.point(iv.hi))?
        {
            return Ok(Some(iv.hi));
        }
    }
    Ok(None)
}

/// Construct the retained on-face contact of two non-parallel opposing straight
/// bands on a shared planar face.
pub(super) fn retained_contact(
    k: &Kernel,
    target: &mut Brep,
    a: &Band,
    b: &Band,
    blended: &[EdgeId],
    corners: &[CornerBand],
    as_scale: bool,
) -> Result<Option<BandContact>> {
    let (Some(prep_a), Some(prep_b)) = (
        prepare(k, target, a, blended)?,
        prepare(k, target, b, blended)?,
    ) else {
        return Ok(None);
    };
    let Some(bisector) = OpposingBisector::from_prepared(&prep_a, &prep_b) else {
        return Ok(None);
    };
    let ends = [
        edge::start_vertex(target, a.edge)?,
        edge::end_vertex(target, a.edge)?,
        edge::start_vertex(target, b.edge)?,
        edge::end_vertex(target, b.edge)?,
    ];
    let mut kept = vec![bisector.domain];
    for corner in corners {
        if corner.edge == a.edge
            || corner.edge == b.edge
            || !ends
                .iter()
                .flatten()
                .any(|v| corner.ends.contains(&Some(*v)))
        {
            continue;
        }
        let Some(line) = corner.line else {
            return Ok(None);
        };
        subtract_corner_strip(&mut kept, &bisector, a.plane_normal, line)?;
        if kept.is_empty() {
            return Ok(None);
        }
    }
    let tolerance = edge::tolerance(target, a.edge)?.max(edge::tolerance(target, b.edge)?);
    let Some(u_on_face) = first_on_face_parameter(k, target, a.face, &bisector, &kept, tolerance)?
    else {
        return Ok(None);
    };
    let contact_at = bisector.point(u_on_face);
    let da = (contact_at - bisector.a0).dot(bisector.na);
    let db = (contact_at - bisector.b0).dot(bisector.nb);
    if !(da.is_finite() && db.is_finite() && da > 0.0 && db > 0.0) {
        return Ok(None);
    }
    let widths = a.width + b.width;
    if !(widths.is_finite() && widths > 0.0) {
        return Err(SolidError::invalid_input(OP, "non-finite total band width"));
    }
    let total_gap = da + db;
    let scale = (da / a.width).max(db / b.width);
    if !(scale.is_finite() && scale > 0.0 && total_gap.is_finite()) {
        return Err(SolidError::invalid_input(
            OP,
            "invalid planar contact scale",
        ));
    }
    let coord_scale = contact_at
        .max_dimension()
        .max(bisector.a0.max_dimension())
        .max(bisector.a1.max_dimension())
        .max(bisector.b0.max_dimension())
        .max(bisector.b1.max_dimension())
        + widths;
    let rounding = crate::nurbs::math::gamma(32) * coord_scale;
    let overlapping = rounding.is_finite() && rounding < widths && (widths - total_gap) > rounding;
    let rho = a
        .offsets
        .first()
        .copied()
        .ok_or_else(|| super::missing(BandInput::FirstOffset))?
        .abs();
    let equal = !as_scale
        && a.offsets
            .iter()
            .chain(b.offsets.iter())
            .all(|offset| (offset.abs() - rho).abs() <= EFF_ZERO * (1.0 + rho));
    let (size, kind) = if equal {
        (rho * scale, FitKind::Radius)
    } else {
        (
            ((total_gap - 1000.0 * tolerance) / widths).max(0.0)
                * (1.0 - crate::nurbs::math::gamma(7)),
            FitKind::Scale,
        )
    };
    if !size.is_finite() {
        return Err(SolidError::invalid_input(
            OP,
            "non-finite planar band contact",
        ));
    }
    Ok(Some(BandContact {
        size,
        kind,
        edge: a.edge,
        face: a.face,
        at: contact_at,
        overlapping,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_interval_clips_and_subtracts_cleanly() {
        let base = Interval::all()
            .clip_linear(0.0, 1.0, true)
            .and_then(|iv| iv.clip_linear(10.0, -1.0, true))
            .expect("non-empty [0, 10]");
        assert_eq!(base.lo, 0.0);
        assert_eq!(base.hi, 10.0);
        assert!(base.lo_closed && base.hi_closed);

        // Subtracting [0, 3] leaves (3, 10].
        let mut kept = Vec::new();
        base.subtract_closed(0.0, 3.0, &mut kept);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].lo, 3.0);
        assert_eq!(kept[0].hi, 10.0);
        assert!(!kept[0].lo_closed && kept[0].hi_closed);

        // Subtracting [5, 7] splits into (3, 5) and (7, 10].
        let mut split = Vec::new();
        for iv in kept {
            iv.subtract_closed(5.0, 7.0, &mut split);
        }
        assert_eq!(split.len(), 2);
        assert_eq!((split[0].lo, split[0].hi), (3.0, 5.0));
        assert!(!split[0].lo_closed && !split[0].hi_closed);
        assert_eq!((split[1].lo, split[1].hi), (7.0, 10.0));
        assert!(!split[1].lo_closed && split[1].hi_closed);
    }

    #[test]
    fn touching_open_and_closed_bounds_leave_empty_interval() {
        let open = Interval::all()
            .clip_linear(0.0, 1.0, false)
            .and_then(|iv| iv.clip_linear(0.0, -1.0, true));
        assert!(open.is_none());
        let closed = Interval::all()
            .clip_linear(0.0, 1.0, true)
            .and_then(|iv| iv.clip_linear(0.0, -1.0, true));
        let single = closed.expect("closed singleton [0, 0]");
        assert_eq!((single.lo, single.hi), (0.0, 0.0));
        let mut out = Vec::new();
        single.subtract_closed(0.0, 0.0, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn crossing_past_a_short_convex_cap_keeps_the_open_interior_domain() {
        let face = FaceId::from_index(1);
        let z = Vec3::new(0.0, 0.0, 1.0);
        // Band A along y = 0 (x in [0, 10]), short 0.25 mm convex cap on x = 0.
        let a = PreparedOpposing {
            face,
            plane_normal: z,
            p0: Point3::new(0.0, 0.0, 0.0),
            p1: Point3::new(10.0, 0.0, 0.0),
            inward: Vec3::new(0.0, 1.0, 0.0),
            width: 1.5,
            boundaries: [
                EndBoundary {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    inward: Vec3::new(1.0, 0.0, 0.0),
                    offset: 0.0,
                    edge_tol: 1e-5,
                    closed: true,
                    convex_cap: Some((Vec3::new(0.0, 1.0, 0.0), 0.25)),
                },
                EndBoundary {
                    origin: Point3::new(10.0, 0.0, 0.0),
                    inward: Vec3::new(-1.0, 0.0, 0.0),
                    offset: 0.0,
                    edge_tol: 1e-5,
                    closed: true,
                    convex_cap: None,
                },
            ],
        };
        // Band B slightly skewed: (0, 2.0) -> (10, 2.1), inward normal roughly -Y.
        let eb = Vec3::new(10.0, 0.1, 0.0).unitize().unwrap();
        let nb = Vec3::new(eb.y, -eb.x, 0.0);
        let b = PreparedOpposing {
            face,
            plane_normal: z,
            p0: Point3::new(0.0, 2.0, 0.0),
            p1: Point3::new(10.0, 2.1, 0.0),
            inward: nb,
            width: 1.5,
            boundaries: [
                EndBoundary {
                    origin: Point3::new(0.0, 2.0, 0.0),
                    inward: Vec3::new(1.0, 0.0, 0.0),
                    offset: 0.0,
                    edge_tol: 1e-5,
                    closed: true,
                    convex_cap: None,
                },
                EndBoundary {
                    origin: Point3::new(10.0, 2.1, 0.0),
                    inward: Vec3::new(-1.0, 0.0, 0.0),
                    offset: 0.0,
                    edge_tol: 1e-5,
                    closed: true,
                    convex_cap: None,
                },
            ],
        };
        // Bisector meets x = 0 at y ~ 1.0 > 0.25 (past cap_len): the lower bound
        // becomes open (excluding only the crossing on x = 0), while the domain
        // x in (0, 10] stays non-empty and reports overlap.
        let bisector = OpposingBisector::from_prepared(&a, &b).expect("domain remains non-empty");
        assert!(!bisector.domain.lo_closed);
        assert!(bisector.domain.hi_closed);
        assert!(overlap_candidate_prepared(&a, &b));
    }
}
