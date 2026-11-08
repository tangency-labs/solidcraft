//! Which face of an edge a chamfer's first size is along: the outward normals of the edge's
//! two faces at its middle, as how far each leans along a direction, for an application that
//! chooses the face (a chamfer whose first distance is along the face facing up most).
//!
//! Not upstream (solidcore's). The normals are the faces' surfaces' at the binormal points of
//! the edge's blend edge use and its radial mate at the middle of the edge's interval, signed
//! by the face's outer face use (its side facing the body's outside). Evaluated, not
//! certified: the leans are for a preference between the two faces, not a decision at a
//! tolerance.
//!
//! [`widest_wedge_bound`] is enclosed, not evaluated: an upper bound on the wedge between an
//! edge's faces anywhere along it, by interval enclosures of the faces' normals along their
//! trim curves, branch and bound over the edge's parameter. No recursion: the pieces are a
//! list, at most [`MAX_WEDGE_PIECES`].

use std::collections::BinaryHeap;

use crate::blend::fillet_section::certificate::{
    curve_derivs_over, nurbs_partials_over_box, NurbsCurve,
};
use crate::blend::fillet_section::interval::{round_up, Ivl};
use crate::blend::fillet_section::series::V3;
use crate::blend::fillet_section::station::{as_b_spline, Nurbs};
use crate::curve::curve::{Curve, CurveKind};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::brep::evaluate_surface_normal;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
use crate::topology::face::p2::outer_faceuse;
use crate::topology::{edge, EdgeId, EdgeuseId, FaceId};
use crate::{Brep, Kernel, Result, SolidError, Vec3};

const OP: &str = "blend::chamfer_side::faces_along";

fn invalid(why: &'static str) -> SolidError {
    SolidError::invalid_input(OP, why)
}

/// The face edge use `u` bounds and its outward unit normal at parameter `t` of its edge.
fn outward(k: &Kernel, b: &mut Brep, u: EdgeuseId, t: f64) -> Result<(FaceId, Vec3)> {
    let face = match eu1::faceuse(b, u)? {
        Some(fu) => b.faceuse(fu)?.face,
        None => None,
    }
    .ok_or_else(|| invalid("an edge use of the edge bounds no face"))?;
    let fu =
        outer_faceuse(b, face)?.ok_or_else(|| invalid("a face of the edge has no face use"))?;
    let at = evaluate_binormal(k, b, u, t, false, &BinormalOptions::default())?;
    let normal = evaluate_surface_normal(k, b.face(face)?.surface.as_ref(), at.uv)?;
    let len = normal.length();
    if !(len > 0.0 && len.is_finite()) {
        return Err(invalid("a face's normal vanishes at the edge's middle"));
    }
    let sign = if b.faceuse(fu)?.orientation == OrientType::Same {
        1.0
    } else {
        -1.0
    };
    Ok((face, normal * (sign / len)))
}

/// The two faces of `e`, the face of its blend edge use first (a chamfer's support 1) and its
/// radial mate's second, each with how far its outward unit normal at the middle of the edge
/// leans along `dir` (the normal's dot with `dir`).
///
/// # Errors
/// [`SolidError::InvalidInput`] for a direction that is not finite, an edge without a blend
/// edge use or its mate (not between two faces), or a face whose normal vanishes there; the
/// evaluators' errors.
pub fn faces_along(k: &Kernel, b: &mut Brep, e: EdgeId, dir: Vec3) -> Result<[(FaceId, f64); 2]> {
    if !(dir.x.is_finite() && dir.y.is_finite() && dir.z.is_finite()) {
        return Err(invalid("the direction is finite"));
    }
    let u0 =
        edge::blend_edgeuse(k, b, e)?.ok_or_else(|| invalid("the edge has no blend edge use"))?;
    let u1 = eu1::radial(b, u0)?.ok_or_else(|| invalid("the edge lies on one face only"))?;
    let t = b.edge(e)?.interval.evaluate(0.5);
    let (f0, n0) = outward(k, b, u0, t)?;
    let (f1, n1) = outward(k, b, u1, t)?;
    Ok([(f0, n0.dot(dir)), (f1, n1.dot(dir))])
}

/// An upper bound on the widest wedge along an edge ([`widest_wedge_bound`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WedgeBound {
    /// The bound, radians.
    pub wedge: f64,
    /// The pieces of the edge's parameter enclosed when the bound was taken.
    pub pieces: usize,
}

/// The most pieces [`widest_wedge_bound`] splits an edge into. A piece's enclosure narrows in
/// proportion to its length, so a bound that has not met its gap by then is taken as it is:
/// still proved, only less tight.
pub const MAX_WEDGE_PIECES: usize = 1 << 12;

