//! (not upstream) How far a face's outward normals turn from a direction: a proved interval
//! of the angle between the direction and the face's outward normal at every point of it,
//! from the surface's definition, rounding included, never samples.
//!
//! Upstream: none.
//!
//! | Rust                      | Kinds                                         |
//! |---------------------------|-----------------------------------------------|
//! | [`normal_angle_interval`] | plane, cylinder, cone, sphere, ring torus     |
//!
//! The ball that holds the face's proved box gives the cone holding the surface's normal
//! lines there ([`Surface::normal_cone_in_ball`](crate::surface::surface::Surface), closed
//! form), and the cone the interval (`NormalCone::angle_interval`): its sense is the
//! outward normal at a point of the face's surface inside the ball (the surface within the
//! ball is one sheet whose normals stay in one nappe, so any such point decides it). The
//! point is the first of the UV domain's centre and the middles of the UV trim curves of
//! the face's edge uses that the surface puts inside the ball; where the uv came from does
//! not matter, only that its surface point is checked to lie in the ball. A plane's normal
//! is the same everywhere, so a plane needs no ball and any point of it decides the sense.
//!
//! Nothing is proved (`None`) for another surface kind (B-spline, extrusion, revolution,
//! offset, blend: a later change, with the same signature), a cone a quarter turn or wider
//! (a ball that reaches the axis, centre or apex height: a whole tube, a large cap), no
//! candidate point inside the ball with a normal, a faceuse of another face or of unknown
//! orientation, a direction of no length, or a construction the kernel refuses on the way
//! (a refusal is not an error here).
//!
//! What it gives: where the interval's upper end is below a quarter turn, the projection of
//! the face along the direction is a local diffeomorphism (near each point, the lines along
//! the direction meet the face once). It does not make the projection one-to-one: a
//! helical ramp, `(r cos t, r sin t, c t)` over two turns, keeps its normals within
//! `atan(c)` of its axis and covers the plane twice. A caller that needs one crossing per
//! line checks that separately. Uses: a face turned toward a direction, draft and undercut
//! against a pull direction, a notice naming how far a face tilts; thresholds and words are
//! the caller's.
//!
//! Deviations: not a port.

use crate::error::native_failure;
use crate::geom::certified::above;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::math::gamma;
use crate::query::api_queries::face_normal_at_uv;
use crate::query::brep as qbrep;
use crate::surface::surface::p2::SurfaceKind;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::face::p2 as fp2;
use crate::topology::face::p4 as fp4;
use crate::topology::FaceuseId;
use crate::{Brep, FaceId, Kernel, Point2, Result, SolidError, Vec3};

/// A proved interval `[lo, hi]` (radians, within `0..=pi`) of the angle between `direction`
/// and the outward normal of face `f` (the sense of its use `fu`) at every point of it
/// (module docs); `None` where nothing is proved.
///
/// # Errors
///
/// Cancellation and the errors [`native_failure`] passes on (an unported call, the port's
/// invariants, a dangling id); a refused construction is `None`.
pub fn normal_angle_interval(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    fu: FaceuseId,
    direction: Vec3,
) -> Result<Option<[f64; 2]>> {
    Ok(native_failure(interval(k, b, f, fu, direction))?.flatten())
}

/// [`normal_angle_interval`], a construction's refusal as an error.
fn interval(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    fu: FaceuseId,
    direction: Vec3,
) -> Result<Option<[f64; 2]>> {
    let len = direction.length();
    if !(len.is_finite() && above(len, 0.0)) {
        return Ok(None);
    }
    let d = direction / len;
    let use_ = b.faceuse(fu)?;
    if use_.face != Some(f) || use_.orientation == OrientType::Unknown {
        return Ok(None);
    }
    let Some(surface) = fp2::surface(b, f)? else {
        return Ok(None);
    };
    // a plane's cone does not depend on the ball, and its sense not on the point
    let plane = matches!(surface.kind(), SurfaceKind::Plane(_));
    let (centre, radius) = if plane {
        (Vec3::new(0.0, 0.0, 0.0), 0.0)
    } else {
        let Some(bx) = crate::query::face::p1::proved_bounding_box(k, b, f, &mut None)? else {
            return Ok(None);
        };
        let centre = bx.mid();
        (centre, bx.max_distance_to_point(centre) * (1.0 + gamma(4)))
    };
    let Some(cone) = surface.normal_cone_in_ball(centre, radius) else {
        return Ok(None);
    };
    // the sense: the outward normal at a point of the surface inside the ball (anywhere on
    // a plane)
    for uv in candidate_uvs(k, b, f)? {
        let p = match qbrep::evaluate_surface_point(k, Some(surface), uv) {
            Ok(p) => p,
            Err(SolidError::InvalidInput { .. }) => continue,
            Err(e) => return Err(e),
        };
        let off = (p - centre).length();
        if !(off.is_finite() && (plane || above(radius, off * (1.0 + gamma(4))))) {
            continue;
        }
        if let Some(n) = outward_at(k, b, f, uv, fu)? {
            return Ok(cone.angle_interval(n.value, d));
        }
    }
    Ok(None)
}

/// UV points to try for the cone's sense, in order: the centre of face `f`'s UV domain, then
/// the middle of each UV trim curve its edge uses hold (or read from their mates). Each is
/// only a guess: the caller checks its surface point lies in the ball.
fn candidate_uvs(k: &Kernel, b: &Brep, f: FaceId) -> Result<Vec<Point2>> {
    let mut uvs = vec![fp2::uv_domain(b, f)?.evaluate(0.5, 0.5)];
    for lu in fp4::upward_loopuses(b, f, false, None)? {
        for eu in eu1::loopuse_edgeuses(b, lu)? {
            if let (Some(c), _) = eu1::uv_trim_curve(b, eu)? {
                match c.evaluate_point(k, c.natural_interval().mid()) {
                    Ok(p) => uvs.push(Point2::new(p.x, p.y)),
                    Err(SolidError::InvalidInput { .. }) => {}
                    Err(e) => return Err(e),
                }
            }
        }
    }
    Ok(uvs)
}

/// The outward normal of `f` at `uv` ([`face_normal_at_uv`]); `None` where it is not
/// defined there (a zero normal, a pole).
fn outward_at(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    uv: Point2,
    fu: FaceuseId,
) -> Result<Option<crate::query::normal::UnitVector>> {
    match face_normal_at_uv(k, b, f, uv, fu) {
        Ok(n) => Ok(Some(n)),
        Err(SolidError::InvalidInput { .. }) => Ok(None),
        Err(e) => Err(e),
    }
}
