//! A shaft and its hub as a pair, and the check that they fit.
//!
//! A shaft and its hub end in a [`Pair`]: the two outlines, where their axes are, on which side
//! each has its material, and how the second part moves when the first turns. [`Pair::through`]
//! turns the pair and measures the gap at every step, as a fitter turns a hub on its shaft to feel
//! how it sits.

use std::f64::consts::TAU;

use crate::error::teeth::{TeethFailure, Why};
use crate::teeth::mesh::{gap_typed, looked_from, Body, Gap, Material};
use crate::teeth::outline::{Outline, Point};
use crate::teeth::{in_range, MAX_LENGTH};
use crate::SolidError;

/// One part of a pair.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    /// Its outline about its own axis.
    pub outline: Outline,
    /// On which side of the outline its material is.
    pub material: Material,
    /// Where its axis is.
    pub axis: Point,
}

/// How the second part of a pair moves when the first turns about its axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Motion {
    /// It turns about its own axis, `ratio` times as far as the first: 1 for parts that turn as one
    /// (a shaft in its hub), another number for a part that turns at another rate (negative for the
    /// opposite sense).
    Turning {
        /// The second part's turn for one radian of the first.
        ratio: f64,
    },
}

/// A shaft and its hub, or any two parts that turn together.
#[derive(Clone, Debug, PartialEq)]
pub struct Pair {
    /// The part that is turned.
    pub first: Part,
    /// The part that follows.
    pub second: Part,
    /// How the second follows.
    pub motion: Motion,
    /// Where the second part stands when the first has not turned: its angle in radians.
    pub second_start: f64,
    /// The turn of the first part after which the pair looks the same again, in radians (one
    /// pitch).
    pub cycle: f64,
}

/// The gaps found while a pair was turned.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gaps {
    /// The smallest gap of all steps and where it was; none if the parts were never within reach of
    /// each other. Negative where one part reaches into the material of the other.
    pub least: Option<Gap>,
    /// The largest of the steps' smallest gaps; none if at some step the parts were not within
    /// reach. Flanks that stay in contact give nothing here as well as in `least`.
    pub most: Option<f64>,
    /// The turn of the first part at which the smallest gap was found.
    pub least_at: f64,
}

/// The most steps a pair may be turned through in one call.
pub const MAX_STEPS: u32 = 100_000;

/// The most work one call of [`Pair::through`] may do: the steps times the points each measurement
/// looks from (every point of both outlines, and the points along their chords that are longer than
/// the reach; see [`crate::teeth::mesh::gap`]). It counts the points looked from, not the time:
/// each of them is compared with the chords near it, and each step keeps its own bound
/// ([`crate::teeth::mesh::MOST_WORK`]). A call that would need more is refused before its first
/// step rather than run slowly: fewer steps, a coarser tolerance or a larger reach make it smaller.
pub const MOST_TURNING_WORK: u64 = 50_000_000;

const TOO_MUCH_TURNING: TeethFailure = TeethFailure::Invalid(Why::TooMuchTurning);

impl Pair {
    /// The angle and the place of the second part's axis when the first part is turned by `turn`.
    pub fn second_at(&self, turn: f64) -> (f64, Point) {
        match self.motion {
            Motion::Turning { ratio } => (self.second_start + ratio * turn, self.second.axis),
        }
    }

    /// The two parts as bodies about their own axes, not yet placed.
    pub fn bodies(&self) -> crate::Result<(Body, Body)> {
        self.bodies_typed()
            .map_err(|e| SolidError::teeth("teeth::pair::Pair::bodies", e))
    }

    /// [`Self::bodies`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
    pub(crate) fn bodies_typed(&self) -> Result<(Body, Body), TeethFailure> {
        Ok((
            Body::of_outline(&self.first.outline, self.first.material)?,
            Body::of_outline(&self.second.outline, self.second.material)?,
        ))
    }

    /// The smallest gap between the two parts when the first is turned by `turn`, looking no
    /// farther than `reach`.
    pub fn gap_at(&self, turn: f64, reach: f64) -> crate::Result<Option<Gap>> {
        self.gap_at_typed(turn, reach)
            .map_err(|e| SolidError::teeth("teeth::pair::Pair::gap_at", e))
    }

