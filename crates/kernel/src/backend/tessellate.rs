//! solidcore's planar fills ([`Tessellate`]): the solidcore-tess calls this crate's callers made
//! before the interface existed, their refusals' words kept whole.

use solidcraft_kernel_api::tessellate::{PlanarFill, TessRefused, Tessellate};

use super::Solidcore;

impl Tessellate for Solidcore {
    fn triangulate_planar(&self, loops: &[Vec<[f64; 2]>]) -> Result<PlanarFill, TessRefused> {
        solidcore_tess::triangulate_planar(loops).map_err(|e| TessRefused(e.to_string()))
    }

    fn triangulate_union(&self, regions: &[Vec<Vec<[f64; 2]>>]) -> Result<PlanarFill, TessRefused> {
        solidcore_tess::triangulate_union(regions).map_err(|e| TessRefused(e.to_string()))
    }
}
