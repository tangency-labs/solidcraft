//! Untrim and extend: the C API's `smlib_untrim_face` and `smlib_extend_sheet`.
//!
//! Untrim is solidcore-ops' ([`solidcore_ops::untrim::untrim_face`]): the face and the holes are
//! picked here as the capi's `smlib_untrim.cpp` picked them (the face nearest the point, the
//! inner loop nearest each hole point), and ops restores the face towards its surface.
//!
//! Extend stays on the kernel here, a port of `capi/src/smlib_extend.cpp` (`on_side`, `eval`,
//! `nearest_edge`, `is_boundary`, `side_edge`, `across`, `cone_continued`, `same_patch`):
//! solidcore-ops' `extend::extend_sheet` gives other results (a plane grows its trim, an
//! extension gets no analytic kind back), so it moves when ops gives these.
//!
//! A sheet is extended by SMLib's extension of its surface (`CreateExtendedSurface`: its end
//! spans continued, or a G1 or G2 run-off), the distance asked of it corrected until the
//! extension measures the distance across the side's middle; `CopyAndAddAnalytics` gives the
//! result back its analytic kind, and a cone or cylinder continued naturally stays the analytic
//! widened. The new face and each of its sides take the attributes of the face and side they
//! continue.

use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::object::Shape;
use solidcore::geom::tol::USE_DEFAULT;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::query::api_queries::{edge_closest_point, face_closest_point_in_shape, face_internal_point};
use solidcore::query::brep::{BrepQuery, LiveBrepObjects};
use solidcore::surface::cone::p1::Cone;
use solidcore::surface::extend::Continuity;
use solidcore::surface::side::BoxSide;
use solidcore::surface::surface::Surface;
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::brep::p1 as brep_p1;
use solidcore::topology::brep::p3::new_brep;
use solidcore::topology::brep::p8::create_face_from_surface;
use solidcore::topology::face::{p1 as face_p1, p2 as face_p2, p4 as face_p4};
use solidcore::topology::{Brep, BrepId, EdgeId, FaceId};
use solidcore::{EntityRef, SolidError};

use super::blend::{p3, refused, status};
use super::call::ops_as;
use super::sweep::build_faces;
use super::{Body, answer};
use crate::trim::Continuation;
use crate::{Error, Result};
use solidcore::topology::brep::p3::brep_copy;

/// `MAX_HOLE_POINTS`.
const MAX_HOLE_POINTS: usize = 10_000;

const ONE: BrepId = BrepId::from_index(0);

/// `face_at`: the face of `b` nearest `p`, within `tol` (`SmApiFaceClosestPoint`; a face whose
/// query fails is skipped).
pub(super) fn face_at(k: &mut Kernel, b: &mut Brep, p: Point3, tol: f64) -> solidcore::Result<Option<FaceId>> {
    let all = brep_p1::faces(b, None)?;
    let mut shapes = Vec::with_capacity(all.len());
    for &f in &all {
        shapes.push(answer(Shape::new(k, &(ONE, &*b), &[], &[(ONE, f)], &[], &[]))?);
    }
    let query = BrepQuery::of_brep(b);
    let identity = query.effective_identity();
    let objects = LiveBrepObjects::new(b, &identity, None)?;
    let (mut best, mut d0) = (None, f64::INFINITY);
    for (&f, shape) in all.iter().zip(&shapes) {
        let Some(shape) = shape else { continue };
        if let Some(c) = answer(face_closest_point_in_shape(k, shape, &objects, (ONE, f), p))?
            && c.distance.is_finite()
            && c.distance < d0
        {
            d0 = c.distance;
            best = Some(f);
        }
    }
    Ok(best.filter(|_| d0 <= tol))
}

/// `copy_attributes`: `to` (in `into`) takes a copy of each attribute of `from` (in `src`).
fn copy_attributes(src: &Brep, from: EntityRef, into: &mut Brep, to: EntityRef) -> solidcore::Result<()> {
    let copies: Vec<_> = src.entity_attributes(from).into_iter().map(|(_, a)| a.make_copy()).collect();
    for a in copies {
        into.add_attribute(to, a)?;
    }
    Ok(())
}

/// The orientation of `f`'s upward faceuse.
fn upward(b: &Brep, f: FaceId) -> solidcore::Result<Option<OrientType>> {
    Ok(match face_p2::upward_faceuse(b, f)? {
        Some(fu) => Some(b.faceuse(fu)?.orientation),
        None => None,
    })
}

