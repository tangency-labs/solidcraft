//! The kernel's side of the portable B-rep record ([`solidcore_record::brep`]): what computes
//! from its coordinates. Evaluation ([`eval`]), exact NURBS forms of analytic geometry
//! ([`form`]), pcurves ([`pcurve`]), the record's geometric checks ([`check`]: frames, planes,
//! placements; [`geometry`]: that the geometry agrees with the topology), what its labels mean
//! in their own parameters ([`label`]) and splitting edges ([`split_edges`]). Rigid and affine
//! motions of a record are the kernel adapter's (`solidcraft_kernel`). The record itself is data: see SolidCraft's docs/native-brep.md.

// `eval` and `form` name the record's geometry through their parent (`super::Curve`).
use solidcore_record::brep::{Curve, Frame, Surface};

pub mod eval;
pub mod form;
pub mod normal_section;
pub mod ray;
mod split;
pub use split::split_edges;
