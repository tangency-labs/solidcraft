//! A whole-turn cylinder wall of a body, as a cell grid on it needs it: its axis, the frame's
//! angle zero, which side its material lies on, its two ends along the axis and whether each is a
//! planar cap square to the axis, and the radius of the other surface of the wall where one
//! faces it over the whole wall (a tube's bore under its outside, or its outside over its bore).
//!
//! The face's kind and frame are its record's ([`record_face`]): the wall's angle zero is the
//! record's `Surface::Cylinder` frame x axis, so a grid anchored on it lands where the record
//! says. The material side is read at the middle of the face's parameter box from the surface's
//! normal turned by the upward face use ([`material_toward_axis`]); the facing surface is the one
//! [`facing_cylinders`] pairs it with.
//!
//! What is refused, each as [`CellGridError::Parameter`] with a message for the user:
//! - a face that is not a cylinder (a cone gets its own message: cones are a later step);
//! - a wall that does not go all the way round: the face is one loop of two whole-turn rims (each
//!   a closed edge) and one seam (an edge both of whose uses are on the face);
//! - a wall with openings or split: an inner loop, or another face on the same cylinder.
//!
//! Not upstream: SolidCraft's cell grid on a wall (issue #23010), no SMLib counterpart.
//! Deviations: none. No recursion.

use crate::cell_grid::{CellGridError, CylinderWall};
use crate::eval::Surface as Record;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::Point2;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::brep::{evaluate_surface_normal, evaluate_surface_point};
use crate::query::edge::{edge_faces, edge_vertices};
use crate::surface::face_pair_cylinder::{facing_cylinders, material_toward_axis, CylinderFace};
use crate::surface::face_surface::record_face;
use crate::topology::{Brep, EdgeId, FaceId};

/// The refusal for a face with openings, or one of several faces on one cylinder.
const SPLIT: &str = "the wall has openings or is split; not supported yet";
/// The refusal for a face that is a part of a turn.
const PARTIAL: &str = "a wall that does not go all the way round is not supported yet";

fn refuse(reason: &str) -> CellGridError {
    CellGridError::Parameter(reason.to_string())
}

