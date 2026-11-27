//! The router's planar fills ([`solidcraft_kernel_api::tessellate::Tessellate`]), as
//! `solidcraft_kernel::tessellate`: closed planar loops, or the union of planar regions, filled
//! with triangles.

use solidcraft_kernel_api::OpKind;
use solidcraft_kernel_api::tessellate::Tessellate;

pub use solidcraft_kernel_api::tessellate::{PlanarFill, TessRefused};

use super::{Which, which};
use crate::backend::Solidcore;

/// Closed planar boundary loops filled, their holes kept: the first loop is the outer boundary,
/// the rest are holes.
///
/// # Errors
/// The kernel's refusal, in its words.
pub fn triangulate_planar(loops: &[Vec<[f64; 2]>]) -> Result<PlanarFill, TessRefused> {
    match which(OpKind::Tessellate) {
        Which::Solidcore => Solidcore.triangulate_planar(loops),
    }
}

/// The union of planar regions filled, their holes kept: each region is its loops (the largest
/// its outline, the rest its holes); regions that overlap or abut come back as one surface.
///
/// # Errors
/// The kernel's refusal, in its words.
pub fn triangulate_union(regions: &[Vec<Vec<[f64; 2]>>]) -> Result<PlanarFill, TessRefused> {
    match which(OpKind::Tessellate) {
        Which::Solidcore => Solidcore.triangulate_union(regions),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routed_fills_are_solidcores() {
        let square = vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        let hole = vec![[1.0, 1.0], [1.0, 2.0], [2.0, 2.0], [2.0, 1.0]];
        let loops = vec![square.clone(), hole];
        assert_eq!(triangulate_planar(&loops).ok(), solidcore_tess::triangulate_planar(&loops).ok());
        let other = vec![[3.0, 0.0], [6.0, 0.0], [6.0, 3.0], [3.0, 3.0]];
        let regions = vec![vec![square], vec![other]];
        assert_eq!(triangulate_union(&regions).ok(), solidcore_tess::triangulate_union(&regions).ok());
        // a refusal carries solidcore's words whole
        let words = solidcore_tess::triangulate_planar(&[]).unwrap_err().to_string();
        assert_eq!(triangulate_planar(&[]), Err(TessRefused(words)));
    }
}
