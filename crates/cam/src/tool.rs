//! Cutting tools.

use serde::{Deserialize, Serialize};

use crate::cutter::Cutter;
use crate::{Result, invalid};

/// Largest tool number a post processor is asked to write.
pub const MAX_TOOL_NUMBER: u32 = 9999;
/// Largest tool diameter (mm).
pub const MAX_TOOL_DIAMETER: f64 = 1000.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    FlatEndmill,
    BallEndmill,
    /// A flat end mill with rounded corners (a torus at the end): see [`Tool::corner_radius`].
    BullNose,
    Drill,
    /// A chamfer mill: a flat tip of [`Tool::tip_diameter`], then a cone of
    /// [`Tool::angle_deg`] (the included angle) up to the cutting diameter.
    Chamfer,
}

impl ToolKind {
    pub fn label(self) -> &'static str {
        match self {
            ToolKind::FlatEndmill => "flat end mill",
            ToolKind::BallEndmill => "ball end mill",
            ToolKind::BullNose => "bull nose end mill",
            ToolKind::Drill => "drill",
            ToolKind::Chamfer => "chamfer mill",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tool {
    /// The pocket the machine holds it in (`T` word).
    pub number: u32,
    pub kind: ToolKind,
    /// Cutting diameter (mm).
    pub diameter: f64,
    /// Length of the cutting edges (mm).
    pub flute_length: f64,
    #[serde(default)]
    pub description: String,
    /// Number of cutting edges.
    #[serde(default = "two")]
    pub flutes: u32,
    /// The cutting data operations start from: spindle speed (rev/min) and feeds (mm/min).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spindle_rpm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feed: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plunge_feed: Option<f64>,
    /// The depth per pass and the distance between passes operations start from (mm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stepdown: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stepover: Option<f64>,
    /// Radius of a bull nose's corners (mm), above 0 and below the tool radius. Ignored by
    /// other kinds: a flat end mill and a chamfer mill have none and a ball's is its radius.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner_radius: Option<f64>,
    /// A chamfer mill's tip diameter (mm), from 0 (pointed) to below the cutting diameter;
    /// left out, 0. Only for chamfer mills.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip_diameter: Option<f64>,
    /// A chamfer mill's included angle (degrees), 1 to 179: 90 for a 45° chamfer. Required for
    /// chamfer mills, and only for them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle_deg: Option<f64>,
    /// Diameter of the plain shank above the flutes (mm); left out, the cutting diameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shank_diameter: Option<f64>,
    /// How far the tool stands out of its holder, tip to holder face (mm); left out, the
    /// flute length. Collision checks use it with [`Tool::holder`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stickout: Option<f64>,
    /// The holder from its face up, as stacked cylinders; empty: no holder is checked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holder: Vec<HolderSection>,
}

/// One cylinder of a tool holder.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct HolderSection {
    pub diameter: f64,
    pub length: f64,
}

/// A solid of revolution about the tool axis: a cylinder from `bottom` to `top` above the tip.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub bottom: f64,
    pub top: f64,
    pub radius: f64,
}

fn two() -> u32 {
    2
}

impl Tool {
    pub fn new(number: u32, kind: ToolKind, diameter: f64, flute_length: f64) -> Tool {
        Tool {
            number,
            kind,
            diameter,
            flute_length,
            description: String::new(),
            flutes: 2,
            spindle_rpm: None,
            feed: None,
            plunge_feed: None,
            stepdown: None,
            stepover: None,
            corner_radius: None,
            tip_diameter: None,
            angle_deg: None,
            shank_diameter: None,
            stickout: None,
            holder: Vec::new(),
        }
    }

