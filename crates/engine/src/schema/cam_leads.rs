//! The lead, entry, linking, tab and finishing keys of `cam.job`'s `contour` and `pocket`
//! operations (docs/cam-jobs.md, "Leads, entries, linking and tabs"). Like the rest of the job,
//! every object refuses keys it does not know.

use schemars::JsonSchema;
use serde::Deserialize;

/// Most tabs on one profile (`tabs.count`).
pub const MAX_TABS: usize = solidcraft_cam::MAX_TABS;
/// Smallest radius of a lead or a helix, mm.
pub const MIN_ARC_RADIUS: f64 = solidcraft_cam::MIN_ARC_RADIUS;
/// Steepest a ramp, a helix or a tab's ramp may be, degrees.
pub const MAX_ANGLE_DEG: f64 = 85.0;
/// Steepness of a ramp or helix entry that gives none, degrees.
pub const ENTRY_DEG: f64 = 2.0;

/// A tangent arc into and out of each pass of a contour, in the plane of the cut, on the side of
/// the pass away from the part. Refused where it would cut the part.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LeadSpec {
    /// Radius of the arc, mm.
    pub radius: f64,
    /// How far round the arc turns, degrees, 1 to 180. Default: 90.
    #[serde(default)]
    pub sweep_deg: Option<f64>,
    /// A straight line along the arc before the lead-in and after the lead-out, mm. Default: 0.
    #[serde(default)]
    pub line: Option<f64>,
}

/// How the tool goes down into each depth pass.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EntrySpec {
    /// Straight down at the plunge feed.
    Plunge,
    /// Down along the pass: a contour along its lead (back and forth), or before its first tab,
    /// else round the loop; a pocket round its ring.
    Ramp {
        /// Steepest descent, degrees. Default: 2.
        #[serde(default)]
        angle_deg: Option<f64>,
    },
    /// Down round a circle tangent to the pass at its start, then once round at the depth. A
    /// helix that does not fit is made smaller, then replaced by the ramp (then the plunge).
    Helix {
        /// Radius of the circle, mm, at most the tool's radius. Default: 90% of the tool's radius.
        #[serde(default)]
        radius: Option<f64>,
        /// Steepest descent, degrees. Default: 2.
        #[serde(default)]
        angle_deg: Option<f64>,
    },
}

/// Where the tool goes between passes it cannot link in the cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LinkingSpec {
    /// Up to the clearance height.
    Clearance,
    /// Up to the retract height where the part leaves room above the link, else to the clearance
    /// height; a pocket also stays down from one level to the next.
    StayDown,
}

/// Holding tabs on an outside contour that cuts through the stock: material left standing
/// `height` above the bottom, placed evenly along the profile on its straight stretches.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TabsSpec {
    /// How many tabs on each profile. Give this or `spacing`.
    #[serde(default)]
    pub count: Option<u32>,
    /// About how far apart along the profile, mm (at least one tab). Give this or `count`.
    #[serde(default)]
    pub spacing: Option<f64>,
    /// Length of each tab along the wall, mm.
    pub width: f64,
    /// Height of the tabs above the bottom, mm, less than the depth of the cut.
    pub height: f64,
    /// Steepness of the ramps onto and off each tab, degrees. Default: 30.
    #[serde(default)]
    pub ramp_angle_deg: Option<f64>,
}
