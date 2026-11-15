//! How far a path turns where two curves meet: the angle between the direction the first
//! leaves in and the direction the second continues in. The kernel's own
//! [`solidcore::geom::vector2d::Vec2::angle_between`] measures it; ops only adapts the
//! application's plain numbers.

use solidcore::geom::vector2d::Vec2;

use crate::{Error, Result};

/// The angle in radians, in `[0, PI]`, between the planar directions `before` (the first curve's
/// end tangent) and `after` (the next curve's start tangent): 0 where the path runs straight
/// on, `PI` where it doubles back. The directions need not be unit.
///
/// # Errors
/// [`Error::Argument`] for a direction that is not finite or is zero-length.
pub fn joint_turn(before: [f64; 2], after: [f64; 2]) -> Result<f64> {
    if !before.iter().chain(after.iter()).all(|c| c.is_finite()) {
        return Err(Error::Argument);
    }
    Vec2::from(before)
        .angle_between(Vec2::from(after))
        .map_err(|_| Error::Argument)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-12
    }

    #[test]
    fn turn_is_the_angle_between_the_directions() {
        assert!(close(joint_turn([1.0, 0.0], [2.0, 0.0]).unwrap(), 0.0));
        assert!(close(
            joint_turn([1.0, 0.0], [0.0, 3.0]).unwrap(),
            std::f64::consts::FRAC_PI_2
        ));
        assert!(close(
            joint_turn([0.0, 1.0], [1.0, 0.0]).unwrap(),
            std::f64::consts::FRAC_PI_2
        ));
        assert!(close(
            joint_turn([1.0, 0.0], [-1.0, 0.0]).unwrap(),
            std::f64::consts::PI
        ));
    }

    #[test]
    fn a_zero_or_non_finite_direction_is_refused() {
        assert_eq!(joint_turn([0.0, 0.0], [1.0, 0.0]), Err(Error::Argument));
        assert_eq!(
            joint_turn([f64::NAN, 0.0], [1.0, 0.0]),
            Err(Error::Argument)
        );
    }
}
