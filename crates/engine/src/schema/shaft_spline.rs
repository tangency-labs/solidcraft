//! Contract and reply of `shaft_spline.insert` (`cmd/shaft_spline.rs`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::features::FeatureResult;
use super::types::{AngleExpr, FeatureCommon, LengthExpr, NumberExpr, Point3, feature_common};

/// The kind of teeth.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SplineKindParam {
    /// Involute teeth, as a rack with a rounded tip cuts them: `module`, `pressure_angle`, `addendum`,
    /// `dedendum`, `root_fillet`, `profile_shift`, `thinning`; the fit `side_clearance`, `tip_clearance`,
    /// `hub_addendum`, `form_clearance`, `hub_root_round`.
    Involute,
    /// Straight-sided teeth with flanks parallel to the tooth's middle: `major_diameter`, `minor_diameter`,
    /// `tooth_width`, `root_round`, `tip_corner`, `tip_size`; the fit `side_clearance`, `major_clearance`,
    /// `minor_clearance`, `hub_root_round`, `hub_tip_round`.
    Straight,
    /// A serration: many small teeth whose flanks meet at `flank_angle`; otherwise as `straight`.
    Serration,
}

/// Which part the command makes.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SplineMakeParam {
    /// The splined shaft only.
    Shaft,
    /// The hub only: a ring of `hub_diameter` with the opening that fits the shaft.
    Hub,
    /// The shaft and its hub, as two bodies.
    Both,
}

/// How the corners at the tips of a straight-sided or serrated shaft are finished.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TipCornerParam {
    /// Left sharp.
    Plain,
    /// Cut at 45 degrees to the flank, starting `tip_size` down the flank from the sharp corner.
    Chamfered,
    /// Rounded with a round of radius `tip_size` that touches the flank and the major circle.
    Rounded,
}

/// The master spline, which lets the hub go on one way only.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MasterSplineParam {
    /// No master spline: the hub goes on in as many positions as there are teeth.
    None,
    /// One tooth is left out of the shaft and the hub keeps one wide tooth in its place.
    ToothOut,
    /// One space of the shaft is left uncut, which makes one wide tooth, and a tooth is left out of the hub.
    WideTooth,
}

