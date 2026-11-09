//! The kernel's side of the portable B-rep record ([`solidcore_record::brep`]): what computes
//! from its coordinates. Evaluation ([`eval`]), exact NURBS forms of analytic geometry
//! ([`form`]), pcurves ([`pcurve`]), the record's geometric checks ([`check`]: frames, planes,
//! placements; [`geometry`]: that the geometry agrees with the topology), what its labels mean
//! in their own parameters ([`label`]), rigid motions ([`placement`]) and splitting edges
//! ([`split_edges`]). The record itself is data: see SolidCraft's docs/native-brep.md.

// `eval` and `form` name the record's geometry through their parent (`super::Curve`).
use solidcore_record::brep::{Curve, Curve2, Frame, Surface};

pub mod eval;
pub mod form;
pub mod normal_section;
/// The pcurve evaluator moved to the kernel (`solidcore::eval::pcurve`, through
/// `solidcore_ops::pcurve`). Kept here for `crates/kernel/src/tess` only, which is moving into
/// solidcore-tess: its call sites switch in that move, and this goes with them.
pub mod pcurve {
    pub use solidcore_ops::pcurve::eval;
}
pub mod placement;
mod split;
pub use split::split_edges;

#[cfg(test)]
mod tests;
