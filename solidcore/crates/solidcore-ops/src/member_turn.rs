//! The turn of a frame member about its own axis that makes one of its flat sides parallel to a
//! line. The kernel measures; ops compares the measures with tolerances.

use solidcore::query::member_turn::measure_square_turn;
use solidcore::Vec3;

/// Why no turn can be answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParallelTurnError {
    /// The picked face is not a flat side of the member: its normal is not square to the axis.
    NotASide,
    /// The line is a zero or non-finite vector.
    NotALine,
    /// The line runs along the axis: turning the member never makes a side parallel to it.
    AlongAxis,
}

/// A side's normal is square to the axis within this (a dot product of unit vectors).
const SIDE_TOL: f64 = 1e-6;
/// A line this close to the axis (radians) is along it.
const AXIS_TOL: f64 = 1e-9;

/// The turn (radians, in (-PI/2, PI/2]) about `axis` that makes the side with normal
/// `face_normal` parallel to `line`.
///
/// # Errors
///
/// [`ParallelTurnError`], see its variants.
pub fn parallel_turn(
    axis: [f64; 3],
    face_normal: [f64; 3],
    line: [f64; 3],
) -> Result<f64, ParallelTurnError> {
    let m = measure_square_turn(Vec3::from(axis), Vec3::from(face_normal), Vec3::from(line))
        .map_err(|_| ParallelTurnError::NotALine)?;
    if m.normal_along_axis > SIDE_TOL {
        return Err(ParallelTurnError::NotASide);
    }
    if m.line_off_axis < AXIS_TOL {
        return Err(ParallelTurnError::AlongAxis);
    }
    Ok(m.turn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f64::consts::PI;

    #[test]
    fn parallel_turn_cases() {
        let z = [0.0, 0.0, 1.0];
        let x = [1.0, 0.0, 0.0];
        let ok = |l: [f64; 3], want: f64| {
            let got = parallel_turn(z, x, l).unwrap();
            assert!((got - want).abs() < 1e-12, "{l:?}: {got} vs {want}");
        };
        ok([0.0, 1.0, 0.0], 0.0);
        ok([1.0, 1.0, 0.0], -PI / 4.0);
        ok([1.0, 1.0, 5.0], -PI / 4.0);
        ok([1.0, 0.0, 0.0], PI / 2.0);
        assert_eq!(parallel_turn(z, x, z), Err(ParallelTurnError::AlongAxis));
        assert_eq!(
            parallel_turn(z, z, [0.0, 1.0, 0.0]),
            Err(ParallelTurnError::NotASide)
        );
        assert_eq!(
            parallel_turn(z, x, [f64::NAN, 0.0, 0.0]),
            Err(ParallelTurnError::NotALine)
        );
        assert_eq!(
            parallel_turn(z, x, [0.0; 3]),
            Err(ParallelTurnError::NotALine)
        );
    }
}
