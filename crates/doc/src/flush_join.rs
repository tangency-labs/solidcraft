//! Join of two solids that only touch along a face.
//!
//! The kernel separates coincident faces by a shift scaled to the whole body (a couple of
//! percent of its size) and refuses the solid when that result's volume is off. A manual move
//! of the tool a fraction of a millimetre into the target makes the same join succeed, because
//! the contact is then an overlap the kernel can finish. Moving the whole tool also shifts its
//! far faces. Here only the back-to-back contact faces move, outward into the target, and only
//! the result whose volume is the flush union (the two volumes added) is kept. The overlap sits
//! inside the target, so the outer shape is the flush join.

use solidcraft_geom::Vec3;
use solidcraft_kernel::{self as kernel, Body, BoolOp};

use crate::{DocError, Result};

/// Depths tried, smallest first. Each is far more than the coincidence tolerance and far less
/// than the body-scaled separation (several millimetres on a part a few hundred millimetres long).
const DEPTHS_MM: [f64; 3] = [0.05, 0.2, 0.5];

/// Back-to-back contacts offset in one retry. More than this is left to the error.
const MAX_CONTACTS: usize = 8;

/// Join `tool` onto `target`. A flush contact that the kernel refuses is retried with the
/// tool's back-to-back faces overlapped a fraction of a millimetre into the target.
/// `empty` is the message when the kernel reports an empty union.
pub(crate) fn join_bodies(target: &Body, tool: &Body, empty: &str) -> Result<Body> {
    match kernel::boolean(target, tool, BoolOp::Union) {
        Ok(Some(body)) => Ok(body),
        Ok(None) => Err(DocError::Invalid(empty.into())),
        Err(error) if flush_refusal(&error) => flush_overlap_join(target, tool).ok_or(error.into()),
        Err(error) => Err(error.into()),
    }
}

fn flush_refusal(error: &kernel::KernelError) -> bool {
    let text = error.to_string();
    text.contains("could not be computed reliably") && text.contains("coincident-face separation")
}

fn flush_overlap_join(target: &Body, tool: &Body) -> Option<Body> {
    let contacts = contact_points(target, tool);
    if contacts.is_empty() {
        return None;
    }
    for depth in DEPTHS_MM {
        let Ok(grown) = kernel::offset_faces(tool, &contacts, depth) else { continue };
        let Ok(Some(joined)) = kernel::boolean(target, &grown, BoolOp::Union) else { continue };
        if joined.validity().is_empty() && volumes_agree(target, tool, &joined) {
            return Some(joined);
        }
    }
    None
}

/// Centroids of the tool's planar faces that lie back-to-back on a planar face of the target.
fn contact_points(target: &Body, tool: &Body) -> Vec<Vec3> {
    let size = target.size().max(tool.size());
    if !(size.is_finite() && size > 0.0 && size < 1e7) {
        return Vec::new();
    }
    let plane_tol = (size * 1e-4).max(1e-3);
    let face_tol = (size * 1e-3).max(1e-3);
    let (Ok(tool_faces), Ok(target_faces)) = (tool.faces(face_tol), target.faces(face_tol)) else {
        return Vec::new();
    };
    let mut points = Vec::new();
    for face in &tool_faces {
        if points.len() >= MAX_CONTACTS {
            break;
        }
        let Some(normal) = face.plane_normal else { continue };
        if face.area < 1e-4 || !normal.x.is_finite() || !normal.y.is_finite() || !normal.z.is_finite() {
            continue;
        }
        let at = face.centroid;
        if !at.x.is_finite() || !at.y.is_finite() || !at.z.is_finite() {
            continue;
        }
        let meets = target_faces.iter().any(|other| {
            let Some(outward) = other.plane_normal else { return false };
            normal.dot(outward) < -1.0 + 1e-4 && outward.dot(at - other.centroid).abs() < plane_tol
        });
        if meets {
            points.push(at);
        }
    }
    points
}

