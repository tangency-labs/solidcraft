//! Certify a whole protrusion above a planar shoulder and cut it away exactly.
//!
//! Every retained face is planar. Edges not incident to a consumed face either
//! stay below the shoulder, proved by positive rational control hulls, or are
//! normal seams between persistent supports ending on the consumed cap. Thus
//! the protrusion has no unrelated surviving geometry above its shoulder. The
//! cutter's other five planes lie outside a hull of the source geometry. No recursion.
//!
//! Upstream: none (regularized inner offsets).

use std::collections::BTreeSet;

use super::consumed::Plane;
use crate::boolean::api_brep::boolean_difference;
use crate::curve::curve::CurveKind;
use crate::error::Result;
use crate::geom::axis2_placement::Axis2Placement;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::nurbs::math::{gamma, CPoint, NOW};
use crate::primitive::api_primitives::create_box;
use crate::surface::surface::SurfaceKind;
use crate::topology::brep::{p1 as bp1, p3 as bp3, p5 as bp5};
use crate::topology::edge;
use crate::topology::face::{p2 as face2, p4 as face4};
use crate::topology::{vertex, Brep, EdgeId, FaceId};

const OP: &str = "offset::offset_executive::clip";

fn point(p: CPoint) -> Option<Point3> {
    if p.w != NOW && !(p.w > 0.0 && p.w.is_finite()) {
        return None;
    }
    let q = p.to_euclid();
    (q.x.is_finite() && q.y.is_finite() && q.z.is_finite()).then_some(q)
}

fn projection(q: Point3, origin: Point3, direction: Vec3) -> (f64, f64) {
    let d = direction.dot(q - origin);
    let magnitude = direction.x.abs() * (q.x.abs() + origin.x.abs())
        + direction.y.abs() * (q.y.abs() + origin.y.abs())
        + direction.z.abs() * (q.z.abs() + origin.z.abs());
    let rounding = gamma(32) * magnitude;
    (d - rounding, d + rounding)
}