/// A kernel failure while reading the wall, as the cell grid's refusal.
fn kernel_err(e: crate::error::SolidError) -> CellGridError {
    CellGridError::Parameter(format!("the wall could not be read: {e}"))
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn arr(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// What a record label is, for the refusal message.
fn kind_name(s: &Record) -> &'static str {
    match s {
        Record::Plane { .. } | Record::PlaneThrough { .. } => "plane",
        Record::Cylinder { .. } => "cylinder",
        Record::Cone { .. } => "cone",
        Record::Sphere { .. } | Record::BallPatch { .. } => "sphere",
        Record::Torus { .. } => "torus",
        Record::PlacedRevolution { .. } => "revolved surface",
        _ => "spline",
    }
}

/// The record label of face `f`: what the face is, with the record's own frame.
fn label(k: &Kernel, b: &Brep, f: FaceId) -> Result<Record, CellGridError> {
    let face = b.face(f).map_err(kernel_err)?;
    let surface = face
        .surface
        .as_ref()
        .ok_or_else(|| refuse("the picked face has no surface"))?;
    Ok(record_face(k, surface, &face.uv_domain)
        .map_err(kernel_err)?
        .surface)
}

/// A cylinder label's axis point, unit axis, unit x axis and radius.
fn cylinder_of(s: &Record) -> Option<(Vec3, Vec3, Vec3, f64)> {
    let Record::Cylinder { frame, radius } = s else {
        return None;
    };
    let (x, y) = (v3(frame.x_axis), v3(frame.y_axis));
    let axis = x.cross(y).unitize().ok()?;
    let x = x.unitize().ok()?;
    Some((v3(frame.origin), axis, x, *radius))
}

/// The unit normal of a plane label, if it is one.
fn plane_normal(s: &Record) -> Option<Vec3> {
    match s {
        Record::Plane { u_axis, v_axis, .. } => v3(*u_axis).cross(v3(*v_axis)).unitize().ok(),
        Record::PlaneThrough {
            origin,
            u_point,
            v_point,
        } => {
            let o = v3(*origin);
            (v3(*u_point) - o).cross(v3(*v_point) - o).unitize().ok()
        }
        _ => None,
    }
}

/// Whether face `f`'s material lies toward the axis, read at the middle of its parameter box
/// from its surface's normal turned out of the material by its upward face use.
fn toward_axis(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    axis_point: Vec3,
    axis: Vec3,
) -> Result<bool, CellGridError> {
    let face = b.face(f).map_err(kernel_err)?;
    let d = &face.uv_domain;
    let uv = Point2::new(
        0.5 * (d.u_min() + d.u_max()),
        0.5 * (d.v_min() + d.v_max()),
    );
    let surface = face.surface.as_ref();
    let point = evaluate_surface_point(k, surface, uv).map_err(kernel_err)?;
    let normal = evaluate_surface_normal(k, surface, uv).map_err(kernel_err)?;
    let fu = face
        .faceuse
        .ok_or_else(|| refuse("the picked face is not in a shell"))?;
    let same = b.faceuse(fu).map_err(kernel_err)?.orientation == OrientType::Same;
    let outward = if same { normal } else { normal * -1.0 };
    material_toward_axis(axis_point, axis, point, outward).map_err(kernel_err)
}

/// The distinct edges of face `f`'s loops.
fn face_edges(b: &Brep, f: FaceId) -> Result<Vec<EdgeId>, CellGridError> {
    let mut out: Vec<EdgeId> = Vec::new();
    for e in crate::topology::face::p4::edges(b, f, None, false, None).map_err(kernel_err)? {
        if !out.contains(&e) {
            out.push(e);
        }
    }
    Ok(out)
}

/// The lowest and highest of face `g`'s vertices along `axis` from `axis_point`.
fn axial_span(
    b: &Brep,
    g: FaceId,
    axis_point: Vec3,
    axis: Vec3,
) -> Result<Option<[f64; 2]>, CellGridError> {
    let mut span: Option<[f64; 2]> = None;
    for e in face_edges(b, g)? {
        for v in edge_vertices(b, e).map_err(kernel_err)? {
            let p: Point3 = b.vertex(v).map_err(kernel_err)?.point;
            let z = (p - axis_point).dot(axis);
            span = Some(match span {
                None => [z, z],
                Some([lo, hi]) => [lo.min(z), hi.max(z)],
            });
        }
    }
    Ok(span)
}

/// One rim of the wall: where it lies along the axis, and whether the face across it is a
/// planar cap square to the axis.
struct Rim {
    z: f64,
    open: bool,
}

/// The whole-turn cylinder wall that is face `face` of `b`, outside or bore, read to the length
/// tolerance `tol`.
///
/// # Errors
///
/// [`CellGridError::Parameter`] for a `tol` that is not finite and positive, a face that is not
/// a cylinder (its kind named; a cone has its own message), a wall that is not a whole turn, a
/// wall with openings or split into several faces, or a face the kernel cannot read.
pub fn cylinder_wall(
    k: &Kernel,
    b: &Brep,
    face: FaceId,
    tol: f64,
) -> Result<CylinderWall, CellGridError> {
    if !(tol.is_finite() && tol > 0.0) {
        return Err(refuse("the wall's tolerance must be finite and positive"));
    }
    let picked = label(k, b, face)?;
    let (axis_point, axis, x_dir, radius) = match &picked {
        Record::Cylinder { .. } => cylinder_of(&picked)
            .ok_or_else(|| refuse("the picked cylinder's frame is degenerate"))?,
        Record::Cone { .. } => {
            return Err(refuse(
                "the face is a cone; cone walls are not supported yet",
            ))
        }
        other => {
            return Err(refuse(&format!(
                "the picked face is a {}; pick a cylinder wall, outside or bore",
                kind_name(other)
            )))
        }
    };
    if !(radius.is_finite() && radius > 0.0) {
        return Err(refuse("the picked cylinder's radius must be finite and positive"));
    }

    // One loop: a second loop is an opening in the wall.
    if b.face(face).map_err(kernel_err)?.loops.len() != 1 {
        return Err(refuse(SPLIT));
    }

    // The loop: one seam (both uses on this face) and two whole-turn rims (closed edges).
    let mut seams = 0usize;
    let mut rims: Vec<Rim> = Vec::new();
    for e in face_edges(b, face)? {
        let faces = edge_faces(b, e).map_err(kernel_err)?;
        let others: Vec<FaceId> = faces.iter().copied().filter(|&g| g != face).collect();
        if others.is_empty() {
            seams += 1;
            continue;
        }
        let vertices = edge_vertices(b, e).map_err(kernel_err)?;
        if vertices.len() != 1 || others.len() != 1 {
            return Err(refuse(PARTIAL));
        }
        let p: Point3 = b.vertex(vertices[0]).map_err(kernel_err)?.point;
        let z = (p - axis_point).dot(axis);
        let ang = k.tolerances.ang_tol_rad().sin();
        let open = plane_normal(&label(k, b, others[0])?)
            .is_some_and(|n| n.cross(axis).length() <= ang);
        rims.push(Rim { z, open });
    }
    if seams != 1 || rims.len() != 2 {
        return Err(refuse(PARTIAL));
    }
    rims.sort_by(|a, c| a.z.total_cmp(&c.z));
    let (lo, hi) = (&rims[0], &rims[1]);
    if hi.z - lo.z <= tol {
        return Err(refuse(PARTIAL));
    }

    let convex = toward_axis(k, b, face, axis_point, axis)?;
    let this = CylinderFace {
        axis_point,
        axis,
        radius,
        material_toward_axis: convex,
    };

    // Every other face: on the same cylinder (a split wall), or the wall's other surface.
    let ang = k.tolerances.ang_tol_rad().sin();
    let mut facing: Option<f64> = None;
    for g in b.faces().ids() {
        if g == face {
            continue;
        }
        let Some((gp, ga, _, gr)) = cylinder_of(&label(k, b, g)?) else {
            continue;
        };
        let coaxial = axis.cross(ga).length() <= ang && (gp - axis_point).cross(ga).length() <= tol;
        if coaxial && (gr - radius).abs() <= tol {
            return Err(refuse(SPLIT));
        }
        if !coaxial {
            continue;
        }
        let other = CylinderFace {
            axis_point: gp,
            axis: ga,
            radius: gr,
            material_toward_axis: toward_axis(k, b, g, gp, ga)?,
        };
        if facing_cylinders(k, &this, &other)
            .map_err(kernel_err)?
            .is_none()
        {
            continue;
        }
        // It faces the wall only where it spans all of the wall's height.
        let Some([glo, ghi]) = axial_span(b, g, axis_point, axis)? else {
            continue;
        };
        if glo > lo.z + tol || ghi < hi.z - tol {
            continue;
        }
        // The nearest facing surface is the wall's other side.
        facing = Some(match facing {
            Some(f) if (f - radius).abs() <= (gr - radius).abs() => f,
            _ => gr,
        });
    }

    let tangent = if convex {
        axis.cross(x_dir)
    } else {
        x_dir.cross(axis)
    };
    Ok(CylinderWall {
        axis_point: arr(axis_point),
        axis: arr(axis),
        x_dir: arr(x_dir),
        tangent: arr(tangent),
        radius,
        convex,
        z_lo: lo.z,
        z_hi: hi.z,
        facing_radius: facing,
        open_lo: lo.open,
        open_hi: hi.open,
        tol,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boolean::api_brep::boolean_difference;
    use crate::primitive::api_primitives::{
        create_box, create_cone, create_cylinder, create_partial_cylinder, create_sphere,
    };

    const TOL: f64 = 1e-6;

    fn p(x: f64, y: f64, z: f64) -> Point3 {
        Point3::new(x, y, z)
    }

    /// A tube about z, outer radius 30, bore 27, from z = 0 to 40: a rod with a longer rod cut
    /// out of it.
    fn tube(k: &mut Kernel) -> Brep {
        let rod = create_cylinder(k, p(0.0, 0.0, 0.0), 30.0, 40.0).unwrap();
        let bore = create_cylinder(k, p(0.0, 0.0, -1.0), 27.0, 42.0).unwrap();
        boolean_difference(k, rod, bore).unwrap()
    }

    /// The face of `b` whose record label is a cylinder of `radius`.
    fn cylinder_face(k: &Kernel, b: &Brep, radius: f64) -> FaceId {
        b.faces()
            .ids()
            .find(|&f| {
                cylinder_of(&label(k, b, f).unwrap()).is_some_and(|c| (c.3 - radius).abs() < 1e-9)
            })
            .expect("a cylinder face of that radius")
    }

    /// The first face of `b` whose record label is not a cylinder and passes `pick`.
    fn face_where(k: &Kernel, b: &Brep, pick: impl Fn(&Record) -> bool) -> FaceId {
        b.faces()
            .ids()
            .find(|&f| pick(&label(k, b, f).unwrap()))
            .expect("a face of that kind")
    }

    fn message(r: Result<CylinderWall, CellGridError>) -> String {
        match r {
            Err(CellGridError::Parameter(m)) => m,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_tubes_outside_faces_its_bore_and_is_open_at_both_ends() {
        let mut k = Kernel::default();
        let b = tube(&mut k);
        let f = cylinder_face(&k, &b, 30.0);
        let w = cylinder_wall(&k, &b, f, TOL).unwrap();
        assert!(w.convex, "{w:?}");
        assert!((w.radius - 30.0).abs() < 1e-9, "{w:?}");
        let facing = w.facing_radius.expect("the bore faces the outside");
        assert!((facing - 27.0).abs() < 1e-9, "{w:?}");
        assert!(w.open_lo && w.open_hi, "{w:?}");
        assert!((w.z_hi - w.z_lo - 40.0).abs() < 1e-9, "{w:?}");
        // The axis is z either way round, through the origin's line.
        assert!((w.axis[2].abs() - 1.0).abs() < 1e-12, "{w:?}");
        let at = v3(w.axis_point);
        assert!(at.x.abs() < 1e-9 && at.y.abs() < 1e-9, "{w:?}");
        // The tangent on an outside is axis x x_dir, square to both.
        let (a, x, t) = (v3(w.axis), v3(w.x_dir), v3(w.tangent));
        assert!((a.cross(x) - t).length() < 1e-12, "{w:?}");
        assert!(x.dot(a).abs() < 1e-12 && (t.length() - 1.0).abs() < 1e-12, "{w:?}");
        assert_eq!(w.tol, TOL);
    }

    #[test]
    fn a_tubes_bore_faces_its_outside_and_turns_its_tangent_over() {
        let mut k = Kernel::default();
        let b = tube(&mut k);
        let f = cylinder_face(&k, &b, 27.0);
        let w = cylinder_wall(&k, &b, f, TOL).unwrap();
        assert!(!w.convex, "{w:?}");
        let facing = w.facing_radius.expect("the outside faces the bore");
        assert!((facing - 30.0).abs() < 1e-9, "{w:?}");
        assert!(w.open_lo && w.open_hi, "{w:?}");
        let (a, x, t) = (v3(w.axis), v3(w.x_dir), v3(w.tangent));
        assert!((x.cross(a) - t).length() < 1e-12, "{w:?}");
    }

    #[test]
    fn the_walls_angle_zero_is_the_record_frames_x_axis() {
        let mut k = Kernel::default();
        let b = tube(&mut k);
        for r in [30.0, 27.0] {
            let f = cylinder_face(&k, &b, r);
            let Record::Cylinder { frame, .. } = label(&k, &b, f).unwrap() else {
                panic!("not a cylinder");
            };
            let w = cylinder_wall(&k, &b, f, TOL).unwrap();
            let x = v3(frame.x_axis).unitize().unwrap();
            assert!((v3(w.x_dir) - x).length() < 1e-15, "{w:?} {frame:?}");
            assert_eq!(w.axis_point, frame.origin);
        }
    }

    #[test]
    fn a_rods_wall_faces_nothing() {
        let mut k = Kernel::default();
        let b = create_cylinder(&mut k, p(0.0, 0.0, 0.0), 30.0, 40.0).unwrap();
        let f = cylinder_face(&k, &b, 30.0);
        let w = cylinder_wall(&k, &b, f, TOL).unwrap();
        assert!(w.convex, "{w:?}");
        assert_eq!(w.facing_radius, None, "{w:?}");
        assert!(w.open_lo && w.open_hi, "{w:?}");
    }

    #[test]
    fn a_plane_a_sphere_and_a_cone_are_refused_by_their_kind() {
        let mut k = Kernel::default();
        let block = create_box(&mut k, p(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
        let f = face_where(&k, &block, |s| plane_normal(s).is_some());
        let m = message(cylinder_wall(&k, &block, f, TOL));
        assert!(m.contains("is a plane") && m.contains("pick a cylinder wall"), "{m}");

        let ball = create_sphere(&mut k, p(0.0, 0.0, 0.0), 10.0).unwrap();
        let f = face_where(&k, &ball, |s| matches!(s, Record::Sphere { .. }));
        let m = message(cylinder_wall(&k, &ball, f, TOL));
        assert!(m.contains("is a sphere"), "{m}");

        let cone = create_cone(&mut k, p(0.0, 0.0, 0.0), 20.0, 10.0, 30.0).unwrap();
        let f = face_where(&k, &cone, |s| matches!(s, Record::Cone { .. }));
        let m = message(cylinder_wall(&k, &cone, f, TOL));
        assert!(m.contains("is a cone") && m.contains("not supported yet"), "{m}");
    }

    #[test]
    fn a_half_wall_is_refused() {
        let mut k = Kernel::default();
        let half = create_partial_cylinder(&mut k, p(0.0, 0.0, 0.0), 30.0, 40.0, 0.0, 180.0).unwrap();
        let f = cylinder_face(&k, &half, 30.0);
        let m = message(cylinder_wall(&k, &half, f, TOL));
        assert!(m.contains("all the way round"), "{m}");
    }

    #[test]
    fn a_pierced_wall_is_refused() {
        let mut k = Kernel::default();
        let b = tube(&mut k);
        // A window through the wall at +x, halfway up.
        let window = create_box(&mut k, p(25.0, -3.0, 15.0), 10.0, 6.0, 10.0).unwrap();
        let pierced = boolean_difference(&mut k, b, window).unwrap();
        let f = cylinder_face(&k, &pierced, 30.0);
        let m = message(cylinder_wall(&k, &pierced, f, TOL));
        assert!(
            m.contains("openings or is split") || m.contains("all the way round"),
            "{m}"
        );
    }

    #[test]
    fn a_bad_tolerance_is_refused_not_answered() {
        let mut k = Kernel::default();
        let b = tube(&mut k);
        let f = cylinder_face(&k, &b, 30.0);
        for tol in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(cylinder_wall(&k, &b, f, tol).is_err(), "{tol}");
        }
    }
}
