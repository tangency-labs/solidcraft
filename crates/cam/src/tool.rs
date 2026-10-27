//! Cutting tools.

use serde::{Deserialize, Serialize};

use crate::{Result, invalid};

/// Young's modulus (GPa) for a catalogue material name (`carbide`, `hss`, …).
/// Carbide is 600 GPa as in the #1621 stock-integrity research (cantilever example).
pub fn youngs_modulus_gpa(material: &str) -> f64 {
    match material {
        "carbide" => 600.0,
        "ceramic" => 300.0,
        "diamond" => 1050.0,
        "hss" | "cobalt" => 210.0,
        _ => 210.0,
    }
}

/// Cantilever stiffness `k = 3EI/L³` in N/µm. Diameter and stick-out are millimetres.
/// Used for the tool assembly and for a wall treated as a round bar; walls use [`wall_stiffness_n_per_um`].
pub fn cantilever_stiffness_n_per_um(youngs_gpa: f64, diameter_mm: f64, stick_out_mm: f64) -> f64 {
    let l = stick_out_mm.max(0.1);
    let d = diameter_mm.max(0.1);
    let e = youngs_gpa * 1_000.0; // N/mm²
    let i = std::f64::consts::PI / 64.0 * d.powi(4);
    (3.0 * e * i / l.powi(3)) / 1_000.0
}

/// Wall as a rectangular cantilever: height `L`, thickness `t`, loaded length `b`, `I = b t³ / 12`.
/// Matches the #1621 first-estimate formula (`δ = F c³ / (3 E I)`).
pub fn wall_stiffness_n_per_um(youngs_gpa: f64, thickness_mm: f64, height_mm: f64, length_mm: f64) -> f64 {
    let l = height_mm.max(0.1);
    let t = thickness_mm.max(0.01);
    let b = length_mm.max(0.1);
    let e = youngs_gpa * 1_000.0;
    let i = b * t.powi(3) / 12.0;
    (3.0 * e * i / l.powi(3)) / 1_000.0
}

/// Tooth-passing frequency `f_tp = n · z / 60` (Hz).
pub fn tooth_passing_hz(spindle_rpm: f64, flutes: u32) -> f64 {
    if spindle_rpm <= 0.0 || flutes == 0 {
        0.0
    } else {
        spindle_rpm * f64::from(flutes) / 60.0
    }
}

/// Largest tool number a post processor is asked to write.
pub const MAX_TOOL_NUMBER: u32 = 9999;
/// Largest tool diameter (mm).
pub const MAX_TOOL_DIAMETER: f64 = 1000.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    FlatEndmill,
    BallEndmill,
    Drill,
}

impl ToolKind {
    pub fn label(self) -> &'static str {
        match self {
            ToolKind::FlatEndmill => "flat end mill",
            ToolKind::BallEndmill => "ball end mill",
            ToolKind::Drill => "drill",
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
    /// Stick-out from the holder face (mm). Drives assembly stiffness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stick_out: Option<f64>,
    /// Shaft / core diameter used for the cantilever (mm). Omitted, the cutting diameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shaft_diameter: Option<f64>,
    /// Catalogue material: `carbide`, `hss`, `cobalt`, `ceramic`, `diamond`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// Holder gauge length (mm), when an assembly is recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holder_gauge: Option<f64>,
    /// Holder stiffness at the gauge face (N/µm). Omitted until measured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holder_stiffness: Option<f64>,
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
            stick_out: None,
            shaft_diameter: None,
            material: None,
            holder_gauge: None,
            holder_stiffness: None,
        }
    }
    pub fn radius(&self) -> f64 {
        self.diameter * 0.5
    }

    /// Effective core diameter for the cantilever: shaft when set, else 80 % of the cutting diameter.
    pub fn core_diameter(&self) -> f64 {
        self.shaft_diameter.filter(|d| *d > 0.0).unwrap_or(self.diameter * 0.8)
    }

    /// Stick-out used for stiffness. Falls back to flute length when unset.
    pub fn stick_out_mm(&self) -> f64 {
        self.stick_out.filter(|l| *l > 0.0).unwrap_or(self.flute_length)
    }

    /// Estimated tool-assembly stiffness (N/µm) from `3EI/L³`.
    pub fn stiffness_n_per_um(&self) -> f64 {
        let mat = self.material.as_deref().unwrap_or("carbide");
        cantilever_stiffness_n_per_um(youngs_modulus_gpa(mat), self.core_diameter(), self.stick_out_mm())
    }

    /// Series combination of tool and holder stiffness when a holder value is present.
    pub fn assembly_stiffness_n_per_um(&self) -> f64 {
        let k_tool = self.stiffness_n_per_um();
        match self.holder_stiffness.filter(|k| *k > 0.0) {
            Some(k_holder) => {
                let sum = 1.0 / k_tool.max(1e-9) + 1.0 / k_holder;
                1.0 / sum
            }
            None => k_tool,
        }
    }

    pub fn tooth_passing_hz(&self, spindle_rpm: f64) -> f64 {
        tooth_passing_hz(spindle_rpm, self.flutes)
    }
    pub fn validate(&self) -> Result<()> {
        if self.number > MAX_TOOL_NUMBER {
            return invalid(format!("tool number {} is above {MAX_TOOL_NUMBER}", self.number));
        }
        if !(self.diameter > 0.0) || !(self.diameter <= MAX_TOOL_DIAMETER) {
            return invalid("the tool diameter must be above 0 and at most 1000 mm");
        }
        if !(self.flute_length > 0.0) || !self.flute_length.is_finite() {
            return invalid("the tool's flute length must be above 0");
        }
        if !(1..=100).contains(&self.flutes) {
            return invalid("a tool has 1 to 100 flutes");
        }
        if [self.spindle_rpm, self.feed, self.plunge_feed, self.stepdown, self.stepover].iter().flatten().any(|v| !(*v > 0.0) || !(*v <= 1.0e6)) {
            return invalid("a tool's spindle speed, feeds, stepdown and stepover must be above 0");
        }
        Ok(())
    }
}