/// The B-spline a trim curve is (a line, a conic and the like are B-splines with a kind);
/// `None` for one that is not.
fn trim_b_spline(c: &Curve) -> Option<&BSplineCurve> {
    match c.kind() {
        CurveKind::BSpline(b) => Some(b),
        CurveKind::Line(l) => Some(l.base()),
        CurveKind::Ellipse(e) => Some(e.base()),
        CurveKind::Hyperbola(h) => Some(h.base()),
        CurveKind::Parabola(p) => Some(p.base()),
        CurveKind::Hermite(_)
        | CurveKind::Projected(_)
        | CurveKind::Iso(_)
        | CurveKind::Offset(_)
        | CurveKind::Conic(_)
        | CurveKind::CrvInVolume(_)
        | CurveKind::CrvOnSurf(_)
        | CurveKind::TangentField(_)
        | CurveKind::Composite(_) => None,
    }
}

/// A piece `[a, z]` of an edge's parameter with the upper end `hi` of its normals' cosine,
/// ordered by `hi` (then by `a`) so the piece to halve next is the greatest.
#[derive(Debug, Clone, Copy)]
struct Piece {
    hi: f64,
    a: f64,
    z: f64,
}

impl PartialEq for Piece {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}

impl Eq for Piece {}

impl PartialOrd for Piece {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Piece {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.hi
            .total_cmp(&other.hi)
            .then(other.a.total_cmp(&self.a))
    }
}

/// One face of an edge as [`widest_wedge_bound`] encloses it: its trim curve, its surface's
/// net, the range its uv is clamped to, and the sign that turns `S_u x S_v` outward.
struct Side {
    trim: NurbsCurve,
    net: Nurbs,
    u: Ivl,
    v: Ivl,
    sign: f64,
}

fn cross(a: V3<Ivl>, b: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: a.y * b.z - a.z * b.y,
        y: a.z * b.x - a.x * b.z,
        z: a.x * b.y - a.y * b.x,
    }
}

fn dot(a: V3<Ivl>, b: V3<Ivl>) -> Ivl {
    a.x * b.x + a.y * b.y + a.z * b.z
}

/// `x` clamped to `[r.lo, r.hi]`, end by end: holds the clamp of every number in `x`.
fn clamped(x: Ivl, r: Ivl) -> Ivl {
    let c = |t: f64| t.max(r.lo).min(r.hi);
    Ivl::new(c(x.lo), c(x.hi))
}

impl Side {
    /// `S_u x S_v` over the trim curve's points for every parameter in `t`, enclosed.
    fn normal_over(&self, t: Ivl) -> Result<V3<Ivl>> {
        let at = curve_derivs_over(&self.trim, t, 0, None)?;
        let p = at
            .first()
            .copied()
            .ok_or_else(|| invalid("a trim curve has no point"))?;
        let (u, v) = (clamped(p.x, self.u), clamped(p.y, self.v));
        let partials = nurbs_partials_over_box(&self.net, u.lo, u.hi, v.lo, v.hi, 1, 0)?;
        let n = cross(partials.at(1, 0), partials.at(0, 1));
        let s = Ivl::from(self.sign);
        Ok(V3 {
            x: n.x * s,
            y: n.y * s,
            z: n.z * s,
        })
    }
}

/// The face of edge use `u` as [`widest_wedge_bound`] encloses it, its sign read against the
/// outward normal at parameter `t` of the edge.
fn side(k: &Kernel, b: &mut Brep, u: EdgeuseId, t: f64) -> Result<Side> {
    let (face, outward_n) = outward(k, b, u, t)?;
    let trim = eu1::uv_trim_curve(b, u)?
        .0
        .and_then(trim_b_spline)
        .ok_or_else(|| invalid("an edge use's trim curve is no B-spline"))?;
    let trim = NurbsCurve::of(trim)?;
    let rec = b.face(face)?;
    let surface = rec
        .surface
        .as_ref()
        .ok_or_else(|| invalid("a face of the edge has no surface"))?;
    // an offset surface's normal is its base's at the same (u, v) (an offset is built only
    // where it does not fold, so the direction is kept up to the sign the probe below reads):
    // its base's net encloses it
    let mut surface = surface;
    for _ in 0..=crate::surface::offset_surface::MAX_SURFACE_NESTING {
        match surface.kind() {
            crate::surface::surface::SurfaceKind::OffsetSurface(o) => surface = o.base_surface(),
            _ => break,
        }
    }
    let net = as_b_spline(k, surface)?.ok_or_else(|| invalid("a face of the edge has no net"))?;
    let net = Nurbs::of(&net)?;
    let domain = rec.uv_domain;
    let first = |knots: &[f64], deg: usize| knots.get(deg).copied().unwrap_or(f64::NAN);
    let last = |knots: &[f64], deg: usize| {
        knots
            .len()
            .checked_sub(deg + 1)
            .and_then(|i| knots.get(i))
            .copied()
            .unwrap_or(f64::NAN)
    };
    let u = Ivl::new(
        domain.min().x.max(first(&net.u, net.p)),
        domain.max().x.min(last(&net.u, net.p)),
    );
    let v = Ivl::new(
        domain.min().y.max(first(&net.v, net.q)),
        domain.max().y.min(last(&net.v, net.q)),
    );
    let mut probe = Side {
        trim,
        net,
        u,
        v,
        sign: 1.0,
    };
    let n = probe.normal_over(Ivl::from(t))?;
    let lean = dot(n, V3::from_vec3(outward_n));
    probe.sign = if lean.lo > 0.0 {
        1.0
    } else if lean.hi < 0.0 {
        -1.0
    } else {
        return Err(invalid("a face's normal cannot be told from its net's"));
    };
    Ok(probe)
}

