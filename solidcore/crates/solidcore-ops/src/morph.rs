//! Surface-to-surface morphing of a solid or sheet body (or a selected subset of its faces).
//!
//! Maps a target body's geometry relative to a base reference surface onto a target reference
//! surface using [`solidcore::deform::deform_brep_by_surface`]. Ops validates record indices,
//! converts through `solidcore-convert`, sequences the kernel call, and converts the result
//! back to a [`solidcore_record::brep::Brep`]; all geometric computation lives in `solidcore`.

use solidcore::deform::{
    deform_brep_by_surface, DeformOptions, MapMode as CoreMapMode,
    ProjectionMode as CoreProjectionMode, SurfaceToSurfaceOptions,
};
use solidcore::geom::vector3d::Vec3;
use solidcore::topology::FaceId;
use solidcore_convert::brep::{to_kernel, to_record, Unsupported};
use solidcore_record::brep::Brep as Record;

use crate::error::status;
use crate::order;
use crate::session::run;
use crate::{Error, Result};

/// How points of the target body are projected onto the base surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MorphProjection {
    /// Closest-point (normal) projection onto the base surface.
    Normal,
    /// Parallel projection along a fixed direction vector.
    Direction([f64; 3]),
}

/// How offsets from the base surface are carried onto the target surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MorphMapMode {
    /// Preserve signed normal distance along the target surface's unit normal.
    NormalToNormal,
    /// Preserve the 3D offset vector directly (`P + (S_target - S_base)`).
    Translate,
}

/// Options for [`morph_by_surface`].
#[derive(Clone, Debug, PartialEq)]
pub struct MorphOptions {
    /// Projection onto the base surface.
    pub projection: MorphProjection,
    /// Mapping from the base surface to the target surface.
    pub map_mode: MorphMapMode,
    /// Deformation scale factor (`1.0` = full target, `0.0` = identity).
    pub scale: f64,
    /// Reverse the normalized `u` parameter between base and target surfaces.
    pub flip_u: bool,
    /// Reverse the normalized `v` parameter between base and target surfaces.
    pub flip_v: bool,
    /// Swap normalized `(u, v)` between base and target surfaces.
    pub swap_uv: bool,
    /// Refitting tolerance in millimetres.
    pub tolerance: f64,
    /// Optional subset of target face record indices to deform. `None` or empty deforms the whole body.
    pub faces: Option<Vec<usize>>,
}

impl Default for MorphOptions {
    fn default() -> Self {
        Self {
            projection: MorphProjection::Normal,
            map_mode: MorphMapMode::NormalToNormal,
            scale: 1.0,
            flip_u: false,
            flip_v: false,
            swap_uv: false,
            tolerance: 1e-2,
            faces: None,
        }
    }
}

/// Morph `target` (a solid or sheet B-rep record) by mapping `base`'s face `base_face_idx`
/// onto `control`'s face `control_face_idx`.
///
/// # Errors
///
/// Returns [`Error::Argument`] if any face index is out of range or options are invalid,
/// [`Error::Record`] if a record cannot be converted, or the kernel's failure status.
pub fn morph_by_surface(
    target: &Record,
    base: &Record,
    base_face_idx: usize,
    control: &Record,
    control_face_idx: usize,
    options: &MorphOptions,
) -> Result<Record> {
    let mut refusal: Option<Error> = None;
    let out = run("morph::by_surface", |k| {
        let (target_brep, target_order) = match to_kernel(k, target, true) {
            Ok(pair) => pair,
            Err(Unsupported::Kernel(e)) => return Err(e),
            Err(_) => {
                refusal = Some(Error::Argument);
                return Ok(None);
            }
        };
        let (mut base_brep, base_order) = match to_kernel(k, base, true) {
            Ok(pair) => pair,
            Err(Unsupported::Kernel(e)) => return Err(e),
            Err(_) => {
                refusal = Some(Error::Argument);
                return Ok(None);
            }
        };
        let (mut control_brep, control_order) = match to_kernel(k, control, true) {
            Ok(pair) => pair,
            Err(Unsupported::Kernel(e)) => return Err(e),
            Err(_) => {
                refusal = Some(Error::Argument);
                return Ok(None);
            }
        };

        let base_face = match order::face(&base_order, base_face_idx) {
            Ok(f) => f,
            Err(e) => {
                refusal = Some(e);
                return Ok(None);
            }
        };
        let control_face = match order::face(&control_order, control_face_idx) {
            Ok(f) => f,
            Err(e) => {
                refusal = Some(e);
                return Ok(None);
            }
        };

        let face_ids: Option<Vec<FaceId>> = match &options.faces {
            None => None,
            Some(indices) if indices.is_empty() => None,
            Some(indices) => {
                let mut ids = Vec::new();
                for &idx in indices {
                    match order::face(&target_order, idx) {
                        Ok(f) => ids.push(f),
                        Err(e) => {
                            refusal = Some(e);
                            return Ok(None);
                        }
                    }
                }
                Some(ids)
            }
        };

        let projection = match options.projection {
            MorphProjection::Normal => CoreProjectionMode::Normal,
            MorphProjection::Direction(d) => {
                CoreProjectionMode::Direction(Vec3::new(d[0], d[1], d[2]))
            }
        };
        let map_mode = match options.map_mode {
            MorphMapMode::NormalToNormal => CoreMapMode::NormalToNormal,
            MorphMapMode::Translate => CoreMapMode::Translate,
        };
        let srf_opts = SurfaceToSurfaceOptions {
            projection,
            map_mode,
            scale: options.scale,
            flip_u: options.flip_u,
            flip_v: options.flip_v,
            swap_uv: options.swap_uv,
            base_forward: true,
            control_forward: true,
            tolerance: options.tolerance,
        };
        let deform_opts = DeformOptions {
            tolerance: options.tolerance,
            faces: face_ids,
        };

        let mut deformed = match deform_brep_by_surface(
            k,
            &target_brep,
            &mut base_brep,
            base_face,
            &mut control_brep,
            control_face,
            srf_opts,
            &deform_opts,
        ) {
            Ok(b) => b,
            Err(e) => {
                refusal = Some(status(e));
                return Ok(None);
            }
        };

        match to_record(k, &mut deformed) {
            Ok((rec, _)) => Ok(Some(rec)),
            Err(Unsupported::Kernel(e)) => Err(e),
            Err(_) => {
                refusal = Some(Error::Argument);
                return Ok(None);
            }
        }
    })?;

    if let Some(e) = refusal {
        return Err(e);
    }
    out.ok_or(Error::Argument)
}