/// The flush union adds the two volumes. A result that grew or lost material is not it.
fn volumes_agree(target: &Body, tool: &Body, joined: &Body) -> bool {
    let (Ok(target_m), Ok(tool_m), Ok(joined_m)) = (kernel::measure(target), kernel::measure(tool), kernel::measure(joined)) else {
        return false;
    };
    let (a, b, got) = (target_m.volume, tool_m.volume, joined_m.volume);
    if !(a.is_finite() && b.is_finite() && got.is_finite()) {
        return false;
    }
    let slack = 2e-4 * (a.abs() + b.abs()) + 1e-6;
    (got - (a + b)).abs() <= slack
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use solidcraft_geom::{Loop2, Plane, Region2, Vec2, Vec3};
    use solidcraft_kernel::{self as kernel, BoolOp};

    use super::*;

    /// The rail profile from the flush frame/rail join: a 4.8 mm plate on y = −0.2, with the
    /// angled ends of that rail. Its area times 4.8 mm is 16569.6 mm³.
    fn rail() -> Body {
        let pts = [Vec2::new(-0.2, -0.2), Vec2::new(9.8, 9.8), Vec2::new(345.0, 9.8), Vec2::new(350.0, 4.8), Vec2::new(355.0, -0.2)];
        let region = Region2 { outer: Loop2::polygon(&pts).ccw(), holes: Vec::new() };
        kernel::extrude(&Plane::XY, &[region], -5.0, -0.2).unwrap().remove(0)
    }

    /// A wall whose top is the rail's contact (y = −0.2) and whose bottom is the rail's cap
    /// (z = −5), with a hole through the wall. The hole is what makes the body-scaled
    /// coincident-face separation come out heavy; the same wall without it joins directly.
    fn drilled_wall() -> Body {
        let wall = kernel::box_solid(Vec3::new(-0.2, -10.0, -5.0), Vec3::new(365.0, -0.2, 30.0)).unwrap();
        let hole = kernel::cylinder(Vec3::new(340.0, -11.0, 5.0), Vec3::Y, 3.0, 12.0).unwrap();
        kernel::boolean(&wall, &hole, BoolOp::Cut).unwrap().unwrap()
    }

    #[test]
    fn a_flush_rail_joins_a_drilled_wall_without_moving_the_rail() {
        let wall = drilled_wall();
        let rail = rail();
        let direct = kernel::boolean(&wall, &rail, BoolOp::Union);
        assert!(direct.is_err(), "the flush join is refused before the overlap retry: {direct:?}");
        let joined = join_bodies(&wall, &rail, "join produced nothing").expect("overlap of the contact face joins them");
        let want = kernel::measure(&wall).unwrap().volume + kernel::measure(&rail).unwrap().volume;
        let got = kernel::measure(&joined).unwrap().volume;
        let slack = 2e-4 * want.abs() + 1e-6;
        assert!((got - want).abs() <= slack, "volume {got} against the flush sum {want}");
        assert!(joined.validity().is_empty(), "{:?}", joined.validity());
        assert_eq!(joined.lumps().unwrap().len(), 1);
        // A move of the whole rail would drop its outer face (y = 9.8) by that move.
        let mesh = joined.tessellate(0.2).unwrap();
        let max_y = mesh.positions.iter().map(|p| p.y).fold(f64::MIN, f64::max);
        assert!((max_y - 9.8).abs() < 0.05, "outer face moved to y = {max_y}");
    }

    #[test]
    fn an_ordinary_overlap_is_joined_as_it_is() {
        let a = kernel::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let b = kernel::box_solid(Vec3::new(5.0, 0.0, 0.0), Vec3::new(15.0, 10.0, 10.0)).unwrap();
        let joined = join_bodies(&a, &b, "join produced nothing").unwrap();
        let got = kernel::measure(&joined).unwrap().volume;
        assert!((got - 1500.0).abs() < 1e-6, "{got}");
    }
}