/// The index of the least of `distances` within `tol`, the first of equal ones; an entry that
/// is `None` or not finite is passed over (`face_at` and the nearest hole, as the capi picked).
fn nearest_within(distances: impl IntoIterator<Item = Option<f64>>, tol: f64) -> Option<usize> {
    let (mut best, mut d0) = (None, f64::INFINITY);
    for (i, d) in distances.into_iter().enumerate() {
        if let Some(d) = d.filter(|d| d.is_finite())
            && d < d0
        {
            d0 = d;
            best = Some(i);
        }
    }
    best.filter(|_| d0 <= tol)
}

/// `smlib_untrim_face`: the face of `body` nearest `point` (within `tol`) restored towards its
/// surface: the holes nearest `holes` (all of them for `None`) taken away and, with `outer`, its
/// outer boundary back on the surface's parameter box. The face alone, or with `in_place` the
/// whole body with it replaced ([`solidcore_ops::untrim::untrim_face`]).
///
/// The picks are the capi's: the face nearest `point` as it is trimmed
/// ([`solidcore_ops::Body::face_distances_within`]), and per hole point the inner loop of that
/// face whose nearest edge is nearest it ([`solidcore_ops::Body::face_loops`],
/// [`solidcore_ops::Body::edge_closest_point`]), each within `tol`, the first of equal ones.
pub(crate) fn untrim_face(body: &Body, point: [f64; 3], tol: f64, holes: Option<&[[f64; 3]]>, outer: bool, in_place: bool) -> Result<Body> {
    let n_holes = holes.map(<[_]>::len);
    if !point.iter().all(|x| x.is_finite())
        || !tol.is_finite()
        || !(tol > 0.0)
        || n_holes.is_some_and(|n| n > MAX_HOLE_POINTS)
        || holes.is_some_and(|h| !h.iter().flatten().all(|x| x.is_finite()))
        || (n_holes == Some(0) && !outer)
    {
        return Err(Error::Argument);
    }
    if body.kernel_left_unfinished() {
        return Err(Error::Internal("smlib_untrim_face: the body was left unfinished by an earlier panic".to_owned()));
    }
    const CALL: &str = "smlib_untrim_face";
    let order = body.own_order();
    let rows = ops_as(CALL, &[("brep", body)], refused, || body.face_distances_within(&order, &[point], tol))?;
    let face = nearest_within(rows.iter().map(|row| row.first().copied().flatten()), tol).ok_or(Error::NoMatch { point: 0 })?;
    // The holes taken away: all of them, or the one nearest each point, named by an edge of it.
    let mut along: Vec<usize> = Vec::new();
    if let Some(points) = holes {
        let loops = ops_as(CALL, &[("brep", body)], refused, || body.face_loops(&order, face))?;
        let inner = loops.get(1..).unwrap_or_default();
        let mut taken: Vec<usize> = Vec::new();
        for &p in points {
            let mut distances = Vec::with_capacity(inner.len());
            for edges in inner {
                let mut d = f64::INFINITY;
                for &e in edges {
                    let hit = ops_as(CALL, &[("brep", body)], refused, || body.edge_closest_point(&order, e, p))?;
                    if let Some(hit) = hit.filter(|h| h.distance.is_finite()) {
                        d = d.min(hit.distance);
                    }
                }
                distances.push(Some(d));
            }
            let l = nearest_within(distances, tol).ok_or(Error::NoMatch { point: 0 })?;
            if !taken.contains(&l) {
                taken.push(l);
                along.extend(inner.get(l).and_then(|edges| edges.first()));
            }
        }
    }
    let which = if holes.is_some() { solidcore_ops::untrim::Holes::Along(&along) } else { solidcore_ops::untrim::Holes::All };
    ops_as(CALL, &[("brep", body)], refused, || solidcore_ops::untrim::untrim_face(body, &order, face, which, outer, in_place))
}

/// One side of a face's parameter box (`Side`): which, whether it is a u = const line (the face
/// grows in u), and whether it is at the box's max.
#[derive(Clone, Copy)]
struct Side {
    which: BoxSide,
    u: bool,
    high: bool,
}

/// `SIDES`.
const SIDES: [Side; 4] = [
    Side { which: BoxSide::UMin, u: true, high: false },
    Side { which: BoxSide::UMax, u: true, high: true },
    Side { which: BoxSide::VMin, u: false, high: false },
    Side { which: BoxSide::VMax, u: false, high: true },
];

/// `SAMPLES`.
const SAMPLES: usize = 9;

