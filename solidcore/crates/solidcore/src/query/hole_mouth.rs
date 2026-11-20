//! (not upstream) How high a hole's entry face stands over its rim. A plane reaches
//! `r |n x d| / |n . d|` within `r` of the axis; a convex cylinder, cone or sphere lies on or
//! below its tangent plane at the entry point, so that plane's height bounds it.

use crate::error::{EntityRef, Result, SolidError};
use crate::geom::tol::{brep_zone_tol_3d, SmallTopoType};
use crate::intersect::point_class::PointClassType;
use crate::kernel::Kernel;
use crate::mass::integrator::properties::region_material;
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::math::MODEL_TOL;
use crate::query::brep::{self as qbrep, BrepClassifier};
use crate::query::face::p1::point_on_face_at;
use crate::query::normal::{certify, Sense};
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::face::p2::{self as fp2, face_side_regions};
use crate::topology::{edge, vertex, Brep, FaceId, RegionId};

const OP: &str = "query::hole_mouth";

/// The height a plane with outward unit `normal` reaches above the plane across the axis `dir`,
/// within `radius` of it. `None` when the axis does not enter it; a tilt within `normal_bound`
/// plus [`MODEL_TOL`] counts as parallel, since the kernel takes those directions as the same.
pub fn plane_lip(normal: Vec3, normal_bound: f64, dir: Vec3, radius: f64) -> Result<Option<f64>> {
    let d = dir.unitize()?;
    let into = -normal.dot(d);
    let across = normal.cross(d).length();
    if !(into.is_finite() && across.is_finite() && normal_bound.is_finite() && radius >= 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "the normal, its bound or the radius is not usable",
        ));
    }
    if into <= normal_bound + MODEL_TOL {
        return Ok(None);
    }
    let lip = radius * across / into;
    if !lip.is_finite() {
        return Err(SolidError::invalid_input(OP, "the radius is not finite"));
    }
    Ok(Some(lip))
}

/// The greatest [`plane_lip`] of the faces of `b` the axis `dir` enters at `spot` (one face, or
/// those of its edge or vertex): exact for a planar face, the tangent plane's upper bound for a
/// convex cylinder, cone or sphere. A sheet face is entered from either side; any other face, or
/// one with no normal at `spot` (an apex, a pole), is [`SolidError::Unsupported`].
pub fn hole_mouth_lip(
    k: &Kernel,
    b: &mut Brep,
    spot: Point3,
    dir: Vec3,
    radius: f64,
) -> Result<Option<f64>> {
    let tol = brep_zone_tol_3d(k, b, SmallTopoType::NormalSize);
    let at = BrepClassifier::of_brep(b).point_3d_classify(k, spot, tol, true)?;
    let faces = match (at.point_class(), at.object()) {
        (PointClassType::Face, Some(EntityRef::Face(f))) => vec![f],
        (PointClassType::Edge, Some(EntityRef::Edge(e))) => edge::faces(b, e, None)?,
        (PointClassType::Vertex, Some(EntityRef::Vertex(v))) => vertex::faces(b, v)?,
        _ => return Ok(None),
    };
    let mut highest = None;
    for f in faces {
        if let Some(lip) = face_lip(k, b, f, spot, tol, dir, radius)? {
            highest = Some(highest.map_or(lip, |h: f64| h.max(lip)));
        }
    }
    Ok(highest)
}