    /// The cutting end's shape, for lowering the tool onto a surface ([`crate::drop_z`]). A
    /// drill has none: it cuts only at its point.
    pub fn cutter(&self) -> Result<Cutter> {
        let r = self.radius();
        match self.kind {
            ToolKind::FlatEndmill => Cutter::flat(r),
            ToolKind::BallEndmill => Cutter::ball(r),
            ToolKind::BullNose => Cutter::bull(r, self.corner_radius.unwrap_or(0.0)),
            ToolKind::Drill => invalid("a drill cannot mill: it cuts only at its point"),
            ToolKind::Chamfer => {
                let Some(angle) = self.angle_deg else { return invalid("a chamfer mill needs its included angle (`angle_deg`)") };
                Cutter::tapered(r, self.tip_diameter.unwrap_or(0.0) * 0.5, angle.to_radians() * 0.5)
            }
        }
    }

    /// The parts of the tool above the flutes that must not touch the part: the shank from the
    /// flutes to the holder face, then the holder's sections. Heights are above the tip (mm).
    pub fn envelopes(&self) -> Vec<Envelope> {
        let stickout = self.stickout.unwrap_or(self.flute_length).max(self.flute_length);
        let shank = self.shank_diameter.unwrap_or(self.diameter) * 0.5;
        let mut out = Vec::new();
        if stickout > self.flute_length {
            out.push(Envelope { bottom: self.flute_length, top: stickout, radius: shank });
        }
        let mut z = stickout;
        for s in &self.holder {
            out.push(Envelope { bottom: z, top: z + s.length, radius: s.diameter * 0.5 });
            z += s.length;
        }
        out
    }

    pub fn radius(&self) -> f64 {
        self.diameter * 0.5
    }
    pub fn validate(&self) -> Result<()> {
        if self.number > MAX_TOOL_NUMBER {
            return invalid(format!("tool number {} is above {MAX_TOOL_NUMBER}", self.number));
        }
        if !(self.diameter > 0.0) || !(self.diameter <= MAX_TOOL_DIAMETER) {
            return invalid("the tool diameter must be above 0 and at most 1000 mm");
        }
        if !(self.flute_length > 0.0) || !(self.flute_length <= 10_000.0) {
            return invalid("the tool's flute length must be above 0 and at most 10 m");
        }
        if !(1..=100).contains(&self.flutes) {
            return invalid("a tool has 1 to 100 flutes");
        }
        if self.kind == ToolKind::BullNose && !self.corner_radius.is_some_and(|c| c > 0.0 && c < self.radius()) {
            return invalid("a bull nose's corner radius must be above 0 and below the tool radius");
        }
        if self.kind == ToolKind::Chamfer {
            if !self.angle_deg.is_some_and(|a| (1.0..=179.0).contains(&a)) {
                return invalid("a chamfer mill's included angle (`angle_deg`) must be given, from 1 to 179 degrees");
            }
            if self.tip_diameter.is_some_and(|t| !(t >= 0.0) || !(t < self.diameter)) {
                return invalid("a chamfer mill's tip diameter must be from 0 to below its diameter");
            }
        } else if self.tip_diameter.is_some() || self.angle_deg.is_some() {
            return invalid("`tip_diameter` and `angle_deg` are for chamfer mills");
        }
        if self.shank_diameter.is_some_and(|d| !(d > 0.0) || !(d <= MAX_TOOL_DIAMETER)) {
            return invalid("the shank diameter must be above 0 and at most 1000 mm");
        }
        if self.stickout.is_some_and(|s| !(s >= self.flute_length) || !(s <= 10_000.0)) {
            return invalid("the stickout must be at least the flute length and at most 10 m");
        }
        if self.holder.len() > 64 {
            return invalid("a holder has at most 64 sections");
        }
        if self.holder.iter().any(|s| !(s.diameter > 0.0) || !(s.diameter <= 10_000.0) || !(s.length > 0.0) || !(s.length <= 10_000.0)) {
            return invalid("a holder section's diameter and length must be above 0 and at most 10 m");
        }
        if [self.spindle_rpm, self.feed, self.plunge_feed, self.stepdown, self.stepover].iter().flatten().any(|v| !(*v > 0.0) || !(*v <= 1.0e6)) {
            return invalid("a tool's spindle speed, feeds, stepdown and stepover must be above 0");
        }
        Ok(())
    }
}