/// `on_side`: the point of side `s` of `dom` at `t` in [0, 1] along it, in UV.
fn on_side(dom: &Extent2d, s: Side, t: f64) -> Point2 {
    let (lo, hi) = (dom.min(), dom.max());
    if s.u {
        Point2::new(if s.high { hi.x } else { lo.x }, lo.y + t * (hi.y - lo.y))
    } else {
        Point2::new(lo.x + t * (hi.x - lo.x), if s.high { hi.y } else { lo.y })
    }
}

/// `eval`: the point at `uv` with its u and v derivatives; `None` where it does not evaluate or
/// is not finite.
fn eval(k: &Kernel, surf: &Surface, uv: Point2) -> Option<(Vec3, Vec3, Vec3)> {
    let d = surf.evaluate(k, uv, 1, 1, EvaluateFlags::default()).ok()?;
    let p = *d.first()?.first()?;
    let du = *d.get(1)?.first()?;
    let dv = *d.first()?.get(1)?;
    (p.length().is_finite() && du.length().is_finite() && dv.length().is_finite()).then_some((p, du, dv))
}

fn point_at(k: &Kernel, surf: &Surface, uv: Point2) -> Option<Vec3> {
    eval(k, surf, uv).map(|(p, _, _)| p)
}

/// `edge_distance`.
fn edge_distance(k: &Kernel, b: &Brep, e: EdgeId, p: Vec3) -> f64 {
    match edge_closest_point(k, b, e, p) {
        Ok(c) if c.closest.distance.is_finite() => c.closest.distance,
        _ => f64::INFINITY,
    }
}

/// `nearest_edge`: the distance to the nearest of `edges`, with that edge.
pub(super) fn nearest_edge(k: &Kernel, b: &Brep, edges: &[EdgeId], p: Vec3) -> (f64, Option<EdgeId>) {
    let (mut best, mut which) = (f64::INFINITY, None);
    for &e in edges {
        let d = edge_distance(k, b, e, p);
        if d < best {
            best = d;
            which = Some(e);
        }
    }
    (best, which)
}

/// `is_boundary`: whether side `s` of the face's box is a side of the face (its samples on the
/// face's edges), and whether it collapses to a point (a pole).
fn is_boundary(k: &Kernel, b: &Brep, surf: &Surface, dom: &Extent2d, s: Side, edges: &[EdgeId], tol: f64) -> (bool, bool) {
    let (Some(a), Some(z), Some(m)) =
        (point_at(k, surf, on_side(dom, s, 0.0)), point_at(k, surf, on_side(dom, s, 1.0)), point_at(k, surf, on_side(dom, s, 0.5)))
    else {
        return (false, false);
    };
    if (a - z).length() <= tol && (a - m).length() <= tol {
        return (true, true);
    }
    let all = (0..=SAMPLES).all(|i| match point_at(k, surf, on_side(dom, s, i as f64 / SAMPLES as f64)) {
        Some(p) => nearest_edge(k, b, edges, p).0 <= tol,
        None => false,
    });
    (all, false)
}

/// `side_edge`: the edge of `edges` side `s` of `dom` runs along (nearest the side's middle), or
/// `None` for a side collapsed to a point.
fn side_edge(k: &Kernel, b: &Brep, surf: &Surface, dom: &Extent2d, s: Side, edges: &[EdgeId], on: f64) -> Option<EdgeId> {
    let a = point_at(k, surf, on_side(dom, s, 0.0))?;
    let z = point_at(k, surf, on_side(dom, s, 1.0))?;
    let m = point_at(k, surf, on_side(dom, s, 0.5))?;
    if (a - z).length() <= on && (a - m).length() <= on {
        return None;
    }
    let (d, e) = nearest_edge(k, b, edges, m);
    e.filter(|_| d <= on)
}

