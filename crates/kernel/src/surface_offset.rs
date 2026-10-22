//! Offset surfaces (Surface › Offset): faces of a body copied out at a signed distance along
//! their normals, as a new surface body. Each face goes onto its surface's offset, SMLib's exact
//! offset (`smlib_offset_sheet`) of the face on its own; the offsets are then stitched where they
//! meet within the tolerance, so neighbouring faces moved to the same place along their shared
//! edge (meeting smoothly) keep sharing it, and across a sharp edge they come apart. Faces that
//! close up make a solid. An offset SMLib cannot make (one that would fold a face over) is its
//! error, never a fitted surface.

use crate::body::Body;
use crate::{KernelError, Result};

/// The picked faces of `body` (indices in its face order) offset by `distance` along their
/// outward normals (negative: against them) into one surface body. `tolerance`: how far apart
/// two offsets' edges may be and still be one edge. A distance of zero copies the faces. The
/// history is over `[body]`: each face is the offset of the one it came from.
pub fn offset_surface(body: &Body, faces: &[usize], distance: f64, tolerance: f64) -> Result<Body> {
    body.require_brep("offset")?;
    if !(distance.is_finite() && distance.abs() < 1e6) {
        return Err(KernelError::Invalid("the offset distance must be finite".into()));
    }
    if !(tolerance.is_finite() && (1e-6..=10.0).contains(&tolerance)) {
        return Err(KernelError::Invalid("the offset tolerance must be 0.000001…10 mm".into()));
    }
    let count = body.face_count();
    let mut picked: Vec<usize> = Vec::new();
    for &i in faces {
        if i >= count {
            return Err(KernelError::Invalid("no such face".into()));
        }
        if !picked.contains(&i) {
            picked.push(i);
        }
    }
    if picked.is_empty() {
        return Err(KernelError::Invalid("pick the faces to offset".into()));
    }
    crate::surfaces::offset_faces(body, &picked, distance, tolerance)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{box_solid, cylinder, measure, sphere};
    use solidcraft_geom::Vec3;

    fn rel(a: f64, b: f64) -> f64 {
        (a - b).abs() / b.abs().max(1e-9)
    }
    fn face_at(b: &Body, p: Vec3) -> usize {
        crate::surfaces::nearest_face(b, p).unwrap()
    }
    fn edges(b: &Body) -> usize {
        b.edge_count()
    }

    /// Planes move exactly: a box's top 5 up is a 10 × 10 square at z = 15. Two faces across a
    /// sharp edge come apart (8 edges, none shared); a zero distance copies.
    #[test]
    fn planes_move_and_sharp_edges_come_apart() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let top = face_at(&b, Vec3::new(5.0, 5.0, 10.0));
        let up = offset_surface(&b, &[top], 5.0, 0.01).unwrap();
        let m = measure(&up).unwrap();
        assert!(up.is_surface() && rel(m.area, 100.0) < 1e-9 && (m.bbox.min.z - 15.0).abs() < 1e-9, "{m:?}");
        let side = face_at(&b, Vec3::new(5.0, 0.0, 5.0));
        let two = offset_surface(&b, &[top, side], 1.0, 0.01).unwrap();
        assert!(two.face_count() == 2 && edges(&two) == 8);
        let copy = offset_surface(&b, &[top, side], 0.0, 0.01).unwrap();
        assert!(copy.face_count() == 2 && rel(measure(&copy).unwrap().area, 200.0) < 1e-9);
    }

    /// A cylinder's side (two halves) moved out by 2 keeps its halves joined, with radius 12:
    /// area 2π·12·20. Moved in past its axis, refused.
    #[test]
    fn a_cylinder_side_changes_radius() {
        let c = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 20.0).unwrap();
        let d = 10.0 / 2f64.sqrt();
        let halves = [face_at(&c, Vec3::new(d, d, 10.0)), face_at(&c, Vec3::new(-d, -d, 10.0))];
        let out = offset_surface(&c, &halves, 2.0, 0.01).unwrap();
        assert!(out.face_count() == 2);
        let want = 2.0 * std::f64::consts::PI * 12.0 * 20.0;
        assert!(rel(measure(&out).unwrap().area, want) < 1e-3, "{} vs {want}", measure(&out).unwrap().area); // (measured on meshes)
        let joined = offset_surface(&c, &halves, 0.0, 0.01).unwrap();
        assert_eq!(edges(&out), edges(&joined), "the halves still share their seams");
        assert!(offset_surface(&c, &halves, -10.5, 0.01).is_err());
    }

    /// A sphere (fitted faces) moved out by 2: area 4π·12², within the tolerance; moved in to
    /// its centre, refused.
    #[test]
    fn a_sphere_is_fitted_and_refused_past_its_centre() {
        let s = sphere(Vec3::ZERO, 10.0).unwrap();
        let all: Vec<usize> = (0..s.face_count()).collect();
        let out = offset_surface(&s, &all, 2.0, 0.01).unwrap();
        let want = 4.0 * std::f64::consts::PI * 144.0;
        assert!(rel(measure(&out).unwrap().area, want) < 2e-3, "{} vs {want}", measure(&out).unwrap().area); // (measured on meshes)
        // The patches meet smoothly: their offsets still share every edge, and close up.
        assert_eq!(edges(&out), edges(&s));
        assert!(!out.is_surface(), "the offset sphere closes into a solid");
        assert!(rel(measure(&out).unwrap().volume, 4.0 / 3.0 * std::f64::consts::PI * 1728.0) < 2e-3, "outward"); // (measured on meshes)
        assert!(offset_surface(&s, &all, -10.0, 0.01).is_err());
    }

    /// A doubly curved patch (a saddle, a B-spline height field) moved by 1.5: every point of the
    /// offset lies 1.5 from the patch, within the tolerance.
    #[test]
    fn a_bspline_patch_is_offset_within_the_tolerance() {
        let b = box_solid(Vec3::new(-14.0, -14.0, 0.0), Vec3::new(14.0, 14.0, 20.0)).unwrap();
        let tool = cylinder(Vec3::new(0.0, -60.0, 50.0), Vec3::Y, 40.0, 120.0).unwrap();
        let cup = crate::boolean(&b, &tool, crate::BoolOp::Cut).unwrap().unwrap();
        let z = |x: f64| 50.0 - (1600.0 - x * x).sqrt();
        let rim = [Vec3::new(14.0, 0.0, z(14.0)), Vec3::new(0.0, 14.0, z(0.0)), Vec3::new(-14.0, 0.0, z(-14.0)), Vec3::new(0.0, -14.0, z(0.0))];
        let cover = crate::patch_edges(&cup, &rim).unwrap();
        let record = cover.record();
        let s = solidcraft_geom::brep::eval::SurfaceEval::new(&record.faces[0].surface).unwrap();
        let out = offset_surface(&cover, &[0], 1.5, 0.01).unwrap();
        let mesh = out.mesh(&crate::MeshTolerance::chord(0.05)).unwrap();
        for p in mesh.positions.iter().step_by(7) {
            let uv = s.project(*p, None, f64::INFINITY).unwrap();
            let dist = s.point(uv).unwrap().dist(*p);
            assert!((dist - 1.5).abs() < 0.03, "{dist} at {p:?}");
        }
    }
}
