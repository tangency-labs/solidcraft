//! Smart Extrude (#1257): which way a new extrude goes and what it does to the bodies, decided
//! from where the profile was picked and where the arrow is dragged, and kept steady while the
//! drag goes on.
//!
//! - Direction: a sketch profile (or a thin extrude's curve) extrudes toward the side it was
//!   picked from, so a profile picked from the bottom view goes down. A body's planar face
//!   always goes outward. Once the user sets the direction (Reverse, Direction), picks no
//!   longer change it.
//! - Operation: the first choice for each side of the profile is latched. A drag that starts
//!   into a body is a Cut and stays one however far it goes, through the far side included;
//!   one that starts as a Join does not turn into a Cut when it reaches another body. When
//!   the distance passes through zero onto the other side (a drag reversed past the start, or
//!   Reverse), the operation is decided afresh for that side and latched there. So is a new
//!   start (From Object, a start offset): it moves the extrude to other bodies (#467). A zero
//!   distance changes nothing. Choosing an operation by hand ends the automatic choice for
//!   the rest of the extrude.

use solidcraft_engine::Sel;
use solidcraft_engine::geom::Vec3;

/// The extrude window's automatic state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Smart {
    /// The picks the latch and the direction were decided for.
    pub picked: Vec<Sel>,
    /// The direction was set by hand (or comes from a saved extrude): picks keep it.
    pub manual_dir: bool,
    /// The automatic operation (an index into the operations) and the side it was chosen for.
    pub latch: Option<(usize, i8)>,
    /// The To Object target the distance was last set from.
    pub target: Option<Sel>,
    /// Where the extrude starts (its From Object and start offset) when the latch was decided.
    pub start: serde_json::Value,
    /// Picks off the profile plane become the To Object target (Preferences › Design).
    pub auto_to_object: bool,
}

/// One-side direction for a new pick: 0 (along the normal) or 1 (reversed). `ray` is the view
/// ray toward the pick; a ray running along the normal looks at the back of the profile, so
/// the extrude comes back toward the camera. Body faces always go outward.
pub fn picked_direction(ray: Vec3, normal: Vec3, body_face: bool) -> usize {
    usize::from(!body_face && ray.dot(normal) > 1e-9)
}

/// Which side of the profile an extrude goes to: +1 along the normal, -1 against it, 0 for
/// none (a zero distance). Symmetric and two-sided extrudes go both ways, so always +1.
pub fn side(distance: f64, direction: usize) -> i8 {
    if direction >= 2 {
        return 1;
    }
    let d = if direction == 1 { -distance } else { distance };
    if !d.is_finite() || d.abs() < 1e-9 {
        0
    } else if d > 0.0 {
        1
    } else {
        -1
    }
}

/// The operation to show: the latched one while the extrude stays on the side it was chosen
/// for, otherwise `fresh` (what the geometry says now), which is then latched for this side.
/// `current` stands while the distance is zero.
pub fn operation(fresh: usize, current: usize, side: i8, latch: &mut Option<(usize, i8)>) -> usize {
    if side == 0 {
        return current;
    }
    match *latch {
        Some((op, s)) if s == side => op,
        _ => {
            *latch = Some((fresh, side));
            fresh
        }
    }
}

/// The view ray at `at`: from the eye for a perspective view, along the view for a parallel one.
pub fn view_ray(cam: &solidcraft_engine::render::Camera, at: Vec3) -> Vec3 {
    if cam.fov > 0.0 { (at - cam.eye()).normalized().unwrap_or(-cam.back()) } else { -cam.back() }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEW: usize = 0;
    const JOIN: usize = 1;
    const CUT: usize = 2;

    #[test]
    fn direction_follows_the_picked_side() {
        let up = Vec3::Z;
        // From the top view the ray runs down: the extrude goes up (along the normal).
        assert_eq!(picked_direction(-Vec3::Z, up, false), 0);
        // From the bottom view the ray runs up: the extrude goes down.
        assert_eq!(picked_direction(Vec3::Z, up, false), 1);
        // Edge on: along the normal, as before.
        assert_eq!(picked_direction(Vec3::X, up, false), 0);
        // A body's face goes outward from either side.
        assert_eq!(picked_direction(Vec3::Z, up, true), 0);
        assert_eq!(picked_direction(-Vec3::Z, up, true), 0);
    }

    #[test]
    fn side_of_the_distance() {
        assert_eq!(side(5.0, 0), 1);
        assert_eq!(side(-5.0, 0), -1);
        assert_eq!(side(5.0, 1), -1);
        assert_eq!(side(-5.0, 1), 1);
        assert_eq!(side(0.0, 0), 0);
        assert_eq!(side(f64::NAN, 0), 0);
        assert_eq!(side(-5.0, 2), 1, "symmetric");
        assert_eq!(side(-5.0, 3), 1, "two sides");
    }

    /// A drag into a box that goes on past its far side: the geometry alone would say Join
    /// there, the latch keeps the Cut.
    #[test]
    fn a_cut_stays_a_cut_past_the_far_side() {
        let mut latch = None;
        let mut op = JOIN;
        // Into the body: Cut.
        op = operation(CUT, op, -1, &mut latch);
        assert_eq!(op, CUT);
        // Further, then past the far side, where a fresh look would see no material.
        for fresh in [CUT, CUT, JOIN, JOIN] {
            op = operation(fresh, op, -1, &mut latch);
            assert_eq!(op, CUT);
        }
    }

    #[test]
    fn a_join_does_not_turn_into_a_cut() {
        let mut latch = None;
        let mut op = operation(JOIN, NEW, 1, &mut latch);
        assert_eq!(op, JOIN);
        // The tool reaches another body further out.
        op = operation(CUT, op, 1, &mut latch);
        assert_eq!(op, JOIN);
    }

    /// Reversing through zero decides afresh for the new side; back again, afresh again.
    #[test]
    fn reversing_through_zero_decides_again() {
        let mut latch = None;
        let mut op = operation(CUT, JOIN, -1, &mut latch);
        assert_eq!(op, CUT);
        // At zero nothing changes.
        op = operation(JOIN, op, 0, &mut latch);
        assert_eq!(op, CUT);
        assert_eq!(latch, Some((CUT, -1)));
        // Out the other side, away from the body: Join, latched there.
        op = operation(JOIN, op, 1, &mut latch);
        assert_eq!(op, JOIN);
        op = operation(CUT, op, 1, &mut latch);
        assert_eq!(op, JOIN);
        // Back into the body: Cut again.
        op = operation(CUT, op, -1, &mut latch);
        assert_eq!(op, CUT);
    }

    #[test]
    fn view_ray_of_both_projections() {
        let mut cam = solidcraft_engine::render::Camera { fov: 0.0, ..Default::default() };
        assert!((view_ray(&cam, Vec3::ZERO) + cam.back()).len() < 1e-12);
        cam.fov = 0.8;
        let at = Vec3::new(10.0, 0.0, 0.0);
        let r = view_ray(&cam, at);
        assert!((r - (at - cam.eye()).normalized().unwrap()).len() < 1e-12);
    }
}