fn face_lip(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    spot: Point3,
    tol: f64,
    dir: Vec3,
    radius: f64,
) -> Result<Option<f64>> {
    let (Some(x), Some(y)) = fp2::faceuses(b, f)? else {
        return Err(SolidError::invalid_input(OP, "a face without its two uses"));
    };
    let material = |r: Option<RegionId>| r.map_or(Ok(false), |r| region_material(b, r));
    let [rx, ry] = face_side_regions(b, f)?;
    // the use that faces out of the material; a sheet has two, a partition none
    let (out, sheet) = match (material(rx)?, material(ry)?) {
        (true, true) => return Ok(None),
        (true, false) => (y, false),
        (false, behind) => (x, !behind),
    };
    let (sign, sense) = match b.faceuse(out)?.orientation {
        OrientType::Same => (1.0, Sense::Surface),
        OrientType::Opposite => (-1.0, Sense::Reversed),
        OrientType::Unknown => {
            return Err(SolidError::Unsupported {
                operation: OP,
                reason: "a faceuse whose orientation is unknown has no proved sense",
            })
        }
    };
    let Some((uv, on)) = point_on_face_at(k, b, f, spot, tol)? else {
        return Err(SolidError::invalid_input(
            OP,
            "the entry point does not drop onto its face",
        ));
    };
    let face_tol = fp2::tolerance(b, f)?;
    let surface =
        fp2::surface(b, f)?.ok_or_else(|| SolidError::invalid_input(OP, "face without surface"))?;
    let normal = qbrep::evaluate_surface_normal(k, Some(surface), uv)
        .and_then(Vec3::unitize)
        .and_then(|n| certify(k, surface, uv, n * sign, sense));
    let n = match normal {
        Err(SolidError::Degenerate { .. }) => {
            return Err(SolidError::Unsupported {
                operation: OP,
                reason: "a hole entering a face at a point where it has no normal",
            })
        }
        n => n?,
    };
    let (lip, side) = match plane_lip(n.value, n.bound, dir, radius)? {
        None if sheet => (plane_lip(-n.value, n.bound, dir, radius)?, -n.value),
        lip => (lip, n.value),
    };
    if lip.is_none() || surface.is_planar(k, face_tol)?.is_planar || convex(surface, on, side) {
        return Ok(lip);
    }
    Err(SolidError::Unsupported {
        operation: OP,
        reason:
            "a hole entering a face that is neither planar nor a convex cylinder, cone or sphere",
    })
}

/// Whether `outward` at `on` points away from the axis or centre: under every tangent plane.
fn convex(surface: &Surface, on: Point3, outward: Vec3) -> bool {
    match surface.kind() {
        SurfaceKind::Cone(c) => {
            let (o, a) = (c.rev.position.origin(), c.rev.position.z_axis());
            let v = Vec3::between(o, on);
            (v - a * v.dot(a)).dot(outward) > 0.0
        }
        SurfaceKind::Sphere(s) => Vec3::between(s.base.position.origin(), on).dot(outward) > 0.0,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::plane_lip;
    use crate::math::Vec3;

    /// The outward normal of a face rising at `angle` towards the direction `turn` from +x.
    fn tilted(angle: f64, turn: f64) -> Vec3 {
        Vec3::new(
            -angle.sin() * turn.cos(),
            -angle.sin() * turn.sin(),
            angle.cos(),
        )
    }

    #[test]
    fn a_tilted_face_lifts_the_mouth_to_the_rim_height() {
        let (angle, radius) = (30f64.to_radians(), 3.0);
        let lip = plane_lip(tilted(angle, 0.0), 0.0, Vec3::new(0.0, 0.0, -1.0), radius)
            .unwrap()
            .expect("a lip");
        let want = radius * angle.tan();
        assert!((lip - want).abs() < 1e-9 * want, "lip {lip} vs {want}");
    }

    /// Uphill between two of the eight rim points the lip was once probed at.
    #[test]
    fn a_face_tilted_between_the_rim_points_lifts_the_mouth_to_the_rim_height() {
        let (angle, turn, radius) = (30f64.to_radians(), 22.5f64.to_radians(), 3.0);
        let lip = plane_lip(tilted(angle, turn), 0.0, Vec3::new(0.0, 0.0, -2.0), radius)
            .unwrap()
            .expect("a lip");
        let want = radius * angle.tan();
        assert!((lip - want).abs() < 1e-9 * want, "lip {lip} vs {want}");
    }

    #[test]
    fn a_flat_face_has_no_lip_and_a_face_not_entered_has_none() {
        let down = Vec3::new(0.0, 0.0, -1.0);
        assert_eq!(plane_lip(Vec3::Z, 0.0, down, 3.0).unwrap(), Some(0.0));
        assert_eq!(plane_lip(Vec3::Z, 0.0, Vec3::Z, 3.0).unwrap(), None);
        assert_eq!(plane_lip(Vec3::X, 0.0, down, 3.0).unwrap(), None);
        assert!(plane_lip(Vec3::Z, 0.0, Vec3::ZERO, 3.0).is_err());
        assert!(plane_lip(Vec3::Z, 0.0, down, -1.0).is_err());
    }
}
