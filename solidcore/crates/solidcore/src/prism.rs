//! Planar prisms: profiles of lines, arcs, conics and Béziers in a frame's coordinates, the
//! exact reading of a curve in a plane, a planar sheet read back as regions, a region extruded
//! between two levels, and the certified section of a body that is a uniform prism.
//!
//! Not upstream: SolidCraft's planar regions and prism measures (`prism_curve.rs`,
//! `prism_measure.rs` and the geometry of `prism.rs`), moved into the kernel so the layers above
//! compute no geometry (docs/layers.md §1). Their recipes (the planar boolean and extrusion) are
//! `solidcore-ops`'.
//!
//! Every query here returns its value with a proved bound, exactly, or not at all (`None` or a
//! typed refusal): it decides nothing at a tolerance it cannot prove (owner rule 13564).

pub mod extrude;
pub mod measure;
pub mod path_export;
pub mod profile;
pub mod read;
pub mod recognition;
pub mod sheet;

#[cfg(test)]
mod tests;
