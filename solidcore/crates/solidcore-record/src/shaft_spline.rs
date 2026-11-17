//! A spline on a shaft and in its hub as data: the numbers it is planned from, the outlines the
//! kernel makes of them (`solidcore::teeth`), and the numbers the outlines come to. Plain data:
//! the meaning of each number, every check and every outline are the kernel's.
use serde::{Deserialize, Serialize};

/// The kind of teeth of a shaft spline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplineForm {
    /// Involute flanks, as a gear's.
    #[default]
    Involute,
    /// Straight-sided teeth of one width.
    Straight,
    /// Serrations: straight flanks at an angle to each other.
    Serration,
}

/// Which new part a shaft spline makes when no face is picked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplineMake {
    /// The splined shaft.
    #[default]
    Shaft,
    /// The hub that fits it.
    Hub,
    /// Both.
    Both,
}

/// The finish of the corner where a straight-sided tooth's flank meets the major circle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplineCorner {
    /// Left sharp.
    #[default]
    Plain,
    /// A chamfer `tip_size` down the flank.
    Chamfered,
    /// A round of radius `tip_size`.
    Rounded,
}

/// The master spline: one tooth made different so that the hub goes on one way only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplineMaster {
    /// Every tooth alike.
    #[default]
    None,
    /// A tooth left out of the shaft, its space in the hub left uncut.
    ToothOut,
    /// A space of the shaft left uncut (a wide tooth), a tooth left out of the hub.
    WideTooth,
}

/// A shaft spline's numbers as plain values: lengths in millimetres, angles in degrees. A number
/// left out takes the kernel's own value, or is required, as the kernel's plan says.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SplineNumbers {
    /// The kind of teeth.
    pub form: SplineForm,
    /// Which new part is made.
    pub make: SplineMake,
    /// The count of teeth.
    pub teeth: u32,
    /// How far the teeth run along the axis.
    pub length: f64,
    /// Involute: the module, the pitch diameter divided by the count of teeth.
    pub module: Option<f64>,
    /// Involute: the pressure angle.
    pub pressure_angle: Option<f64>,
    /// Involute: the height of the shaft's teeth above the pitch circle, in modules.
    pub addendum: Option<f64>,
    /// Involute: the depth of the shaft's spaces below the pitch circle, in modules.
    pub dedendum: Option<f64>,
    /// Involute: the radius of the cutting tool's tip, in modules.
    pub root_fillet: Option<f64>,
    /// Involute: the profile shift, in modules.
    pub profile_shift: Option<f64>,
    /// Involute: what is taken off each tooth's thickness on the pitch circle.
    pub thinning: Option<f64>,
    /// Involute: the height of the hub's teeth inside the pitch circle, in modules.
    pub hub_addendum: Option<f64>,
    /// Involute: the gap between the shaft's tips and the hub's roots.
    pub tip_clearance: Option<f64>,
    /// Involute: the least gap between the hub's tips and the start of the shaft's root fillet.
    pub form_clearance: Option<f64>,
    /// Straight-sided and serration: the diameter over the shaft's tips.
    pub major_diameter: Option<f64>,
    /// Straight-sided and serration: the diameter at the bottoms of the shaft's spaces.
    pub minor_diameter: Option<f64>,
    /// Straight-sided: the width of a tooth.
    pub tooth_width: Option<f64>,
    /// Serration: the angle between the two flanks of a tooth.
    pub flank_angle: Option<f64>,
    /// Straight-sided and serration: the round where a flank meets the minor circle.
    pub root_round: Option<f64>,
    /// Straight-sided and serration: the finish of the tip corners.
    pub tip_corner: SplineCorner,
    /// The size of a chamfered or rounded tip corner.
    pub tip_size: Option<f64>,
    /// Straight-sided and serration: the gap at the major circle.
    pub major_clearance: Option<f64>,
    /// Straight-sided and serration: the gap at the minor circle.
    pub minor_clearance: Option<f64>,
    /// Straight-sided and serration: the round on the tips of the hub's teeth.
    pub hub_tip_round: Option<f64>,
    /// The play between the flanks.
    pub side_clearance: Option<f64>,
    /// The round at the roots of the hub's teeth.
    pub hub_root_round: Option<f64>,
    /// The outside diameter of a new hub.
    pub hub_diameter: Option<f64>,
    /// The master spline.
    pub master: SplineMaster,
    /// Where the middle of the master spline sits.
    pub master_angle: Option<f64>,
    /// The diameter of the cutter that ends the tooth spaces of a shaft.
    pub cutter_diameter: Option<f64>,
    /// The chamfer on the ends of a shaft's teeth.
    pub end_chamfer: Option<f64>,
    /// The chamfer where the teeth run out of a hub's bore.
    pub entry_chamfer: Option<f64>,
    /// Whether a hub is made or cut.
    pub with_hub: bool,
    /// Whether a shaft is made or cut.
    pub with_shaft: bool,
}

