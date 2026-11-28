//! Discard planar offset regions whose straight boundaries have collapsed to a line.
//!
//! Positive rational curve hulls enclose the entire trimmed boundary. Every hull
//! point must be collinear, allowing only arithmetic roundoff also below the B-rep
//! tolerance. This does not use integrated area or a general sliver threshold.
//! Upstream: none. Deviations: one finite face and boundary walk; no recursion.

use super::p2::OffsetNotifications;
use crate::curve::curve::CurveKind;
use crate::error::Result;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::nurbs::math::{gamma, NOW};
use crate::surface::surface::SurfaceKind;
use crate::topology::brep::{p1 as bp1, p5 as bp5};
use crate::topology::edge::{self, NotificationStores, NotifyState};
use crate::topology::face::p4 as face4;
use crate::topology::{Brep, FaceId};

const OP: &str = "offset::offset_executive::empty";

fn abs(p: Point3) -> Vec3 {
    Vec3::new(p.x.abs(), p.y.abs(), p.z.abs())
}

fn collapsed(k: &Kernel, b: &Brep, f: FaceId) -> Result<bool> {
    if !b
        .face(f)?
        .surface
        .as_ref()
        .is_some_and(|s| matches!(s.kind(), SurfaceKind::Plane(_)))
    {
        return Ok(false);
    }
    let mut points = Vec::new();
    for e in face4::edges(b, f, None, false, None)? {
        // Arbitrarily curved boundaries need arrangement, even if their end
        // vertices coincide. Restrict this certificate to analytic straight edges.
        if !b
            .edge(e)?
            .curve
            .as_ref()
            .is_some_and(|c| matches!(c.kind(), CurveKind::Line(_)))
        {
            return Ok(false);
        }
        let Some(curve) = edge::create_trimmed_nurbs_curve(k, b, e)? else {
            return Ok(false);
        };
        let CurveKind::BSpline(curve) = curve.kind() else {
            return Ok(false);
        };
        let Some(net) = curve.gw_nurb() else {
            return Ok(false);
        };
        if net.cpts().is_empty() {
            return Ok(false);
        }
        for &p in net.cpts() {
            if p.w != NOW && !(p.w > 0.0 && p.w.is_finite()) {
                return Ok(false);
            }
            let q = p.to_euclid();
            if ![q.x, q.y, q.z].iter().all(|x| x.is_finite()) {
                return Ok(false);
            }
            points.push(q);
        }
    }
    let Some(&a) = points.first() else {
        return Ok(false);
    };
    let mut endpoint = a;
    let mut length = 0.0_f64;
    for &q in &points {
        let d = (q - a).length();
        if !d.is_finite() {
            return Ok(false);
        }
        if d > length {
            length = d;
            endpoint = q;
        }
    }
    if length == 0.0 {
        return Ok(points.iter().all(|&q| q == a));
    }
    let direction = endpoint - a;
    let v = abs(endpoint) + abs(a);
    for &q in &points {
        let cross = direction.cross(q - a);
        let w = abs(q) + abs(a);
        // Homogeneous division, two subtractions and the cross product. The
        // magnitude expression also covers cancellation at a large world origin.
        let round = Vec3::new(
            v.y * w.z + v.z * w.y,
            v.z * w.x + v.x * w.z,
            v.x * w.y + v.y * w.x,
        ) * gamma(64);
        if !(round.x.is_finite()
            && round.y.is_finite()
            && round.z.is_finite()
            && cross.x.abs() <= round.x
            && cross.y.abs() <= round.y
            && cross.z.abs() <= round.z
            && cross.length() / length <= b.tolerance())
        {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn remove(
    k: &mut Kernel,
    b: &mut Brep,
    notifications: &mut OffsetNotifications<'_>,
) -> Result<()> {
    let mut faces = Vec::new();
    for f in bp1::faces(b, None)? {
        k.check_cancelled(OP)?;
        if collapsed(k, b, f)? {
            faces.push(f);
        }
    }
    if faces.is_empty() {
        return Ok(());
    }
    let mut stores = NotificationStores {
        records: &mut *notifications.records,
        attributes: &mut *notifications.attributes,
        key_of: notifications.result_key_of,
    };
    NotifyState::from_kernel_stores(&mut stores).with_kernel_env(k, |k, env| {
        let editing = b.editing_enabled();
        bp5::remove_faces_notified(k, b, editing, &faces, env)
    })?;
    b.refresh_cached_topology();
    Ok(())
}