/// `cone_continued`: a cone's or cylinder's face continued naturally `distance` past side `s` of
/// its box `dom`, as the analytic itself over a wider STEP domain (degrees round the axis,
/// height along it). `None` where the cone can't be widened so (more than a turn, or past the
/// apex).
fn cone_continued(k: &Kernel, surf: &Surface, dom: &Extent2d, s: Side, distance: f64) -> Option<Surface> {
    let SurfaceKind::Cone(cone) = surf.kind() else { return None };
    let mut cone: Cone = cone.as_ref().clone();
    let step = |uv: Point2| cone.convert_uv_from_nurbs_to_step(k, uv).ok();
    let (a, z, m) = (step(on_side(dom, s, 0.0))?, step(on_side(dom, s, 1.0))?, step(on_side(dom, s, 0.5))?);
    let (lo, hi) = (step(dom.min())?, step(dom.max())?);
    let semi = cone.canonical().semi_angle_deg;
    let mut bx = [[lo.x.min(hi.x), lo.x.max(hi.x)], [lo.y.min(hi.y), lo.y.max(hi.y)]];
    let eps = 1e-9 * (1.0 + m.x.abs() + m.y.abs());
    // The STEP coordinate constant along the side is the one the face grows in.
    let c = if (a.x - z.x).abs() <= eps {
        0
    } else if (a.y - z.y).abs() <= eps {
        1
    } else {
        return None;
    };
    let at = if c == 0 { m.x } else { m.y };
    let row = bx.get_mut(c)?;
    let end = if (at - row[1]).abs() <= (at - row[0]).abs() { 1 } else { 0 };
    let by = if c == 0 {
        let r = cone.radius(m.y);
        if !(r > 0.0) {
            return None;
        }
        (distance / r).to_degrees()
    } else {
        distance * semi.to_radians().cos()
    };
    if end == 1 {
        row[1] += by;
    } else {
        row[0] -= by;
    }
    if bx[0][1] - bx[0][0] > 360.0 + 1e-9 {
        return None;
    }
    let wider = Extent2d::from_bounds(bx[0][0], bx[1][0], bx[0][1], bx[1][1]);
    cone.adjust_step_uv_domain(k, &wider).ok()?;
    Some(Surface::from(cone))
}

/// `same_patch`: whether `p` and `q` span the same patch: their boxes' corners the same points,
/// in any order, within `tol`.
fn same_patch(k: &Kernel, p: &Surface, q: &Surface, tol: f64) -> bool {
    let (Ok(bp), Ok(bq)) = (p.natural_uv_domain_with_nurb(k), q.natural_uv_domain_with_nurb(k)) else { return false };
    let corners = |b: &Extent2d| {
        let (lo, hi) = (b.min(), b.max());
        [lo, Point2::new(hi.x, lo.y), hi, Point2::new(lo.x, hi.y)]
    };
    corners(&bp).into_iter().all(|u| {
        let Some(x) = point_at(k, p, u) else { return false };
        corners(&bq).into_iter().any(|v| point_at(k, q, v).is_some_and(|y| (x - y).length() <= tol))
    })
}

/// `extended_by`: the surface `surf` (whose face is its whole box `dom`) extended past side `s` as
/// `how` says, so that it runs `distance` past that side measured across it, exactly: solidcore's
/// one extension ([`Surface::extended_by_distance`]), which keeps the source's own part bit for bit
/// and meets the distance within 1e-9 of it. A distance it cannot meet exactly is
/// [`Error::NotExact`], never a surface that falls short or runs past.
fn extended_by(k: &Kernel, surf: &Surface, dom: &Extent2d, s: Side, how: Continuity, distance: f64) -> Result<Surface> {
    match surf.extended_by_distance(k, dom, s.which, how, distance) {
        Ok(Some(e)) => Ok(e.surface),
        Ok(None) => Err(Error::NotExact),
        Err(SolidError::ExtendRefused { .. } | SolidError::NotConverged { .. }) => Err(Error::NotExact),
        Err(e) => Err(status(e)),
    }
}

