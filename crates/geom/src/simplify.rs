//! Display levels of detail for triangle meshes. Simplification and error measurement live in
//! solidcore-tess.

pub use solidcore_tess::simplify::{Level, MAX_TRIANGLES, levels, levels_from_indexed_facets};

#[cfg(test)]
#[path = "simplify_tests.rs"]
mod tests;
