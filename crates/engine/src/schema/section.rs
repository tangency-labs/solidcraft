//! Section clipping contract. Explicit planes use numerical millimetre coordinates.
use super::{AxisDir, LengthExpr, Point3, StringList, present};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Plane {
    Name(String),
    Id(u64),
    Explicit { origin: Option<Point3>, normal: AxisDir },
}
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SectionParams {
    /// Required unless clearing. XY/XZ/YZ, construction name/id, or {origin?,normal}.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Plane")]
    pub plane: Option<Plane>,
    /// Signed distance along the original plane normal, in design units or an expression.
    pub offset: Option<LengthExpr>,
    /// World coordinate on a named plane's normal axis; mutually exclusive with offset.
    pub at: Option<LengthExpr>,
    pub flip: Option<bool>,
    pub hatch: Option<bool>,
    pub clear: Option<bool>,
}
#[derive(Serialize, JsonSchema)]
pub struct Section {
    pub origin: [f64; 3],
    pub normal: [f64; 3],
    pub hatch: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub axis: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<f64>,
}
#[derive(Serialize, JsonSchema)]
pub struct SectionResult {
    pub section: Option<Section>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// The cut of bodies by a plane as 2D lines, arcs and splines in the plane's frame. Changes
/// nothing in the design.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SectionCurvesParams {
    /// XY/XZ/YZ, a construction plane's name or id, or {origin?, normal}, as for Section Analysis.
    pub plane: Plane,
    /// Signed distance along the plane's normal, in design units or an expression.
    pub offset: Option<LengthExpr>,
    /// World coordinate on a named plane's normal axis; instead of offset.
    pub at: Option<LengthExpr>,
    /// Turn the plane over (its normal and its y axis). Default: false.
    pub flip: Option<bool>,
    /// The bodies to cut: a name or a list of 1 to 10 000 names. Default: every visible body.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "StringList")]
    pub bodies: Option<StringList>,
    /// Mesh chord tolerance in mm, 0.0001 to 1. Default: 0.01.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub tolerance: Option<f64>,
}

/// The cut plane's frame, in world coordinates (mm): the loops' points are [x, y] along `x` and
/// `y` from `origin`.
#[derive(Serialize, JsonSchema)]
pub struct SectionFrame {
    pub origin: [f64; 3],
    pub x: [f64; 3],
    pub y: [f64; 3],
    pub normal: [f64; 3],
}

/// One curve of a section loop, points [x, y] in the frame.
#[derive(Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SectionCurveSegment {
    /// A straight piece (exact).
    Line { start: [f64; 2], end: [f64; 2] },
    /// A circular arc (exact); `sweep_deg` counter-clockwise positive.
    Arc { center: [f64; 2], radius: f64, start: [f64; 2], end: [f64; 2], sweep_deg: f64 },
    /// A fit spline through points on the surface, with the same curve as cubic Bézier pieces
    /// [p0, p1, p2, p3]; `closed` when it is periodic (its last point repeats its first).
    Spline { closed: bool, points: Vec<[f64; 2]>, beziers: Vec<[[f64; 2]; 4]> },
}

/// One loop of one body's cut: outer loops run counter-clockwise, holes clockwise.
#[derive(Serialize, JsonSchema)]
pub struct SectionCurveLoop {
    pub body: String,
    pub closed: bool,
    /// Enclosed area in mm², for closed loops.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub area: Option<f64>,
    pub segments: Vec<SectionCurveSegment>,
}

/// What `inspect.section_curves` returns.
#[derive(Serialize, JsonSchema)]
pub struct SectionCurvesResult {
    pub frame: SectionFrame,
    /// Always "mm".
    pub units: String,
    pub loops: Vec<SectionCurveLoop>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "section_tests.rs"]
mod tests;
