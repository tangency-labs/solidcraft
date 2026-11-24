//! Surface-to-surface morphing of a solid or sheet body (or a selected subset of its faces).
//!
//! Thin kernel-boundary wrapper over [`solidcore_ops::morph::morph_by_surface`].

use serde::{Deserialize, Serialize};
use solidcore_ops::morph::{MorphMapMode as OpsMapMode, MorphOptions as OpsOptions, MorphProjection as OpsProjection, morph_by_surface};

use crate::body::Body;
use crate::{KernelError, Result, guard};

/// How points of the target body are projected onto the base surface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MorphProjection {
    /// Closest-point (normal) projection onto the base surface.
    #[default]
    Normal,
    /// Parallel projection along a fixed direction vector.
    Direction { dir: [f64; 3] },
}

/// How offsets from the base surface are carried onto the target surface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MorphMapMode {
    /// Preserve signed normal distance along the target surface's unit normal.
    #[default]
    NormalToNormal,
    /// Preserve the 3D offset vector directly (`P + (S_target - S_base)`).
    Translate,
}

/// Options for [`morph_surface`].
#[derive(Clone, Debug, PartialEq)]
pub struct MorphOptions {
    pub projection: MorphProjection,
    pub map_mode: MorphMapMode,
    pub scale: f64,
    pub flip_u: bool,
    pub flip_v: bool,
    pub swap_uv: bool,
    pub tolerance: f64,
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
        }
    }
}

/// Morph `target` (or the selected `faces` of `target`) by mapping `base`'s face `base_face`
/// onto `control`'s face `control_face`.
pub fn morph_surface(
    target: &Body,
    faces: &[usize],
    base: &Body,
    base_face: usize,
    control: &Body,
    control_face: usize,
    options: &MorphOptions,
) -> Result<Body> {
    if crate::watch::cancelled() {
        return Err(crate::watch::cancelled_error("morph"));
    }
    target.require_brep("Morph")?;
    base.require_brep("Morph base surface")?;
    control.require_brep("Morph target surface")?;
    if !options.scale.is_finite() {
        return Err(KernelError::Invalid("scale must be finite".into()));
    }
    if !options.tolerance.is_finite() || options.tolerance <= 0.0 {
        return Err(KernelError::Invalid("tolerance must be positive and finite".into()));
    }

    let ops_opts = OpsOptions {
        projection: match options.projection {
            MorphProjection::Normal => OpsProjection::Normal,
            MorphProjection::Direction { dir } => OpsProjection::Direction(dir),
        },
        map_mode: match options.map_mode {
            MorphMapMode::NormalToNormal => OpsMapMode::NormalToNormal,
            MorphMapMode::Translate => OpsMapMode::Translate,
        },
        scale: options.scale,
        flip_u: options.flip_u,
        flip_v: options.flip_v,
        swap_uv: options.swap_uv,
        tolerance: options.tolerance,
        faces: if faces.is_empty() { None } else { Some(faces.to_vec()) },
    };

    let record = guard("Morph", || {
        morph_by_surface(target.record(), base.record(), base_face, control.record(), control_face, &ops_opts)
            .map_err(|e| KernelError::Failed(format!("Morph: {e}")))
    })?;

    Body::from_smlib_record(record)
}
