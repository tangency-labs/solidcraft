//! Where a plane touches a cylindrical wall, outside or a bore, and the frame a profile wraps
//! onto it in ([`solidcore::surface::cylinder_wall`]).

pub use solidcore::surface::cylinder_wall::WallFrame;

/// The frame of the plane with unit `normal`, through the point `w` from a point of the axis
/// (unit direction `axis`), on the wall of radius `radius` ([`solidcore::surface::cylinder_wall::wall_frame`]);
/// `None` unless the plane is parallel to the axis and touches the wall. An exterior wall needs
/// the normal pointing away from the axis; a bore takes either side.
#[must_use]
pub fn wall_frame(
    normal: [f64; 3],
    w: [f64; 3],
    axis: [f64; 3],
    radius: f64,
    convex: bool,
    tol: f64,
) -> Option<WallFrame> {
    solidcore::surface::cylinder_wall::wall_frame(normal, w, axis, radius, convex, tol)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_outside_wall_and_a_bore_wrap_the_same_way_about_their_own_normals() {
        let z = [0.0, 0.0, 1.0];
        let outside = wall_frame([0.0, 1.0, 0.0], [0.0, 10.0, 0.0], z, 10.0, true, 1e-8).unwrap();
        assert_eq!(outside.radial, [0.0, 1.0, 0.0]);
        assert_eq!(outside.tangent, [-1.0, 0.0, 0.0]);
        // A bore takes the normal either way; the contact line is the same.
        for n in [1.0, -1.0] {
            let bore = wall_frame([0.0, n, 0.0], [0.0, 8.0, 0.0], z, 8.0, false, 1e-8).unwrap();
            assert_eq!(bore.radial, [0.0, 1.0, 0.0]);
            assert_eq!(bore.tangent, [1.0, 0.0, 0.0]);
        }
    }

    #[test]
    fn a_plane_that_does_not_touch_or_leans_has_no_frame() {
        let z = [0.0, 0.0, 1.0];
        // The wrong side of an outside wall, a plane clear of the wall, a plane across the axis.
        assert!(wall_frame([0.0, 1.0, 0.0], [0.0, -10.0, 0.0], z, 10.0, true, 1e-8).is_none());
        assert!(wall_frame([0.0, 1.0, 0.0], [0.0, 9.0, 0.0], z, 10.0, true, 1e-8).is_none());
        assert!(wall_frame([0.0, 0.0, 1.0], [0.0, 0.0, 10.0], z, 10.0, false, 1e-8).is_none());
    }
}
