//! Contract and result types for `solid.arrange` in `cmd/arrange.rs` (#12430).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::types::{AngleExpr, FaceRef, LengthExpr, NumberExpr, PlaneParam, Point2, StringList};

/// Largest side of the rectangle, and the largest coordinate of its corner, in mm.
pub const MAX_SIZE: f64 = 1e6;

/// Lay solid bodies out side by side on a sheet, as on a build plate or a sheet to cut from: a
/// rectangle on the XY plane or a given plane, or a flat face at any tilt. Each body keeps its
/// height above the sheet and only turns about the sheet's normal, unless it is laid on a
/// flat side first (`bottoms`, `lay_flat`); its footprint is the
/// outline it covers seen from above the sheet, from its exact edges and silhouettes. Each body that fits
/// gets a Move feature; the whole arrangement is one undo step. Bodies that don't fit stay where
/// they are and are listed in `unplaced`. A hole through a body is not used for other bodies.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ArrangeParams {
    /// The bodies to arrange, by name. Solid bodies only: a mesh body can't be arranged yet.
    pub bodies: StringList,
    /// The rectangle's size along X: above 0, at most 1 000 000 mm. Needed unless `sheet` is given.
    #[serde(default)]
    pub width: Option<LengthExpr>,
    /// The rectangle's size along Y: above 0, at most 1 000 000 mm. Needed unless `sheet` is given.
    #[serde(default)]
    pub length: Option<LengthExpr>,
    /// A flat face, at any tilt, of a body that is not arranged, to lay the bodies out on in place
    /// of the rectangle: its outline and holes, in its plane.
    #[serde(default)]
    pub sheet: Option<FaceRef>,
    /// How many of each body to lay out: the body and copies of it, 1 to 1 000. Default: 1.
    #[serde(default)]
    pub quantity: Option<NumberExpr>,
    /// How many sheets to fill, side by side along +X, each taking what the ones before could
    /// not: 1 to 100. Default: 1.
    #[serde(default)]
    pub sheets: Option<NumberExpr>,
    /// The gap between one sheet and the next: 0 to 100 000 mm. Default: 10 mm.
    #[serde(default)]
    pub sheet_spacing: Option<LengthExpr>,
    /// The plane the rectangle lies on, along its X and Y from its origin, the bodies on its
    /// normal side. Default: XY.
    #[serde(default)]
    pub plane: Option<PlaneParam>,
    /// The rectangle's lower-left corner on its plane, [x, y] in mm. Default: [0, 0].
    #[serde(default)]
    pub origin: Option<Point2>,
    /// Least distance between two bodies' footprints: 0 to 100 000 mm. Default: 1 mm.
    #[serde(default)]
    pub spacing: Option<LengthExpr>,
    /// Least distance between a footprint and the rectangle's edge: 0 to 100 000 mm. Default: 0.
    #[serde(default)]
    pub margin: Option<LengthExpr>,
    /// The step of the turns about Z a body may be given: 0 to 360 degrees, where 0 keeps each
    /// body's turn; other steps under 1 degree are refused. Default: 90 degrees.
    #[serde(default)]
    pub angle_step: Option<AngleExpr>,
    /// How footprints fit together. Default: `true_shape`.
    #[serde(default)]
    pub method: Option<ArrangeMethodParam>,
    /// How many other orders of the bodies to try after the first layout, keeping the best:
    /// a whole number from 0 to 10 000. Default: 0.
    #[serde(default)]
    pub improve: Option<f64>,
    /// Lay each body on the flat side it rests on with the most of it touching, of the sides
    /// nothing of it hangs below, before it is placed, standing on the sheet. A body with no such
    /// side stays as it is. Default: true.
    #[serde(default)]
    pub lay_flat: Option<bool>,
    /// Leave the bodies where they are and lay out copies of them instead. Default: false (the
    /// bodies themselves are moved; copies are made only for a `quantity` above 1).
    #[serde(default)]
    pub copy: Option<bool>,
    /// Keep clear of the other bodies on the sheet, of the originals left by `copy`, and of a
    /// body that does not fit and stays where it is. Default: true; false lays out as if the
    /// sheet were empty.
    #[serde(default)]
    pub avoid: Option<bool>,
    /// Flat faces to lay bodies on, at most one on each body: that body is laid on the plane of
    /// the face, standing on the sheet, whatever `lay_flat` says.
    #[serde(default)]
    pub bottoms: Option<Vec<FaceRef>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArrangeMethodParam {
    /// Each body's own outline: bodies tuck into each other's corners.
    TrueShape,
    /// Each body's bounding rectangle at its turn: neat rows.
    Rectangular,
}

/// Where one body went.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ArrangedBody {
    pub body: String,
    /// When `body` is a copy made by the arrangement: the body it is a copy of.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_of: Option<String>,
    /// Which sheet it is on, from 0.
    pub sheet: usize,
    /// How far the body moved along X after turning, in mm.
    pub x: f64,
    /// How far the body moved along Y after turning, in mm.
    pub y: f64,
    /// How far the body turned about the world Z axis through the origin, counter-clockwise, in
    /// degrees (after it was laid down, when it was).
    pub angle_deg: f64,
    /// When the body was laid on a flat side: that side's outward normal before the move. The
    /// body was first turned about the origin to point it down, then by `angle_deg`, moved by
    /// `x` and `y`, and raised to stand on the sheet; the move feature holds it as one turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom: Option<[f64; 3]>,
}

/// One sheet seen from above.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SheetOutline {
    pub outer: Vec<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<Vec<[f64; 2]>>,
}

/// What `solid.arrange` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ArrangeReply {
    /// The bodies that fit, in the order they were laid.
    pub placed: Vec<ArrangedBody>,
    /// The bodies that did not fit; they were not moved.
    pub unplaced: Vec<String>,
    /// How many copies asked for by `quantity` did not fit, and were not made.
    pub copies_left: usize,
    /// How many sheets have bodies on them.
    pub sheets_used: usize,
    /// The placed footprints' area over the area of the sheets used, 0 to 1.
    pub fill: f64,
    /// How far a footprint may be from the true outline, in mm; the spacing and margin were
    /// widened by it.
    pub footprint_error_mm: f64,
    /// Each sheet asked for, seen from above: its outer ring and its holes as [x, y] points, mm.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sheet_outlines: Vec<SheetOutline>,
    /// The height the sheets lie at, mm.
    #[serde(default)]
    pub sheet_z: f64,
    /// A tilted sheet's frame: along it, across it and out of it (unit, world). The outlines,
    /// `x`, `y` and `sheet_z` are in it: a point is `x e1 + y e2 + sheet_z n`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet_frame: Option<[[f64; 3]; 3]>,
    /// The margin kept from each sheet's edges, mm.
    #[serde(default)]
    pub margin_mm: f64,
    /// The bodies whose outline from above could not be found: they were placed by their box.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub boxed: Vec<String>,
    /// The browser groups the copies were put in (one per component they are in), by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub copy_groups: Vec<String>,
    /// The layout ran long and hurried to finish within its time budget (about 30 s, 6 s on the web): some parts
    /// may be packed by their box, less tightly, or left unplaced, and the same call may lay out
    /// differently next time.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hurried: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "arrange_tests.rs"]
mod tests;
