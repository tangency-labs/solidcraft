//! Centroids of one body's planar faces that lie back-to-back on a planar face of
//! another: the contact a flush join overlaps so the boolean can finish.
//!
//! Not upstream. Applications must not compute this themselves (solidcore/docs/layers.md
//! §4); they call [`flush_contact_points`].

use crate::kernel::Kernel;
use crate::mass::integrator::properties::face_centroid;
use crate::query::api_queries::{brep_bounding_box, face_normal_at_edge, get_faces};
use crate::topology::face::{self, p2::outer_faceuse};
use crate::topology::Brep;
use crate::{Point3, Result, Vec3};

/// Back-to-back contacts returned in one call. More than this is left to the caller.
pub const MAX_CONTACTS: usize = 8;

/// A planar face's outward unit normal and the centroid of its area.
struct PlanarFace {
    normal: Vec3,
    centroid: Point3,
}

/// Whether `tool` lies back-to-back on `target`: opposite unit normals, and the tool
/// centroid is within `plane_tol` of the target's plane.
#[must_use]
pub fn faces_are_flush_contact(
    tool_normal: Vec3,
    tool_centroid: Point3,
    target_normal: Vec3,
    target_centroid: Point3,
    plane_tol: f64,
) -> bool {
    tool_normal.dot(target_normal) < -1.0 + 1e-4
        && target_normal
            .dot(Vec3::between(target_centroid, tool_centroid))
            .abs()
            < plane_tol
}

/// The span of `body` used to set the plane tolerance: the tight bounding box's
/// diagonal, when that is finite and positive.
fn body_span(k: &Kernel, body: &Brep) -> Option<f64> {
    let bounds = brep_bounding_box(k, body, true).ok()?;
    let size = (bounds.max() - bounds.min()).length();
    (size.is_finite() && size > 0.0).then_some(size)
}

/// Planar faces of `body` with a finite outward normal, a finite centroid, and area at
/// least 1e-4. A face the kernel cannot describe is skipped, not a failure of the body.
fn planar_faces(k: &mut Kernel, body: &mut Brep) -> Result<Vec<PlanarFace>> {
    let Ok(faces) = get_faces(body) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for f in faces {
        let Some(surface) = face::surface(body, f)? else {
            continue;
        };
        let Ok(planarity) = surface.is_planar(k, face::tolerance(body, f)?) else {
            continue;
        };
        if !planarity.is_planar {
            continue;
        }
        let Some(&e) = face::edges(body, f, None, false, None)?.first() else {
            continue;
        };
        let Some(fu) = outer_faceuse(body, f)? else {
            continue;
        };
        let Ok(frame) = face_normal_at_edge(k, body, e, f, fu) else {
            continue;
        };
        let normal = frame.normal.value;
        if !normal.x.is_finite() || !normal.y.is_finite() || !normal.z.is_finite() {
            continue;
        }
        let Ok((area, centroid)) = face_centroid(k, body, f, 1e-8) else {
            continue;
        };
        if area < 1e-4
            || !area.is_finite()
            || !centroid.x.is_finite()
            || !centroid.y.is_finite()
            || !centroid.z.is_finite()
        {
            continue;
        }
        out.push(PlanarFace { normal, centroid });
    }
    Ok(out)
}

/// Centroids of `tool`'s planar faces that lie back-to-back on a planar face of
/// `target`, at most [`MAX_CONTACTS`]. Empty when either body has no usable size or
/// no such contact.
///
/// # Errors
///
/// Those of listing the faces or of a face query that is not skipped.
pub fn flush_contact_points(
    k: &mut Kernel,
    target: &mut Brep,
    tool: &mut Brep,
) -> Result<Vec<Point3>> {
    let Some(size) = body_span(k, target)
        .zip(body_span(k, tool))
        .map(|(a, b)| a.max(b))
    else {
        return Ok(Vec::new());
    };
    if !(size.is_finite() && size > 0.0 && size < 1e7) {
        return Ok(Vec::new());
    }
    let plane_tol = (size * 1e-4).max(1e-3);
    let tool_faces = planar_faces(k, tool)?;
    let target_faces = planar_faces(k, target)?;
    let mut points = Vec::new();
    for tool_face in &tool_faces {
        if points.len() >= MAX_CONTACTS {
            break;
        }
        let meets = target_faces.iter().any(|other| {
            faces_are_flush_contact(
                tool_face.normal,
                tool_face.centroid,
                other.normal,
                other.centroid,
                plane_tol,
            )
        });
        if meets {
            points.push(tool_face.centroid);
        }
    }
    Ok(points)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opposite_coplanar_faces_are_a_flush_contact() {
        let n_tool = Vec3::new(0.0, -1.0, 0.0);
        let n_target = Vec3::new(0.0, 1.0, 0.0);
        let tool_c = Point3::new(5.0, 10.0, 5.0);
        let target_c = Point3::new(5.0, 10.0, 5.0);
        assert!(faces_are_flush_contact(
            n_tool, tool_c, n_target, target_c, 1e-3
        ));
        // Same-direction faces are not back-to-back.
        assert!(!faces_are_flush_contact(
            n_target, tool_c, n_target, target_c, 1e-3
        ));
        // A centroid a centimetre off the plane is not a flush contact at 1e-3 mm.
        let off = Point3::new(5.0, 10.1, 5.0);
        assert!(!faces_are_flush_contact(
            n_tool, off, n_target, target_c, 1e-3
        ));
    }
}