fn edge_hull(
    k: &Kernel,
    b: &Brep,
    e: EdgeId,
    visit: &mut impl FnMut(Point3) -> bool,
) -> Result<bool> {
    let Some(c) = edge::create_trimmed_nurbs_curve(k, b, e)? else {
        return Ok(false);
    };
    let CurveKind::BSpline(c) = c.kind() else {
        return Ok(false);
    };
    let Some(net) = c.gw_nurb() else {
        return Ok(false);
    };
    if net.cpts().is_empty() {
        return Ok(false);
    }
    for &p in net.cpts() {
        let Some(q) = point(p) else { return Ok(false) };
        if !visit(q) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn face_hull(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    visit: &mut impl FnMut(Point3) -> bool,
) -> Result<bool> {
    let Some(surface) = b.face(f)?.surface.as_ref() else {
        return Ok(false);
    };
    if matches!(surface.kind(), SurfaceKind::Plane(_)) {
        let edges = face4::edges(b, f, None, false, None)?;
        if edges.is_empty() {
            return Ok(false);
        }
        for e in edges {
            if !edge_hull(k, b, e, visit)? {
                return Ok(false);
            }
        }
        return Ok(true);
    }
    if !surface.is_analytic()? {
        return Ok(false);
    }
    let Some(nurb) = bp3::with_nurb(k, surface)? else {
        return Ok(false);
    };
    let uv = face2::uv_domain(b, f)?;
    let natural = nurb.natural_uv_domain();
    if !(uv.min().x >= natural.min().x
        && uv.max().x <= natural.max().x
        && uv.min().y >= natural.min().y
        && uv.max().y <= natural.max().y)
    {
        return Ok(false);
    }
    let patch = nurb.copy_sub_patch(&uv)?;
    let Some(net) = patch.gw_nurb() else {
        return Ok(false);
    };
    let mut any = false;
    for &p in net.rows().flatten() {
        any = true;
        let Some(q) = point(p) else { return Ok(false) };
        if !visit(q) {
            return Ok(false);
        }
    }
    Ok(any)
}

/// An intersection seam of persistent planar walls can continue above the root
/// until it terminates on the consumed cap. Its whole line is parallel to the
/// shoulder normal, so it introduces no separate shoulder or protrusion there.
fn normal_seam(b: &Brep, e: EdgeId, consumed: &BTreeSet<FaceId>, shoulder: Plane) -> Result<bool> {
    let sides = edge::faces(b, e, None)?;
    if sides.len() != 2 {
        return Ok(false);
    }
    for face in sides {
        if consumed.contains(&face) {
            return Ok(false);
        }
        let Some(surface) = b.face(face)?.surface.as_ref() else {
            return Ok(false);
        };
        let SurfaceKind::Plane(plane) = surface.kind() else {
            return Ok(false);
        };
        // Independently intersected supports retain direction roundoff. Bound
        // normalized dot/cross products at arithmetic scale, not model distance.
        if plane
            .evaluate_normal()
            .unitize()?
            .dot(shoulder.normal)
            .abs()
            > gamma(64)
        {
            return Ok(false);
        }
    }
    let Some(curve) = b.edge(e)?.curve.as_ref() else {
        return Ok(false);
    };
    let CurveKind::Line(line) = curve.kind() else {
        return Ok(false);
    };
    if line.line_vector().cross(shoulder.normal).length() > gamma(64) * line.line_vector().length()
    {
        return Ok(false);
    }
    let (Some(a), Some(c)) = (edge::start_vertex(b, e)?, edge::end_vertex(b, e)?) else {
        return Ok(false);
    };
    let pa = b.vertex(a)?.point;
    let pc = b.vertex(c)?.point;
    let ra = projection(pa, shoulder.origin, shoulder.normal);
    let rc = projection(pc, shoulder.origin, shoulder.normal);
    let (below, above, top) = if ra.1 <= rc.1 {
        (ra, rc, c)
    } else {
        (rc, ra, a)
    };
    if !(below.1.is_finite()
        && above.0.is_finite()
        && below.1 <= b.tolerance()
        && above.0 > b.tolerance())
    {
        return Ok(false);
    }
    Ok(vertex::faces(b, top)?.iter().any(|f| consumed.contains(f)))
}

/// A certified source cut, or `None` when surviving geometry cannot be proved to
/// stay below this shoulder. `consumed` is already proved to form thin protrusions.
pub(super) fn clip(
    k: &mut Kernel,
    source: &Brep,
    consumed: &BTreeSet<FaceId>,
    shoulder: Plane,
) -> Result<Option<Brep>> {
    let tolerance = source.tolerance();
    for f in bp1::faces(source, None)? {
        if consumed.contains(&f) {
            continue;
        }
        let Some(surface) = source.face(f)?.surface.as_ref() else {
            return Ok(None);
        };
        let SurfaceKind::Plane(plane) = surface.kind() else {
            return Ok(None);
        };
        // Only normal-extruded support walls may survive above this shoulder.
        // A different elevated shoulder must not be cut just because consumed
        // ribs happen to surround every edge of its boundary.
        if plane
            .evaluate_normal()
            .unitize()?
            .dot(shoulder.normal)
            .abs()
            > gamma(64)
            && !face_hull(k, source, f, &mut |q| {
                let (_, upper) = projection(q, shoulder.origin, shoulder.normal);
                upper.is_finite() && upper <= tolerance
            })?
        {
            return Ok(None);
        }
    }
    for e in bp1::edges(source, None)? {
        k.check_cancelled(OP)?;
        if edge::faces(source, e, None)?
            .iter()
            .any(|f| consumed.contains(f))
        {
            continue;
        }
        if normal_seam(source, e, consumed, shoulder)? {
            continue;
        }
        if !edge_hull(k, source, e, &mut |q| {
            let (_, upper) = projection(q, shoulder.origin, shoulder.normal);
            upper.is_finite() && upper <= tolerance
        })? {
            return Ok(None);
        }
    }
    let n = shoulder.normal.unitize()?;
    let reference = if n.x.abs() <= n.y.abs() && n.x.abs() <= n.z.abs() {
        Vec3::X
    } else if n.y.abs() <= n.z.abs() {
        Vec3::Y
    } else {
        Vec3::Z
    };
    let u = n.cross(reference).unitize()?;
    let v = n.cross(u).unitize()?;
    let mut lower = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut upper = Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for f in bp1::faces(source, None)? {
        k.check_cancelled(OP)?;
        if !face_hull(k, source, f, &mut |q| {
            let (a, b) = projection(q, shoulder.origin, u);
            let (c, d) = projection(q, shoulder.origin, v);
            let (e, f) = projection(q, shoulder.origin, n);
            if ![a, b, c, d, e, f].iter().all(|x| x.is_finite()) {
                return false;
            }
            lower.x = lower.x.min(a);
            lower.y = lower.y.min(c);
            lower.z = lower.z.min(e);
            upper.x = upper.x.max(b);
            upper.y = upper.y.max(d);
            upper.z = upper.z.max(f);
            true
        })? {
            return Ok(None);
        }
    }
    let reach = (upper - lower).length().max(tolerance);
    let low = lower - Vec3::new(reach, reach, 0.0);
    let length = upper.x - lower.x + 2.0 * reach;
    let width = upper.y - lower.y + 2.0 * reach;
    let height = upper.z + reach;
    if ![reach, length, width, height]
        .iter()
        .all(|x| x.is_finite() && *x > 0.0)
    {
        return Ok(None);
    }
    let mut cutter = create_box(k, Point3::new(low.x, low.y, 0.0), length, width, height)?;
    let placement = Axis2Placement::from_stored(shoulder.origin, u, v);
    bp5::transform(k, &mut cutter, &placement, None, false)?;
    Ok(Some(boolean_difference(
        k,
        source.clone_standing_for_self()?,
        cutter,
    )?))
}