/// Cut a spline into a picked shaft, a picked bore, or both, or make a new splined shaft, its hub, or both: the
/// teeth that let a hub sit on a shaft and turn with it. The spline is one feature in the History, cut again on
/// every rebuild, and its numbers stay parameters. With no face picked the shaft stands on the origin along +Z,
/// from z = 0 to `length`, and the hub is the same length around it. Angle 0 (the tooth on +X for a shaft along
/// Z) comes from the world axes. Each number is a number or an expression: lengths in the design's unit, angles
/// in degrees.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ShaftSplineParams {
    /// The kind of teeth: involute, straight or serration.
    #[serde(rename = "type")]
    pub kind: SplineKindParam,
    /// How many teeth, a whole number: 3 to 2000 for involute teeth, 2 to 2000 for the others.
    pub teeth: NumberExpr,
    /// How far the teeth run along the shaft, from 0.001 to 100000 mm.
    pub length: LengthExpr,
    /// A point on the outside of the shaft the tooth spaces are cut into. With a face picked, `make` is not
    /// needed: a shaft face cuts the shaft, a bore face cuts the hub, both cut both.
    #[serde(default)]
    pub shaft_face: Option<Point3>,
    /// A point on the face the shaft's teeth start from: the shaft's end, a shoulder, a chamfer or a round. The
    /// teeth start at the end of the shaft's round face nearest it and run into the face for `length`. Only with
    /// `shaft_face`. Left out, they start at the end nearest `shaft_face`, and that end is stored as the start.
    #[serde(default)]
    pub shaft_start: Option<Point3>,
    /// A point on the bore the hub's tooth spaces are cut into.
    #[serde(default)]
    pub bore_face: Option<Point3>,
    /// A point on the face the hub's teeth start from, as `shaft_start` is for the shaft. Only with `bore_face`.
    /// Left out, they start at the end of the bore nearest `bore_face`.
    #[serde(default)]
    pub bore_start: Option<Point3>,
    /// With no face picked: which part to make (default: shaft).
    #[serde(default)]
    pub make: Option<SplineMakeParam>,
    /// The outside diameter of a new hub: required when a new hub is made, and refused otherwise (also when a
    /// face is picked). It must leave a rim around the hub's spaces at least half as deep as the teeth.
    #[serde(default)]
    pub hub_diameter: Option<LengthExpr>,
    /// Involute: the module, the pitch diameter divided by the count of teeth. Required, but with `shaft_face` it
    /// may be left out: the spline then takes the shaft's own diameter.
    #[serde(default)]
    pub module: Option<LengthExpr>,
    /// Involute: the pressure angle, above 1 and at most 50 degrees (default 30).
    #[serde(default)]
    pub pressure_angle: Option<AngleExpr>,
    /// Involute: the height of the shaft's teeth above the pitch circle, in modules (default 0.5).
    #[serde(default)]
    pub addendum: Option<NumberExpr>,
    /// Involute: the depth of the shaft's spaces below the pitch circle, in modules (default 0.85).
    #[serde(default)]
    pub dedendum: Option<NumberExpr>,
    /// Involute: the radius of the cutter's tip corner, which makes the root fillet, in modules (default 0.4).
    #[serde(default)]
    pub root_fillet: Option<NumberExpr>,
    /// Involute: the profile shift in modules; positive moves tips and roots outward (default 0).
    #[serde(default)]
    pub profile_shift: Option<NumberExpr>,
    /// Involute: the length taken off each tooth's thickness on the pitch circle (default 0).
    #[serde(default)]
    pub thinning: Option<LengthExpr>,
    /// Involute: how far the hub's teeth reach inward from the pitch circle, in modules (default 0.5).
    #[serde(default)]
    pub hub_addendum: Option<NumberExpr>,
    /// Involute: the gap between the shaft's tips and the hub's roots, above 0. Required.
    #[serde(default)]
    pub tip_clearance: Option<LengthExpr>,
    /// Involute: the least gap kept between the hub's tips and the start of the shaft's root fillet (default 0).
    #[serde(default)]
    pub form_clearance: Option<LengthExpr>,
    /// Straight and serration: the diameter over the shaft's tips. Required, but with `shaft_face` it may be left
    /// out: it is then the shaft's own diameter.
    #[serde(default)]
    pub major_diameter: Option<LengthExpr>,
    /// Straight and serration: the diameter at the bottom of the shaft's spaces. Required.
    #[serde(default)]
    pub minor_diameter: Option<LengthExpr>,
    /// Straight: the width of a tooth between its two parallel flanks. Required.
    #[serde(default)]
    pub tooth_width: Option<LengthExpr>,
    /// Serration: the angle between the two flanks of a tooth, above 0 and at most 170 degrees (default 90).
    #[serde(default)]
    pub flank_angle: Option<AngleExpr>,
    /// Straight and serration: the radius where the shaft's flanks meet its minor circle (default 0, sharp).
    #[serde(default)]
    pub root_round: Option<LengthExpr>,
    /// Straight and serration: how the corners at the shaft's tips are finished (default plain).
    #[serde(default)]
    pub tip_corner: Option<TipCornerParam>,
    /// Straight and serration: the chamfer's length down the flank, or the round's radius. Required for
    /// chamfered or rounded corners.
    #[serde(default)]
    pub tip_size: Option<LengthExpr>,
    /// Straight and serration: the gap between the shaft's tips and the bottom of the hub's spaces. Required.
    #[serde(default)]
    pub major_clearance: Option<LengthExpr>,
    /// Straight and serration: the gap between the hub's tips and the shaft's minor circle. Required.
    #[serde(default)]
    pub minor_clearance: Option<LengthExpr>,
    /// Straight and serration: the radius of the rounds at the corners of the hub's teeth (default 0).
    #[serde(default)]
    pub hub_tip_round: Option<LengthExpr>,
    /// The play between the flanks. Involute: both sides together, as an arc on the pitch circle. Straight and
    /// serration: on each side, square to the flanks. Required.
    pub side_clearance: LengthExpr,
    /// The radius of the rounds in the corners of the hub's spaces (default 0).
    #[serde(default)]
    pub hub_root_round: Option<LengthExpr>,
    /// The master spline (default none).
    #[serde(default)]
    pub master_spline: Option<MasterSplineParam>,
    /// With a master spline: where its middle sits, in degrees counter-clockwise from angle 0, from -360 to 360
    /// (default 0). The whole spline, shaft and hub together, is turned by at most half a pitch to put it exactly
    /// there. Refused without a master spline.
    #[serde(default)]
    pub master_angle: Option<AngleExpr>,
    /// The cutter's diameter: each tooth space cut into a picked shaft ends in the curve the cutter leaves.
    /// Without it the spaces end square. Only with `shaft_face`, and more than twice the depth of the teeth.
    #[serde(default)]
    pub cutter_diameter: Option<LengthExpr>,
    /// A chamfer on the ends of the shaft's teeth, running this far along the shaft. Alone it is at 45 degrees
    /// and runs as far in from the major diameter; `end_chamfer_distance2` or `end_chamfer_angle` sets how far
    /// in it runs. On a picked shaft: on the end the teeth start from, and on the far end where the teeth run
    /// the whole face. On a new shaft: on both ends. The spline cuts it itself, so it is made again at the
    /// shaft's size and tooth count whenever they change. Without it the teeth end square. Only when a shaft
    /// is cut or made.
    #[serde(default)]
    pub end_chamfer: Option<LengthExpr>,
    /// A chamfer where the teeth run out of the hub's bore, running this far into the bore. Alone it is at 45
    /// degrees and runs as far out from the tops of the hub's teeth; `entry_chamfer_distance2` or
    /// `entry_chamfer_angle` sets how far out it runs. In a picked bore: at the entry the teeth start from, and
    /// at the far end where the teeth run the whole bore. In a new hub: at both ends. Without it the hub's
    /// teeth end square. Only when a hub is cut or made.
    #[serde(default)]
    pub entry_chamfer: Option<LengthExpr>,
    /// The End chamfer as two distances: how far in from the major diameter it runs, more than 0 and at most
    /// 100000 mm (`end_chamfer` is how far along the shaft). Only with `end_chamfer`, and not with
    /// `end_chamfer_angle`.
    #[serde(default)]
    pub end_chamfer_distance2: Option<LengthExpr>,
    /// The End chamfer as a distance and an angle: its slope to the shaft's surface, above 0 and below 90
    /// degrees, so it runs `end_chamfer` times the angle's tangent in from the major diameter. Only with
    /// `end_chamfer`, and not with `end_chamfer_distance2`.
    #[serde(default)]
    pub end_chamfer_angle: Option<AngleExpr>,
    /// The Entry chamfer as two distances: how far out from the tops of the hub's teeth it runs, more than 0
    /// and at most 100000 mm (`entry_chamfer` is how far into the bore). Only with `entry_chamfer`, and not
    /// with `entry_chamfer_angle`.
    #[serde(default)]
    pub entry_chamfer_distance2: Option<LengthExpr>,
    /// The Entry chamfer as a distance and an angle: its slope to the bore's surface, above 0 and below 90
    /// degrees, so it runs `entry_chamfer` times the angle's tangent out from the tops of the hub's teeth.
    /// Only with `entry_chamfer`, and not with `entry_chamfer_distance2`.
    #[serde(default)]
    pub entry_chamfer_angle: Option<AngleExpr>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Where the master spline is, as made.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MasterSplineMade {
    /// `tooth_out` or `wide_tooth`.
    pub form: MasterSplineParam,
    /// The angle asked for, in degrees.
    pub angle: f64,
    /// The tooth left out, or for a wide tooth the tooth before the space that is filled, counted
    /// counter-clockwise from the tooth at angle 0 before the turn.
    pub tooth: u32,
    /// How far the whole spline was turned to put the master spline at its angle, in degrees.
    pub turn: f64,
}

