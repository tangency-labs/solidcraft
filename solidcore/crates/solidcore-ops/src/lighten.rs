//! Lighten / Auto-Pocket recipe in `solidcore-ops`: configuration types and B-rep pocket
//! cutting sequence (`union_all` + `BooleanOp::Subtract`).
//!
//! Computes no geometric quantities of its own: 2D profile offsetting and edge-pick collection
//! produce the cutter bodies and edge picks, and this module sequences the kernel boolean cut.

use serde::{Deserialize, Serialize};

use crate::boolean::{BooleanOp, boolean, union_all};
use crate::{Body, Error, Result};

/// Manufacturing method preset controlling pocket corner and depth defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManufacturingMethod {
    /// CNC endmill pocketing with rounded interior corners.
    CncPocket,
    /// Laser or waterjet through-cut pocketing.
    LaserWaterjet,
    /// Additive manufacturing weight-reduction pocket.
    AdditiveWeightReduction,
}

/// Pocket depth specification relative to the selected planar face.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PocketDepthSpec {
    /// Cut to a fixed depth below the target face (mm).
    BlindDepth(f64),
    /// Leave a constant residual floor thickness (mm).
    FloorThickness(f64),
    /// Cut completely through the solid.
    ThroughHole,
}

/// Which pocket edge set receives the automatic fillet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FilletLocation {
    /// Fillet only pocket floor edges.
    #[default]
    Bottom,
    /// Fillet only pocket top entry edges.
    Top,
    /// Fillet both top and bottom pocket edges.
    All,
}

/// Which pocket edge set receives the automatic chamfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChamferLocation {
    /// Chamfer only pocket top entry edges.
    #[default]
    Top,
    /// Chamfer only pocket floor edges.
    Bottom,
    /// Chamfer both top and bottom pocket edges.
    All,
}

/// Parametric configuration for `solid.lighten` auto-pocketing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LightenConfig {
    /// Inward offset from outer boundary and outward keep-out from holes (mm).
    pub wall_offset: f64,
    /// Cut depth mode and value.
    pub depth_spec: PocketDepthSpec,
    /// Interior pocket corner fillet radius (mm).
    pub corner_radius: f64,
    /// Side-wall draft angle (degrees).
    pub draft_angle: f64,
    /// Optional automatic pocket edge fillet radius (mm).
    pub auto_fillet_radius: Option<f64>,
    /// Optional automatic pocket edge chamfer size (mm).
    pub auto_chamfer_size: Option<f64>,
    /// Manufacturing process preset.
    pub method: ManufacturingMethod,
    /// Optional face IDs treated as keep-out islands.
    pub island_faces: Vec<u64>,
}

impl Default for LightenConfig {
    fn default() -> Self {
        Self {
            wall_offset: 4.0,
            depth_spec: PocketDepthSpec::BlindDepth(7.0),
            corner_radius: 3.0,
            draft_angle: 0.0,
            auto_fillet_radius: None,
            auto_chamfer_size: None,
            method: ManufacturingMethod::CncPocket,
            island_faces: Vec::new(),
        }
    }
}

impl LightenConfig {
    /// Validate configuration parameters prior to geometry generation.
    pub fn validate(&self) -> std::result::Result<(), String> {
        if !self.wall_offset.is_finite() || self.wall_offset <= 0.0 {
            return Err("wall_offset must be positive".into());
        }
        if !self.corner_radius.is_finite() || self.corner_radius < 0.0 {
            return Err("corner_radius must be non-negative".into());
        }
        if !self.draft_angle.is_finite() || !(0.0..45.0).contains(&self.draft_angle) {
            return Err("draft_angle must be in [0, 45) degrees".into());
        }
        match self.depth_spec {
            PocketDepthSpec::BlindDepth(d) if !d.is_finite() || d <= 0.0 => {
                return Err("blind pocket depth must be positive".into());
            }
            PocketDepthSpec::FloorThickness(t) if !t.is_finite() || t < 0.0 => {
                return Err("floor thickness must be non-negative".into());
            }
            _ => {}
        }
        if let Some(r) = self.auto_fillet_radius {
            if !r.is_finite() || r <= 0.0 {
                return Err("auto_fillet_radius must be positive".into());
            }
        }
        if let Some(c) = self.auto_chamfer_size {
            if !c.is_finite() || c <= 0.0 {
                return Err("auto_chamfer_size must be positive".into());
            }
        }
        Ok(())
    }
}

/// Subtract `cutters` from `target` in the kernel: when multiple cutters are given, union them
/// first (falling back to sequential subtraction if the union is refused).
///
/// # Errors
///
/// Returns [`Error::Argument`] if `cutters` is empty, or any kernel boolean refusal.
pub fn cut_pockets(target: Body, cutters: Vec<Body>) -> Result<Option<Body>> {
    if cutters.is_empty() {
        return Err(Error::Argument);
    }
    if cutters.len() > 1 {
        if let Ok(copied_target) = target.copy() {
            if let Ok(copied_cutters) = cutters.iter().map(Body::copy).collect::<Result<Vec<_>>>() {
                if let Ok(combined) = union_all(copied_cutters) {
                    if let Ok(cut_body) = boolean(copied_target, combined, BooleanOp::Subtract) {
                        return Ok(cut_body);
                    }
                }
            }
        }
    }
    let mut acc = Some(target);
    for cutter in cutters {
        let Some(current) = acc else {
            return Ok(None);
        };
        acc = boolean(current, cutter, BooleanOp::Subtract)?;
    }
    Ok(acc)
}
