//! Planar fills ([`Tessellate`]): closed planar loops, or the union of planar regions, filled
//! with triangles. Points in, points and triangles out. The display plan and its meshes stay
//! solidcore-tess's own types, which the viewport draws (`docs/design/kernel-layer.md` §2).

use crate::Backend;

/// A planar fill: its vertices, in the input's coordinates, and its counter-clockwise triangles.
pub type PlanarFill = (Vec<[f64; 2]>, Vec<[u32; 3]>);

/// A fill the kernel refuses, in its words: a boundary with fewer than three finite points, more
/// points than the limit, a region without an outline, or loops it cannot fill.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct TessRefused(pub String);

/// The planar fills of one kernel ([`crate::OpKind::Tessellate`]).
pub trait Tessellate: Backend {
    /// Closed planar boundary loops filled, their holes kept: the first loop is the outer
    /// boundary, the rest are holes.
    fn triangulate_planar(&self, loops: &[Vec<[f64; 2]>]) -> Result<PlanarFill, TessRefused>;
    /// The union of planar regions filled, their holes kept: each region is its loops (the
    /// largest its outline, the rest its holes); regions that overlap or abut come back as one
    /// surface with no seam where they meet.
    fn triangulate_union(&self, regions: &[Vec<Vec<[f64; 2]>>]) -> Result<PlanarFill, TessRefused>;
}