/// The numbers a shaft spline comes to. Diameters and gaps in millimetres.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SplineMade {
    /// The count of teeth.
    pub teeth: u32,
    /// The diameter over the shaft's tips.
    pub shaft_major_diameter: f64,
    /// The diameter at the bottoms of the shaft's spaces.
    pub shaft_minor_diameter: f64,
    /// The diameter at the bottoms of the hub's spaces.
    pub hub_major_diameter: f64,
    /// The diameter over the tips of the hub's teeth.
    pub hub_minor_diameter: f64,
    /// Involute: the pitch diameter.
    pub pitch_diameter: Option<f64>,
    /// Involute: the base diameter.
    pub base_diameter: Option<f64>,
    /// Involute: the diameter where the shaft's flanks start.
    pub form_diameter: Option<f64>,
    /// Involute: whether the cutting tool undercuts the shaft's teeth.
    pub undercut: Option<bool>,
    /// Serration: the diameter the flanks would meet at.
    pub sharp_tip_diameter: Option<f64>,
    /// The play on each side.
    pub side_clearance_each: f64,
    /// The gap at the tips of the shaft's teeth.
    pub tip_gap: f64,
    /// The gap at the roots of the shaft's teeth.
    pub root_gap: f64,
    /// Involute: the gap at the start of the shaft's root fillet.
    pub form_gap: Option<f64>,
    /// How far the hub turns on the shaft before the flanks touch, in degrees.
    pub turning_play: f64,
    /// In how many positions the hub goes on: the count of teeth, or 1 with a master spline.
    pub positions: u32,
    /// With a master spline: the tooth left out, or the tooth before the space left uncut,
    /// counted counter-clockwise from the tooth on angle 0.
    pub master_tooth: Option<u32>,
    /// With a master spline: how far the whole spline was turned to put it at its angle, in
    /// degrees.
    pub master_turn: Option<f64>,
    /// How far the involute flanks and fillets may be from the true curves.
    pub flank_tolerance: Option<f64>,
}

/// Why the numbers make nothing: the number the refusal is about, and the words, which name it
/// in backticks first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SplineRefusal {
    /// The number to change, where the refusal names one.
    pub key: Option<String>,
    /// The refusal in plain words.
    pub words: String,
}

impl std::fmt::Display for SplineRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.words)
    }
}

impl std::error::Error for SplineRefusal {}

/// What a piece of a tooth's outline is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToothRole {
    /// A working flank.
    Flank,
    /// The round between a flank and the root.
    Fillet,
    /// The top of a tooth, and its corners.
    Tip,
    /// The bottom of the space between two teeth.
    Root,
}

/// One piece of a tooth's outline, in the plane square to the axis. The pieces of a tooth follow
/// each other counter-clockwise around the axis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutlinePiece {
    /// A circular arc from the angle `start` (radians, from the x axis) sweeping `sweep`
    /// (positive: counter-clockwise).
    Arc {
        /// The centre of its circle.
        centre: [f64; 2],
        /// The radius of its circle.
        radius: f64,
        /// The angle it starts at.
        start: f64,
        /// How far it sweeps.
        sweep: f64,
        /// What the piece is.
        role: ToothRole,
    },
    /// A straight line.
    Line {
        /// Where it starts.
        from: [f64; 2],
        /// Where it ends.
        to: [f64; 2],
        /// What the piece is.
        role: ToothRole,
    },
    /// A curve as points close enough together to stand for it.
    Curve {
        /// The points, in order.
        points: Vec<[f64; 2]>,
        /// What the piece is.
        role: ToothRole,
    },
    /// A B-spline curve.
    Spline {
        /// Its degree.
        degree: usize,
        /// Its knots.
        knots: Vec<f64>,
        /// Its control points.
        control: Vec<[f64; 2]>,
        /// What the piece is.
        role: ToothRole,
    },
}