    /// [`Self::gap_at`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
    pub(crate) fn gap_at_typed(&self, turn: f64, reach: f64) -> Result<Option<Gap>, TeethFailure> {
        let (first, second) = self.bodies_typed()?;
        self.gap_of(&first, &second, turn, reach)
    }

    fn gap_of(
        &self,
        first: &Body,
        second: &Body,
        turn: f64,
        reach: f64,
    ) -> Result<Option<Gap>, TeethFailure> {
        let turn = in_range("turn", turn, -1e6, 1e6)?;
        if !self
            .first
            .axis
            .iter()
            .all(|v| v.is_finite() && v.abs() <= MAX_LENGTH)
        {
            return Err(TeethFailure::Invalid(Why::FirstAxisNotFinite));
        }
        let (angle, axis) = self.second_at(turn);
        if !(angle.is_finite() && axis.iter().all(|v| v.is_finite() && v.abs() <= MAX_LENGTH)) {
            return Err(TeethFailure::Invalid(Why::SecondNotFinite));
        }
        gap_typed(
            &first.placed(turn, self.first.axis),
            &second.placed(angle, axis),
            reach,
        )
    }

    /// The gaps while the first part is turned from `from` to `to` in `steps` equal steps (the end
    /// itself is not visited), looking no farther than `reach` at each.
    ///
    /// Refused before the first step when the steps times the points each measurement looks from
    /// would be more than [`MOST_TURNING_WORK`].
    pub fn through(&self, from: f64, to: f64, steps: u32, reach: f64) -> crate::Result<Gaps> {
        self.through_typed(from, to, steps, reach)
            .map_err(|e| SolidError::teeth("teeth::pair::Pair::through", e))
    }

    /// [`Self::through`] with its failure as a [`TeethFailure`], for the tooth module's own
    /// callers.
    pub(crate) fn through_typed(
        &self,
        from: f64,
        to: f64,
        steps: u32,
        reach: f64,
    ) -> Result<Gaps, TeethFailure> {
        if !(1..=MAX_STEPS).contains(&steps) {
            return Err(TeethFailure::OutOfRange {
                what: "steps",
                value: f64::from(steps),
                above: 0.0,
                up_to: f64::from(MAX_STEPS),
            });
        }
        let reach = in_range("reach", reach, 0.0, MAX_LENGTH)?;
        let (first, second) = self.bodies_typed()?;
        // The count of points looked from does not change as the parts turn: it hangs on the
        // lengths of the chords alone.
        let work = f64::from(steps) * looked_from(&first, &second, reach);
        if !(work <= MOST_TURNING_WORK as f64) {
            return Err(TOO_MUCH_TURNING);
        }
        let mut gaps = Gaps {
            least: None,
            most: Some(f64::NEG_INFINITY),
            least_at: from,
        };
        for i in 0..steps {
            let turn = from + (to - from) * f64::from(i) / f64::from(steps);
            match self.gap_of(&first, &second, turn, reach)? {
                Some(found) => {
                    if gaps
                        .least
                        .is_none_or(|least| found.distance < least.distance)
                    {
                        (gaps.least, gaps.least_at) = (Some(found), turn);
                    }
                    gaps.most = gaps.most.map(|most| most.max(found.distance));
                }
                None => gaps.most = None,
            }
        }
        Ok(gaps)
    }

    /// The gaps through one cycle of the pair, in `steps` equal steps.
    pub fn through_a_cycle(&self, steps: u32, reach: f64) -> crate::Result<Gaps> {
        self.through_a_cycle_typed(steps, reach)
            .map_err(|e| SolidError::teeth("teeth::pair::Pair::through_a_cycle", e))
    }

    /// [`Self::through_a_cycle`] with its failure as a [`TeethFailure`], for the tooth module's own
    /// callers.
    pub(crate) fn through_a_cycle_typed(
        &self,
        steps: u32,
        reach: f64,
    ) -> Result<Gaps, TeethFailure> {
        let cycle = if self.cycle.is_finite() && self.cycle != 0.0 {
            self.cycle
        } else {
            TAU
        };
        self.through_typed(0.0, cycle, steps, reach)
    }
}
