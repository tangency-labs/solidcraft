//! Planar prisms: profiles of lines, arcs, conics and Béziers in a frame's coordinates, the
//! exact reading of a curve in a plane, a planar sheet read back as regions, a region extruded
//! between two levels, a channel between two walls filled to a region ([`fill`]), the
//! certified section of a body that is a uniform prism, and straight members: a section run
//! along a line, its ends where trims and butt joints put them.
//!
//! Not upstream: SolidCraft's planar regions and prism measures (`prism_curve.rs`,
//! `prism_measure.rs` and the geometry of `prism.rs`), moved into the kernel so the layers above
//! compute no geometry (docs/layers.md §1). Their recipes (the planar boolean and extrusion) are
//! `solidcore-ops`'.
//!
//! Every query here returns its value with a proved bound, exactly, or not at all (`None` or a
//! typed refusal): it decides nothing at a tolerance it cannot prove (owner rule 13564). The
//! exception is [`fill`], a construction that keeps the margins its docs give, with the two
//! queries it is built from, [`profile::loop_crosses`] and [`fill::arc_clearance`]: they decide
//! at the near-miss tolerance ([`profile::near_miss`]), not at a proved bound.

pub mod arc_conversion;
pub mod bounds;
pub mod extrude;
pub mod fill;
pub mod fit;
pub mod gusset;
pub mod measure;
pub mod member;
pub mod placement;
pub mod profile;
pub mod read;
pub mod recognition;
pub mod sheet;
pub mod transform;

#[cfg(test)]
mod tests;
