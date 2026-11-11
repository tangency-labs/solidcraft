//! The controller settings a post must agree with for a written program to be read right.
//!
//! A dwell's `P` is read in the unit the post writes it in ([`DwellUnit`]), which must be the
//! unit the controller is set to read (some read `G4 P` in seconds, others in milliseconds).
//! [`crate::readback`] and [`crate::readback5`] read a program's dwells back in that unit, so a
//! post whose dwells the controller would read a thousand times too long or too short is caught.

use serde::{Deserialize, Serialize};

use crate::gcode5::JointWords;
use crate::path::Move;
use crate::post::Post;
use crate::{Result, invalid};

/// How a post's dwell template states a dwell's length.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DwellUnit {
    Seconds,
    Milliseconds,
}

impl DwellUnit {
    /// The unit `post`'s dwell template writes: `{milliseconds}` or `{seconds}`, never both, and
    /// never neither (a dwell would then be written at a fixed length, whatever was planned).
    pub fn of(post: &Post) -> Result<DwellUnit> {
        match (post.dwell.contains("{seconds}"), post.dwell.contains("{milliseconds}")) {
            (true, false) => Ok(DwellUnit::Seconds),
            (false, true) => Ok(DwellUnit::Milliseconds),
            (true, true) => invalid("the post's dwell template must give the length once: `{seconds}` or `{milliseconds}`, not both"),
            (false, false) => invalid("the post's dwell template must give the length: `{seconds}` or `{milliseconds}`"),
        }
    }

    /// A dwell's `P` (or `X`) as written, in seconds.
    pub fn seconds(self, written: f64) -> f64 {
        match self {
            DwellUnit::Seconds => written,
            DwellUnit::Milliseconds => written / 1000.0,
        }
    }
}

/// The dwells of read motion, as [`crate::parse`] and [`crate::gcode5::parse5`] read them (`P`
/// in seconds), in seconds for a post that writes them in `unit`.
pub fn dwells_in(unit: DwellUnit, moves: Vec<Move>) -> Vec<Move> {
    moves
        .into_iter()
        .map(|m| match m {
            Move::Dwell { seconds } => Move::Dwell { seconds: unit.seconds(seconds) },
            other => other,
        })
        .collect()
}

/// [`dwells_in`] for multi-axis motion.
pub fn joint_dwells_in(unit: DwellUnit, words: Vec<JointWords>) -> Vec<JointWords> {
    words
        .into_iter()
        .map(|w| match w {
            JointWords::Dwell { line, seconds } => JointWords::Dwell { line, seconds: unit.seconds(seconds) },
            other => other,
        })
        .collect()
}
