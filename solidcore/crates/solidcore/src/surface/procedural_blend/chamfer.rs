//! A chamfer's sizes as its definition keeps them (#2486): two distances, a distance and an
//! angle, or a chord, with the wedge angle they were converted at. A chamfer is the section
//! generator's order 0, the straight chord between the feet of a point that stands off the
//! two supports by the offsets `r_1`, `r_2` (the ball's offsets); the sizes say what those
//! offsets were asked as.
//!
//! In the plane square to the edge, the supports meet at the wedge angle `alpha` (the angle
//! of the wedge the offsets' point sits in, `0 < alpha < pi`). A point at `r_1` off support
//! 1 and `r_2` off support 2 has its feet at the setbacks
//!
//! `d_1 = (r_2 + r_1 cos alpha) / sin alpha`, `d_2 = (r_1 + r_2 cos alpha) / sin alpha`
//!
//! from the edge, along supports 1 and 2, so `r_1 = (d_2 - d_1 cos alpha) / sin alpha` and
//! `r_2 = (d_1 - d_2 cos alpha) / sin alpha`. An offset that is not positive is a chamfer
//! whose distances fold: the chord would leave the wedge. A distance `d` along support 1 and
//! the chamfer's angle `theta` to it set `d_2 = d sin theta / sin(alpha + theta)` (the
//! triangle of the edge and the two feet), `alpha + theta < pi`; a chord `c` has equal
//! setbacks `d = c / (2 sin(alpha / 2))`.
//!
//! Between planes `alpha` is the planes' angle, and the sizes hold all along the edge.
//! Elsewhere they hold where the wedge was measured: the offsets, not the setbacks, are
//! what the definition keeps along the edge. Edges of one size on a tangent chain are one
//! chamfer, so one pair of offsets, and their rails meet where the edges run on into each
//! other: its wedge is that of its edges between planes, on which the sizes then hold all
//! along. Without one it is the narrowest at its edges' middles, whose offsets are the
//! smallest, so a setback is nowhere much past its size; unequal sizes can fold at it, so it
//! moves up to the least wedge at which the smallest setback keeps a thousand edge tolerances
//! at the chain's widest wedge, bounded from above by enclosures of the faces' normals along
//! each edge (`blend::chamfer_side::widest_wedge_bound`), not by samples. The sizes drift. A rim whose wedge runs from 63.4 to
//! 116.6 degrees, asked 1 and 2, is measured at 86.4 and has setbacks from 0.01 to 1.95 and
//! from 1.73 to 2.61. Setbacks that hold at every station are another definition. Edges
//! between planes of different wedges have no one pair: each keeps its own.
//!
//! Not upstream (solidcore's, #2486). Deviations: none. No recursion.

use crate::error::{Result, SolidError};
use crate::math::PI;

const OP: &str = "surface::procedural_blend::chamfer";

/// How a chamfer's size was asked for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChamferSize {
    /// The setbacks from the edge along supports 1 and 2 (equal distances: the same twice).
    Distances([f64; 2]),
    /// The setback from the edge along support 1, and the angle (radians) between the
    /// chamfer and support 1.
    DistanceAngle {
        /// The setback along support 1.
        distance: f64,
        /// The chamfer's angle to support 1, `0 < angle < pi - wedge`.
        angle: f64,
    },
    /// The chord between the rails (equal setbacks; `SmApiChamferFillet`'s distance).
    Chord(f64),
}

/// A chamfer's sizes and the wedge angle they were converted to offsets at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChamferDefinition {
    /// The sizes as asked.
    pub size: ChamferSize,
    /// The angle between the supports at the edge, inside the wedge the chamfer cuts
    /// across, `0 < wedge < pi`.
    pub wedge: f64,
}

fn positive(x: f64) -> bool {
    x.is_finite() && x > 0.0
}

impl ChamferDefinition {
    /// The sizes at the wedge `wedge`, checked: every size finite and positive, the wedge
    /// and the angle inside their ranges.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a size that is not finite and positive, a wedge outside
    /// `(0, pi)`, or an angle outside `(0, pi - wedge)`.
    pub fn new(size: ChamferSize, wedge: f64) -> Result<Self> {
        if !(wedge.is_finite() && wedge > 0.0 && wedge < PI) {
            return Err(SolidError::invalid_input(
                OP,
                "a chamfer's wedge angle is between 0 and pi",
            ));
        }
        let sizes_ok = match size {
            ChamferSize::Distances([a, b]) => positive(a) && positive(b),
            ChamferSize::DistanceAngle { distance, angle } => {
                positive(distance) && positive(angle) && angle + wedge < PI
            }
            ChamferSize::Chord(c) => positive(c),
        };
        if !sizes_ok {
            return Err(SolidError::invalid_input(
                OP,
                "a chamfer's sizes are finite and positive, its angle less than pi less the wedge's",
            ));
        }
        Ok(Self { size, wedge })
    }

    /// The sizes of the chamfer of `offsets` at `wedge`: its two setbacks.
    ///
    /// # Errors
    ///
    /// Those of [`Self::new`] (an offset that is not positive gives a setback that may
    /// not be).
    pub fn of_offsets(offsets: [f64; 2], wedge: f64) -> Result<Self> {
        let [r1, r2] = offsets;
        let (s, c) = wedge.sin_cos();
        Self::new(
            ChamferSize::Distances([(r2 + r1 * c) / s, (r1 + r2 * c) / s]),
            wedge,
        )
    }