/// The kernel's numbers for a Shaft Spline feature, as its reply carries them. Diameters and clearances in
/// mm, angles in degrees.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ShaftSplineNumbers {
    /// The kind of teeth.
    #[serde(rename = "type")]
    pub kind: SplineKindParam,
    /// The count of teeth.
    pub teeth: u32,
    /// The diameter over the shaft's tips.
    pub shaft_major_diameter: f64,
    /// The diameter at the bottom of the shaft's spaces.
    pub shaft_minor_diameter: f64,
    /// The diameter at the bottom of the hub's spaces.
    pub hub_major_diameter: f64,
    /// The diameter over the tips of the hub's teeth.
    pub hub_minor_diameter: f64,
    /// Involute: the pitch diameter, module times teeth.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pitch_diameter: Option<f64>,
    /// Involute: the base diameter, from which the flanks unwind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_diameter: Option<f64>,
    /// Involute: the diameter at which the shaft's involute flank starts above its root fillet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub form_diameter: Option<f64>,
    /// Involute: whether the cutter undercuts the shaft's flanks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub undercut: Option<bool>,
    /// Serration: the diameter at which a tooth's flanks, carried on, would meet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sharp_tip_diameter: Option<f64>,
    /// The gap between the flanks on each side with the hub centred.
    pub side_clearance_each: f64,
    /// The gap between the shaft's tips and the hub's roots.
    pub tip_gap: f64,
    /// The gap between the hub's tips and the shaft's roots.
    pub root_gap: f64,
    /// Involute: the gap between the hub's tips and the start of the shaft's root fillet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub form_gap: Option<f64>,
    /// How far the hub turns on the shaft either way from centred before a flank touches.
    pub turning_play: f64,
    /// In how many positions the hub goes on the shaft: the count of teeth, or 1 with a master spline.
    pub positions: u32,
    /// The master spline, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub master_spline: Option<MasterSplineMade>,
    /// Involute: how far the outlines given to the kernel for the flanks and fillets may be from the true curves,
    /// in mm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flank_tolerance: Option<f64>,
}

/// What `shaft_spline.insert` returns: a feature command's reply (the one Shaft Spline feature and the bodies),
/// with the kernel's numbers.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ShaftSplineAdded {
    #[serde(flatten)]
    pub feature: FeatureResult,
    #[serde(flatten)]
    pub numbers: ShaftSplineNumbers,
}

#[cfg(test)]
#[path = "shaft_spline_tests.rs"]
mod tests;
