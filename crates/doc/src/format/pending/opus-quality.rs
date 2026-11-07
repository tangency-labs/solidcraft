//! Imported mesh sources may save optional `corner_normals`: nine f64 values per
//! triangle, independent of positions, topology and optical boundary certification.
//! Missing/empty normals retain the previous flat shading. OBJ `vn` and `s` import
//! preserve authored corner normals and smoothing groups. Explicit crease generation
//! stores only shading metadata and preserves original coordinates and triangle indices.
//! Last released readers refuse this design format instead of silently losing normals.
//!
//! Render jobs may save opt-in `texture_filtering`; omission/false preserves LOD0
//! texture sampling. Supported consumers use the captured texture's mip chain and
//! ray footprint without changing source image identity or source color semantics.
//!
//! Optional `metal_energy_compensation` restores a bounded share of rough metal
//! reflectance in both path tracers. Omission or false keeps the previous BRDF.
//!
//! Caustics options may save `area_lights: true` and `environment: true` for
//! both path tracers. Omission preserves the hard-source photon map.
