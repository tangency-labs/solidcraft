//! Stock-removal simulation for SolidCraft: a neutral move list and G-code that fills it, swept
//! tool volumes, a tri-dexel stock that the sweeps cut, and the surface mesh of the cut stock.
//! Units are millimetres, in the workpiece frame. Depends on the geometry foundation only.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod det_math;
pub mod dexel;
pub mod gcode;
pub mod mesh;
pub mod sim;
pub mod sweep;
pub mod toolpath;

/// Errors from the simulation core. Never a panic: hostile sizes and counts come back as these.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SimError {
    #[error("bad input: {0}")]
    BadInput(String),
    #[error("stock needs about {need} bytes, over the {budget} byte budget")]
    Budget { need: u64, budget: u64 },
    #[error("more than {max} segments on one ray")]
    SegmentCap { max: usize },
    #[error("more than {max} moves")]
    TooManyMoves { max: usize },
    /// A cut failed at move `index` (0-based), from G-code line or CAM op `line`.
    #[error("move {index} (line {line}): {cause}")]
    AtMove { index: usize, line: u32, cause: String },
}