/// The cosine of the angle between the faces' outward normals, enclosed for every parameter
/// in `t`; the whole line where a normal's enclosure holds 0.
fn normals_cos_over(sides: &[Side; 2], t: Ivl) -> Result<Ivl> {
    let [a, b] = sides;
    let (na, nb) = (a.normal_over(t)?, b.normal_over(t)?);
    Ok(dot(na, nb) / (dot(na, na).sqrt() * dot(nb, nb).sqrt()))
}

/// An upper bound on the wedge between the faces of edge `e` anywhere along it (radians, the
/// angle between the directions into the faces, as [`faces_along`]'s binormals give it), with
/// the enclosure of its cosine's least value within `gap` of its upper end where
/// [`MAX_WEDGE_PIECES`] allows.
///
/// The wedge is `pi` less the angle between the outward normals, so its widest is where their
/// cosine `c` is largest. Each piece of the edge's parameter encloses `c` by the faces' normals
/// `S_u x S_v` over the uv their trim curves reach there (interval and Bernstein enclosures,
/// `blend::fillet_section::certificate`); the piece with the highest upper end is halved until
/// that end is within `gap` of the largest `c` enclosed at a single parameter. The bound is that
/// upper end, so it holds wherever along the edge the widest wedge lies.
///
/// # Errors
/// [`SolidError::InvalidInput`] for an edge without a blend edge use or its mate, a trim curve
/// that is no B-spline, a face with no net or one whose normal's side cannot be told; those of
/// the enclosures.
pub fn widest_wedge_bound(k: &Kernel, b: &mut Brep, e: EdgeId, gap: f64) -> Result<WedgeBound> {
    let u0 =
        edge::blend_edgeuse(k, b, e)?.ok_or_else(|| invalid("the edge has no blend edge use"))?;
    let u1 = eu1::radial(b, u0)?.ok_or_else(|| invalid("the edge lies on one face only"))?;
    let interval = b.edge(e)?.interval;
    let t_mid = interval.evaluate(0.5);
    let sides = [side(k, b, u0, t_mid)?, side(k, b, u1, t_mid)?];
    let (t0, t1) = (interval.evaluate(0.0), interval.evaluate(1.0));
    let (t0, t1) = (t0.min(t1), t0.max(t1));
    // the largest cosine proved at one parameter: a lower bound on the largest of all
    let mut least = normals_cos_over(&sides, Ivl::from(t_mid))?.lo;
    let mut pieces = BinaryHeap::new();
    pieces.push(Piece {
        hi: normals_cos_over(&sides, Ivl::new(t0, t1))?.hi,
        a: t0,
        z: t1,
    });
    while pieces.len() <= MAX_WEDGE_PIECES {
        let Some(piece) = pieces.pop() else {
            break;
        };
        let Piece { hi, a, z } = piece;
        let m = 0.5 * (a + z);
        // halved while it is past the gap and its middle splits it
        let splits = hi - least > gap && a < m && m < z;
        if !splits {
            pieces.push(piece);
            break;
        }
        least = least.max(normals_cos_over(&sides, Ivl::from(m))?.lo);
        for (a, z) in [(a, m), (m, z)] {
            let hi = normals_cos_over(&sides, Ivl::new(a, z))?.hi;
            pieces.push(Piece { hi, a, z });
        }
    }
    let top = pieces.iter().fold(f64::NEG_INFINITY, |t, p| t.max(p.hi));
    // the wedge is pi less the normals' angle: acos(-c), rounded up; a cosine enclosed at 1 or
    // above (or not at all) is clamped to -1, a wedge of pi
    Ok(WedgeBound {
        wedge: round_up((-top).max(-1.0).acos()).min(crate::math::PI),
        pieces: pieces.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitive::api_primitives::create_box;
    use crate::query::api_queries::edge_closest_point;

    /// A unit box's top front edge: its top face leans 1 along +z, its front face 0.
    #[test]
    fn the_top_face_of_a_box_edge_leans_up() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Vec3::new(0.0, 0.0, 0.0), 1.0, 1.0, 1.0).unwrap();
        let p = Vec3::new(0.5, 0.0, 1.0);
        let e = b
            .edge_list()
            .iter()
            .copied()
            .find(|&e| edge_closest_point(&k, &b, e, p).unwrap().closest.distance < 1e-12)
            .unwrap();
        let leans = faces_along(&k, &mut b, e, Vec3::new(0.0, 0.0, 1.0)).unwrap();
        let mut ups: Vec<f64> = leans.iter().map(|(_, l)| *l).collect();
        ups.sort_by(f64::total_cmp);
        assert!(
            ups[0].abs() < 1e-12 && (ups[1] - 1.0).abs() < 1e-12,
            "{ups:?}"
        );
        assert_ne!(leans[0].0, leans[1].0);
        assert!(faces_along(&k, &mut b, e, Vec3::new(f64::NAN, 0.0, 0.0)).is_err());
    }

    /// A box edge's faces meet square all along it: the bound is a right angle, from above,
    /// within the gap asked (`c` within 1e-9 of 0: the wedge within about 1e-9 of pi / 2).
    #[test]
    fn a_box_edge_s_widest_wedge_is_a_right_angle_from_above() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Vec3::new(0.0, 0.0, 0.0), 1.0, 1.0, 1.0).unwrap();
        let p = Vec3::new(0.5, 0.0, 1.0);
        let e = b
            .edge_list()
            .iter()
            .copied()
            .find(|&e| edge_closest_point(&k, &b, e, p).unwrap().closest.distance < 1e-12)
            .unwrap();
        // the binormals the trim curves are read with
        faces_along(&k, &mut b, e, Vec3::new(0.0, 0.0, 1.0)).unwrap();
        let w = widest_wedge_bound(&k, &mut b, e, 1e-9).unwrap().wedge;
        let right = 0.5 * crate::math::PI;
        assert!(w >= right && w - right < 2e-9, "{w}");
    }

    /// The box's top front edge.
    fn box_top_front(k: &mut Kernel) -> (Brep, EdgeId) {
        let b = create_box(k, Vec3::new(0.0, 0.0, 0.0), 1.0, 1.0, 1.0).unwrap();
        let p = Vec3::new(0.5, 0.0, 1.0);
        let e = b
            .edge_list()
            .iter()
            .copied()
            .find(|&e| edge_closest_point(k, &b, e, p).unwrap().closest.distance < 1e-12)
            .unwrap();
        (b, e)
    }

    /// A gap no enclosure meets: the pieces stop at the cap, and the bound stands, from above.
    #[test]
    fn a_gap_never_met_stops_at_the_cap_with_a_bound_from_above() {
        let mut k = Kernel::default();
        let (mut b, e) = box_top_front(&mut k);
        faces_along(&k, &mut b, e, Vec3::new(0.0, 0.0, 1.0)).unwrap();
        let w = widest_wedge_bound(&k, &mut b, e, -1.0).unwrap();
        assert!(w.pieces > MAX_WEDGE_PIECES, "{w:?}");
        assert!(w.wedge >= 0.5 * crate::math::PI, "{w:?}");
    }

    /// A face whose surface is an offset surface is enclosed by its base's net: the top made
    /// the offset by 0 of its own plane meets the front square, as before.
    #[test]
    fn an_offset_surface_face_is_enclosed_by_its_base() {
        use crate::surface::offset_surface::OffsetSurface;
        use crate::surface::surface::Surface;
        let mut k = Kernel::default();
        let (mut b, e) = box_top_front(&mut k);
        let [(f0, l0), (f1, _)] = faces_along(&k, &mut b, e, Vec3::new(0.0, 0.0, 1.0)).unwrap();
        let top = if l0 > 0.5 { f0 } else { f1 };
        let plane = b.face(top).unwrap().surface.clone().unwrap();
        let offset = OffsetSurface::new(&k, 0.0, plane, true, 0).unwrap();
        b.face_mut(top).unwrap().surface = Some(Surface::from(offset));
        let w = widest_wedge_bound(&k, &mut b, e, 1e-9).unwrap().wedge;
        let right = 0.5 * crate::math::PI;
        assert!(w >= right && w - right < 2e-9, "{w}");
    }
}
