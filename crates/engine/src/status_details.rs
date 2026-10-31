//! Retained diagnostics for the existing Continuity effect. Queries never evaluate geometry.
use schemars::JsonSchema;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContinuityVerdict {
    Smooth,
    CurvatureJump,
    Sharp,
    Gap,
}
impl From<crate::kernel::SeamVerdict> for ContinuityVerdict {
    fn from(value: crate::kernel::SeamVerdict) -> Self {
        match value {
            crate::kernel::SeamVerdict::Smooth => Self::Smooth,
            crate::kernel::SeamVerdict::CurvatureJump => Self::CurvatureJump,
            crate::kernel::SeamVerdict::Sharp => Self::Sharp,
            crate::kernel::SeamVerdict::Gap => Self::Gap,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct ContinuityDetail {
    pub body: String,
    pub edge: usize,
    pub samples: usize,
    pub gap_tolerance_mm: f64,
    pub angle_tolerance_deg: f64,
    pub curvature_tolerance_per_mm: f64,
    pub gap_mm: f64,
    pub normal_angle_deg: f64,
    pub curvature_jump_per_mm: f64,
    pub length_mm: f64,
    pub verdict: ContinuityVerdict,
    pub gap_at: [f64; 3],
    pub gap_fraction: f64,
    pub normal_angle_at: [f64; 3],
    pub normal_angle_fraction: f64,
    pub curvature_jump_at: [f64; 3],
    pub curvature_jump_fraction: f64,
    pub skipped: usize,
    pub unmeasured: usize,
    /// True only while the exact evaluated geometry and occurrence placement still exist.
    pub current: bool,
    #[serde(skip)]
    pub(crate) geometry_key: Option<u64>,
    #[serde(skip)]
    pub(crate) feature: u64,
}

#[derive(Serialize, JsonSchema)]
pub struct StatusDetailItem {
    pub source: crate::status_items::StatusItem,
    pub has_details: bool,
}

#[derive(Serialize, JsonSchema)]
pub struct StatusDetails {
    pub document_id: String,
    pub items: Vec<StatusDetailItem>,
    pub continuity: Vec<ContinuityDetail>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

impl crate::Session {
    /// Cheap current-state lookup. A pending model evaluation or cold/stale assembly world
    /// is conservatively unavailable, even if the old state's geometry keys still match.
    pub fn continuity_details(&self) -> Vec<ContinuityDetail> {
        let world = if self.model.is_stale() { None } else { self.cached_world_state() };
        self.continuity
            .iter()
            .map(|seam| {
                let mut detail = seam.details.clone();
                detail.verdict = seam.verdict.into();
                detail.current = world.as_ref().and_then(|world| world.body(&seam.body)).is_some_and(|body| {
                    detail.geometry_key.is_some() && body.geometry_key() == detail.geometry_key && body.feature == detail.feature
                });
                detail
            })
            .collect()
    }

    /// Validate again at activation; a button's earlier descriptor is never authority.
    pub fn continuity_target_points(&self, body: &str, edge: usize) -> Option<&[solidcraft_geom::Vec3]> {
        if !self.continuity_details().iter().any(|row| row.body == body && row.edge == edge && row.current) {
            return None;
        }
        self.continuity.iter().find(|seam| seam.body == body && seam.edge == edge).map(|seam| seam.points.as_slice())
    }
}
