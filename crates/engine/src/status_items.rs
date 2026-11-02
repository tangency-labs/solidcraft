//! A read-only declaration of persistent viewport effects. Settings remain in the session's
//! existing effect fields; only activation order is recorded separately, never in a design.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Session, SurfaceAnalysis};

/// Stable identity of an effect slot, retained when its analysis is replaced.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusId {
    SurfaceAnalysis,
    /// Independent Continuity slot when the compatibility policy is enabled.
    Continuity,
    Section,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusKind {
    Mode,
    Result,
    Attention,
}

/// Structured settings, without unit formatting or translated text.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum StatusSetting {
    Zebra {
        stripes: f64,
    },
    Draft {
        /// Normalised pull direction.
        pull: [f64; 3],
        /// The stored Draft threshold is in degrees.
        angle_deg: f64,
    },
    Environment,
    Accessibility {
        /// Normalised direction from which the tool approaches.
        direction: [f64; 3],
    },
    Curvature {
        radius_mm: f64,
    },
    Continuity {
        /// Number of persistent seam records currently shaded.
        edge_count: usize,
    },
    Section {
        origin_mm: [f64; 3],
        /// Normalised world-space plane normal.
        normal: [f64; 3],
        hatch: bool,
    },
}

/// An executable request that ends this effect, through the same command as other clients.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StatusClear {
    pub command: String,
    pub params: StatusClearParams,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StatusClearParams {
    pub clear: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StatusItem {
    pub id: StatusId,
    pub kind: StatusKind,
    /// Source command ID, whose catalog label consumers can translate.
    pub command: String,
    pub setting: StatusSetting,
    pub has_key: bool,
    pub clear: StatusClear,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StatusItems {
    /// Oldest active slot first; replacing an analysis retains its position.
    pub items: Vec<StatusItem>,
}

impl StatusItem {
    /// Capability independent of a window's presentation preferences. `has_key` retains
    /// the existing read-only legend contract; live details also include settings/findings.
    pub fn has_details(&self) -> bool {
        !matches!(self.setting, StatusSetting::Environment)
    }
}

impl Session {
    /// Whether shading and Continuity share one slot (the default). This is session
    /// configuration, never design data; SDK clients may choose the legacy independent policy.
    pub fn single_surface_analysis(&self) -> bool {
        self.single_surface_analysis
    }

    /// Change the analysis policy without changing the design or undo history. Enabling a
    /// single slot keeps the latest successful analysis activation/replacement and retains the
    /// oldest analysis-group position relative to Section. Queries never perform this transition.
    pub fn set_single_surface_analysis(&mut self, single: bool) {
        if self.single_surface_analysis == single {
            return;
        }
        let before = self.status_items();
        if single && self.analysis.is_some() && !self.continuity.is_empty() {
            if self.latest_analysis == Some(StatusId::Continuity) {
                self.analysis = None;
            } else {
                self.continuity.clear();
            }
        }
        self.single_surface_analysis = single;
        let shading = self.analysis.is_some();
        let continuity = !self.continuity.is_empty();
        let section = self.section.is_some();
        self.status_activation.retain(|source| match source {
            StatusId::SurfaceAnalysis => shading,
            StatusId::Continuity => continuity,
            StatusId::Section => section,
        });
        self.status_order.clear();
        for item in &before {
            let id = match item.id {
                StatusId::SurfaceAnalysis | StatusId::Continuity if single => StatusId::SurfaceAnalysis,
                StatusId::SurfaceAnalysis if self.analysis.is_none() => StatusId::Continuity,
                id => id,
            };
            let active = match id {
                StatusId::SurfaceAnalysis => self.analysis.is_some() || (single && !self.continuity.is_empty()),
                StatusId::Continuity => !self.continuity.is_empty(),
                StatusId::Section => self.section.is_some(),
            };
            self.status_slot(id, active);
        }
        if self.status_items() != before {
            self.refresh_display();
        }
    }

    /// After a successful activation, or a clear (`None`), reconcile only chronology and the
    /// explicitly configured sharing policy. No duplicate effect settings are kept here.
    pub(crate) fn status_analysis_changed(&mut self, activated: Option<StatusId>) {
        if let Some(source) = activated
            && self.single_surface_analysis
        {
            match source {
                StatusId::SurfaceAnalysis => self.continuity.clear(),
                StatusId::Continuity => self.analysis = None,
                StatusId::Section => {}
            }
        }
        // A successful Continuity measurement may find only open boundaries and no seams.
        // An ended/empty effect cannot become the winning source on the next policy change.
        let shading = self.analysis.is_some();
        let continuity = !self.continuity.is_empty();
        let active = |source: &StatusId| match source {
            StatusId::SurfaceAnalysis => shading,
            StatusId::Continuity => continuity,
            StatusId::Section => false,
        };
        let remaining = if shading {
            Some(StatusId::SurfaceAnalysis)
        } else if continuity {
            Some(StatusId::Continuity)
        } else {
            None
        };
        self.status_activation.retain(|id| *id == StatusId::Section || active(id));
        if let Some(id) = activated.filter(active) {
            self.status_activated(id);
        }
        self.latest_analysis = activated.filter(active).or(self.latest_analysis.filter(active)).or(remaining);
        self.status_slot(StatusId::SurfaceAnalysis, self.analysis.is_some() || (self.single_surface_analysis && !self.continuity.is_empty()));
        self.status_slot(StatusId::Continuity, !self.single_surface_analysis && !self.continuity.is_empty());
    }

    /// Current effects, derived without evaluating the model or changing any session state.
    pub fn status_items(&self) -> Vec<StatusItem> {
        let analysis = self
            .analysis
            .map(|a| match a {
                SurfaceAnalysis::Zebra { stripes } => ("inspect.zebra", StatusSetting::Zebra { stripes }, false),
                SurfaceAnalysis::Draft { pull, angle } => ("inspect.draft", StatusSetting::Draft { pull: pull.into(), angle_deg: angle }, true),
                SurfaceAnalysis::Environment => ("inspect.environment_map", StatusSetting::Environment, false),
                SurfaceAnalysis::Access { dir } => ("inspect.accessibility", StatusSetting::Accessibility { direction: dir.into() }, true),
                SurfaceAnalysis::Curvature { radius } => ("inspect.curvature_map", StatusSetting::Curvature { radius_mm: radius }, true),
            })
            .or_else(|| {
                (self.single_surface_analysis && !self.continuity.is_empty()).then_some((
                    "inspect.continuity",
                    StatusSetting::Continuity { edge_count: self.continuity.len() },
                    false,
                ))
            });
        let analysis = analysis.map(|(command, setting, has_key)| item(StatusId::SurfaceAnalysis, command, setting, has_key));
        let continuity = (!self.single_surface_analysis && !self.continuity.is_empty())
            .then(|| item(StatusId::Continuity, "inspect.continuity", StatusSetting::Continuity { edge_count: self.continuity.len() }, false));
        let section = self.section.map(|(origin, normal)| {
            item(
                StatusId::Section,
                "inspect.section",
                StatusSetting::Section { origin_mm: origin.into(), normal: normal.into(), hatch: self.section_hatch },
                false,
            )
        });
        // Public effect fields can also be populated by embedders without command chronology.
        // Include those effects after recorded slots, without inventing or storing timestamps.
        let mut items = Vec::new();
        for id in self.status_order.iter().chain([StatusId::SurfaceAnalysis, StatusId::Continuity, StatusId::Section].iter()) {
            let effect = match id {
                StatusId::SurfaceAnalysis => &analysis,
                StatusId::Continuity => &continuity,
                StatusId::Section => &section,
            };
            if let Some(effect) = effect
                && !items.iter().any(|item: &StatusItem| item.id == *id)
            {
                items.push(effect.clone());
            }
        }
        items
    }

    /// Most recently activated effect among the caller's currently presented slots.
    /// Row position is deliberately independent: replacing an analysis retains its slot.
    /// Effects assigned directly by embedders have no activation record; their declared
    /// order provides a deterministic fallback without inventing session timestamps.
    pub fn newest_status_item(&self, eligible: &[StatusId]) -> Option<StatusItem> {
        let items = self.status_items();
        let find = |source: StatusId| {
            items
                .iter()
                .find(|item| {
                    eligible.contains(&item.id)
                        && match source {
                            StatusId::SurfaceAnalysis => item.id == StatusId::SurfaceAnalysis && item.command != "inspect.continuity",
                            StatusId::Continuity => item.command == "inspect.continuity",
                            StatusId::Section => item.id == StatusId::Section,
                        }
                })
                .cloned()
        };
        self.status_activation.iter().rev().find_map(|id| find(*id)).or_else(|| items.into_iter().rev().find(|item| eligible.contains(&item.id)))
    }

    pub(crate) fn status_activated(&mut self, id: StatusId) {
        self.status_activation.retain(|source| *source != id);
        self.status_activation.push(id);
    }

    pub(crate) fn status_slot(&mut self, id: StatusId, active: bool) {
        if !active {
            self.status_order.retain(|slot| *slot != id);
            if id == StatusId::Section {
                self.status_activation.retain(|source| *source != id);
            }
        } else if !self.status_order.contains(&id) {
            self.status_order.push(id);
        }
    }
}

fn item(id: StatusId, command: &str, setting: StatusSetting, has_key: bool) -> StatusItem {
    StatusItem {
        id,
        kind: StatusKind::Mode,
        command: command.into(),
        setting,
        has_key,
        clear: StatusClear { command: command.into(), params: StatusClearParams { clear: true } },
    }
}