    /// The setbacks from the edge along supports 1 and 2.
    #[must_use]
    pub fn setbacks(&self) -> [f64; 2] {
        match self.size {
            ChamferSize::Distances(d) => d,
            ChamferSize::DistanceAngle { distance, angle } => [
                distance,
                distance * angle.sin() / (self.wedge + angle).sin(),
            ],
            ChamferSize::Chord(c) => {
                let d = c / (2.0 * (0.5 * self.wedge).sin());
                [d, d]
            }
        }
    }

    /// The offsets `r_1`, `r_2` of the point whose feet are the chamfer's rails.
    ///
    /// # Errors
    ///
    /// `InvalidInput` where an offset is not positive: the distances fold (the chord
    /// would leave the wedge; a second distance at most the first's `cos(wedge)`).
    pub fn offsets(&self) -> Result<[f64; 2]> {
        let [d1, d2] = self.setbacks();
        let (s, c) = self.wedge.sin_cos();
        let r = [(d2 - d1 * c) / s, (d1 - d2 * c) / s];
        if !(positive(r[0]) && positive(r[1])) {
            return Err(SolidError::invalid_input(
                OP,
                "the chamfer's distances fold: each must be more than the other's cosine of the wedge",
            ));
        }
        Ok(r)
    }

    /// Whether `offsets` are this chamfer's, to the rounding of computing them.
    #[must_use]
    pub fn holds(&self, offsets: [f64; 2]) -> bool {
        let Ok(r) = self.offsets() else {
            return false;
        };
        let [d1, d2] = self.setbacks();
        // the conversion's rounding: a few operations on the setbacks, over sin(wedge)
        let room = 64.0 * f64::EPSILON * (d1.abs() + d2.abs()) / self.wedge.sin();
        r.iter().zip(offsets).all(|(&a, b)| (a - b).abs() <= room)
    }

    /// The chamfer scaled by `factor` (a uniform scale): its lengths scale, its angles
    /// stay.
    #[must_use]
    pub fn scaled(&self, factor: f64) -> Self {
        let size = match self.size {
            ChamferSize::Distances([a, b]) => ChamferSize::Distances([a * factor, b * factor]),
            ChamferSize::DistanceAngle { distance, angle } => ChamferSize::DistanceAngle {
                distance: distance * factor,
                angle,
            },
            ChamferSize::Chord(c) => ChamferSize::Chord(c * factor),
        };
        Self {
            size,
            wedge: self.wedge,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-14 * (1.0 + b.abs())
    }

    #[test]
    fn a_right_angle_s_offsets_are_the_other_setbacks() {
        let c = ChamferDefinition::new(ChamferSize::Distances([1.0, 2.0]), 0.5 * PI).unwrap();
        let [r1, r2] = c.offsets().unwrap();
        assert!(close(r1, 2.0) && close(r2, 1.0), "{r1} {r2}");
        let back = ChamferDefinition::of_offsets([r1, r2], 0.5 * PI).unwrap();
        let [d1, d2] = back.setbacks();
        assert!(close(d1, 1.0) && close(d2, 2.0), "{d1} {d2}");
        assert!(c.holds([r1, r2]));
    }

    #[test]
    fn a_chord_s_setbacks_and_offsets() {
        // a chord of 1 on a square edge: setbacks 1/sqrt 2, offsets 1/sqrt 2
        let c = ChamferDefinition::new(ChamferSize::Chord(1.0), 0.5 * PI).unwrap();
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let [d1, d2] = c.setbacks();
        let [r1, r2] = c.offsets().unwrap();
        assert!(close(d1, h) && close(d2, h) && close(r1, h) && close(r2, h));
    }

    #[test]
    fn a_distance_and_angle_on_a_square_edge() {
        // 45 degrees on a square edge: the second setback is the first
        let c = ChamferDefinition::new(
            ChamferSize::DistanceAngle {
                distance: 2.0,
                angle: 0.25 * PI,
            },
            0.5 * PI,
        )
        .unwrap();
        let [d1, d2] = c.setbacks();
        assert!(close(d1, 2.0) && close(d2, 2.0), "{d1} {d2}");
        // 30 degrees: tan 30 times the first
        let c = ChamferDefinition::new(
            ChamferSize::DistanceAngle {
                distance: 3.0,
                angle: PI / 6.0,
            },
            0.5 * PI,
        )
        .unwrap();
        assert!(close(c.setbacks()[1], 3.0 * (PI / 6.0).tan()));
    }

    #[test]
    fn distances_that_fold_are_refused() {
        // a 60 degree wedge: the second distance must exceed half the first
        let c = ChamferDefinition::new(ChamferSize::Distances([2.0, 0.9]), PI / 3.0).unwrap();
        assert!(c.offsets().is_err());
        let c = ChamferDefinition::new(ChamferSize::Distances([2.0, 1.1]), PI / 3.0).unwrap();
        assert!(c.offsets().is_ok());
    }

    #[test]
    fn hostile_sizes_are_refused() {
        for (size, wedge) in [
            (ChamferSize::Distances([f64::NAN, 1.0]), 1.0),
            (ChamferSize::Distances([1.0, -1.0]), 1.0),
            (ChamferSize::Chord(0.0), 1.0),
            (ChamferSize::Chord(1.0), PI),
            (ChamferSize::Chord(1.0), f64::INFINITY),
            (
                ChamferSize::DistanceAngle {
                    distance: 1.0,
                    angle: 2.0,
                },
                2.0,
            ),
        ] {
            assert!(ChamferDefinition::new(size, wedge).is_err(), "{size:?}");
        }
    }
}