/// `smlib_extend_sheet`: the one-face sheet `sheet`, whose face is its surface's whole parameter
/// box, continued `distance` past the side its edge nearest `point` (within `tol`) runs along,
/// as `how` says. A sheet of another shape is [`Error::Unsupported`]; an extension that cannot be
/// made `distance` long across the side (within 1e-9 of it) is [`Error::NotExact`].
pub(crate) fn extend_sheet(sheet: &Body, point: [f64; 3], tol: f64, distance: f64, how: Continuation) -> Result<Body> {
    if !point.iter().all(|x| x.is_finite()) || !tol.is_finite() || !(tol > 0.0) || !distance.is_finite() || !(distance > 0.0) {
        return Err(Error::Argument);
    }
    if sheet.kernel_left_unfinished() {
        return Err(Error::Internal("smlib_extend_sheet: the body was left unfinished by an earlier panic".to_owned()));
    }
    super::run_on("smlib_extend_sheet", &[("sheet", sheet)], |k| {
        let b = brep_copy(k, &sheet.kernel())?;
        let faces = brep_p1::faces(&b, None)?;
        let [f] = faces.as_slice() else { return Ok(Err(Error::Unsupported)) };
        let f = *f;
        let Some(surf) = face_p2::surface(&b, f)?.cloned() else { return Ok(Err(Error::Unsupported)) };
        if face_p2::loops(&b, f, false, None)?.len() != 1 {
            return Ok(Err(Error::Unsupported));
        }
        let edges = face_p4::edges(&b, f, None, false, None)?;
        let (d, picked) = nearest_edge(k, &b, &edges, p3(point));
        let Some(picked) = picked.filter(|_| d <= tol) else { return Ok(Err(Error::NoMatch { point: 0 })) };
        let model_tol = b.tolerance().max(1e-9);
        let on = 10.0 * model_tol;
        let dom = face_p2::uv_domain(&b, f)?;
        // Every side of the box is a side of the face (the face is its whole box), and the picked
        // edge is all of one side.
        let mut grow = None;
        for s in SIDES {
            let (boundary, collapsed) = is_boundary(k, &b, &surf, &dom, s, &edges, on);
            if !boundary {
                return Ok(Err(Error::Unsupported));
            }
            if collapsed {
                continue;
            }
            let all = (0..=SAMPLES).all(|i| match point_at(k, &surf, on_side(&dom, s, i as f64 / SAMPLES as f64)) {
                Some(p) => edge_distance(k, &b, picked, p) <= on,
                None => false,
            });
            if all {
                grow = Some(s);
            }
        }
        let Some(grow) = grow else { return Ok(Err(Error::Unsupported)) };
        // SMLib extends the whole surface; the face must be all of it.
        let natural = surf.natural_uv_domain_with_nurb(k)?;
        let (nl, nh, dl, dh) = (natural.min(), natural.max(), dom.min(), dom.max());
        let ptol = 1e-9 * 1f64.max((nh.x - nl.x).max(nh.y - nl.y));
        if (nl.x - dl.x).abs() > ptol || (nl.y - dl.y).abs() > ptol || (nh.x - dh.x).abs() > ptol || (nh.y - dh.y).abs() > ptol {
            return Ok(Err(Error::Unsupported));
        }
        let continuity = match how {
            Continuation::Tangent => Continuity::Tangent,
            Continuation::Curvature => Continuity::Curvature,
            Continuation::Natural => Continuity::Natural,
        };
        // The extension runs `distance` across the side, exactly (solidcore's one extension).
        let extended = match extended_by(k, &surf, &dom, grow, continuity, distance) {
            Ok(e) => e,
            Err(e) => return Ok(Err(e)),
        };
        // A cone or cylinder continued naturally stays one, as the analytic widened, else SMLib
        // gives it back its analytic kind where it can.
        let grown = match how {
            Continuation::Natural => cone_continued(k, &surf, &dom, grow, distance).filter(|g| same_patch(k, g, &extended, 10.0 * model_tol)),
            _ => None,
        };
        let grown = match grown {
            Some(g) => g,
            None => match extended.copy_and_add_analytics(k) {
                Ok(g) => g,
                Err(e) => return Ok(Err(status(e))),
            },
        };
        let bx = grown.natural_uv_domain_with_nurb(k)?;
        let mut r = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
        r.set_tolerance(b.tolerance());
        let grown_eval = grown.copy(k)?;
        let nf = match build_faces(&mut r, |t, fenv| create_face_from_surface(k, t, Some(grown), &bx, fenv)) {
            Ok(nf) => nf,
            Err(e) => return Ok(Err(status(e))),
        };
        r.refresh_cached_topology();
        // The face and each side stand for the face and side they continue.
        copy_attributes(&b, EntityRef::Face(f), &mut r, EntityRef::Face(nf))?;
        let made = face_p4::edges(&r, nf, None, false, None)?;
        let new_on = 10.0 * r.tolerance().max(1e-9);
        for s in SIDES {
            let from = side_edge(k, &b, &surf, &dom, s, &edges, on);
            let to = side_edge(k, &r, &grown_eval, &bx, s, &made, new_on);
            if let (Some(from), Some(to)) = (from, to) {
                copy_attributes(&b, EntityRef::Edge(from), &mut r, EntityRef::Edge(to))?;
            }
        }
        // The new face faces the way the old one did.
        if let (Some(was), Some(now)) = (upward(&b, f)?, upward(&r, nf)?)
            && was != now
            && let Err(e) = face_p1::flip_face_orientation(&mut r, nf)
        {
            return Ok(Err(status(e)));
        }
        Ok(Ok(r))
    })?
    .map(Body::from_kernel)
}

/// A point inside each face, in the body's face order (`smlib_brep_face_points`:
/// `SmFace::CalculateAnInternalPoint`).
pub(crate) fn face_points(body: &Body) -> Result<Vec<[f64; 3]>> {
    super::with_body("smlib_brep_face_points", body, |k, b| {
        let faces = b.face_list().to_vec();
        faces.into_iter().map(|f| face_internal_point(k, b, f).map(|p| [p.x, p.y, p.z])).collect()
    })
}