impl OutlinePiece {
    /// What the piece is.
    #[must_use]
    pub fn role(&self) -> ToothRole {
        match self {
            OutlinePiece::Arc { role, .. }
            | OutlinePiece::Line { role, .. }
            | OutlinePiece::Curve { role, .. }
            | OutlinePiece::Spline { role, .. } => *role,
        }
    }
}

/// The outline of a toothed part about its axis: one tooth, and how many times it repeats.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToothOutline {
    /// How many teeth the part has.
    pub teeth: u32,
    /// The angle from one tooth to the next, in radians.
    pub pitch_angle: f64,
    /// The pieces of one tooth, counter-clockwise. The other teeth are this one turned by whole
    /// pitches.
    pub tooth: Vec<OutlinePiece>,
    /// How far a chord between two points of the outline may be from the curve.
    pub tolerance: f64,
    /// The smallest distance of the outline from the axis.
    pub least_radius: f64,
    /// The largest distance of the outline from the axis.
    pub most_radius: f64,
}

/// An arc about the axis: where a master spline closed a space or left a tooth out.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MasterArc {
    /// The radius of the arc's circle.
    pub radius: f64,
    /// The angle it starts at, in radians.
    pub start: f64,
    /// The angle it ends at, in radians.
    pub end: f64,
}

/// A master spline as the kernel placed it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MasterPlaced {
    /// The tooth left out, or the tooth before the space left uncut, counted counter-clockwise
    /// from the tooth on angle 0.
    pub tooth: u32,
    /// How far the whole spline was turned to put the master spline at its angle, in radians.
    pub turn: f64,
    /// The angle of the middle of the master spline, in radians.
    pub middle: f64,
    /// The arc that stands in the shaft's outline for what was changed.
    pub shaft_arc: MasterArc,
    /// The arc that stands in the hub's outline for what was changed.
    pub hub_arc: MasterArc,
    /// `hub_arc` turned by the hub's start, so that it lies over the shaft.
    pub hub_arc_on_shaft: MasterArc,
}

/// What the numbers make, before any body is touched.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SplinePlan {
    /// The shaft's outline, with the master spline cut in and the whole spline turned.
    pub shaft: ToothOutline,
    /// The outline of the hub's opening, as it lies about its own axis.
    pub hub: ToothOutline,
    /// The angle the hub's outline is turned by to lie on the shaft, in radians.
    pub hub_start: f64,
    /// The numbers the spline comes to.
    pub made: SplineMade,
    /// The master spline as the kernel placed it.
    pub master: Option<MasterPlaced>,
    /// The angle from one tooth to the next of the teeth as counted, in radians: a full turn over
    /// the count of teeth. Not the outline's own pitch angle, which is a full turn once a master
    /// spline is cut in.
    pub tooth_pitch: f64,
    /// The angle of the middle of a tooth that stands, in radians counter-clockwise from the x
    /// axis: tooth 0, or with a master spline the tooth opposite it.
    pub tooth_at: f64,
}

/// One tooth space of a shaft as a region closed above by a circle about the axis: what a cutter
/// takes out between two teeth.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpaceSection {
    /// The outline's own pieces between the tops of the two teeth: from the corner of the next
    /// tooth down its side, along the bottom of the space and up the side of the tooth before, to
    /// its corner.
    pub floor: Vec<OutlinePiece>,
    /// Where `floor` starts: the corner of the tooth after the space.
    pub start: [f64; 2],
    /// Where `floor` ends: the corner of the tooth before the space.
    pub end: [f64; 2],
    /// Whether the region stays clear of the regions of the neighbouring spaces.
    pub apart: bool,
}
