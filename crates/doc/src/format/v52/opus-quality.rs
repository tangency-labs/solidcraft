//! Imported mesh sources may save optional `corner_normals`: nine f64 values per
//! triangle, independent of positions, topology and optical boundary certification.
//! Missing/empty normals retain the previous flat shading. OBJ `vn` and `s` import
//! preserve authored corner normals and smoothing groups. Explicit crease generation
//! stores only shading metadata and preserves original coordinates and triangle indices.
//! Older released readers warn about this format and retain fields they cannot read.
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
//!
//! Caustics may also save `guided_emission: true` to concentrate photon emission
//! around captured optical bounds with probability-corrected power. Omission or
//! false preserves the original emission proposal. Older released readers warn
//! about the newer format and retain fields they cannot interpret when saving.
//!
//! Caustics may save `refresh_per_pass: true` to build one photon map per camera
//! pass. Omission or false retains one map per render job.
//!
//! Caustics may save `power_weighted_sources: true` to allocate photon launches
//! using estimated source power with a uniform support component. Each source
//! retains its matching probability correction. Omission or false preserves
//! uniform source selection and the original random sequence.
//!
//! CPU jobs may save `environment_importance_sampling: true` to use the captured
//! HDR radiance distribution for lighting and photon emission. Omission or false
//! retains uniform sphere sampling. Older readers warn and retain unknown fields.
