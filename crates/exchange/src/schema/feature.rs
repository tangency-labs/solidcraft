//! The timeline: how each body was built.

use serde::{Deserialize, Serialize};

use super::geometry::{Direction, Frame, Mm, Point, Point2, Quantity, Radians, Transform};
use super::ids::{BodyName, FeatureId};

/// One modelling feature of the timeline (sketches and construction geometry are not listed).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct Feature {
    pub id: FeatureId,
    /// The name the user sees (`Extrude1`).
    pub name: String,
    /// SolidCraft's name for the kind (`extrude`, `thread`, …), also for kinds [`FeatureKind`]
    /// has no variant for.
    pub kind_name: String,
    pub suppressed: bool,
    pub kind: FeatureKind,
    /// Every numeric input, evaluated, with its label as the user sees it.
    pub inputs: Vec<Input>,
    /// The names it gives the bodies it makes.
    pub creates: Vec<BodyName>,
    /// The snapshot's bodies it made faces of, or names.
    pub bodies: Vec<BodyName>,
}

/// A numeric input of a feature.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Input {
    pub label: String,
    pub value: Quantity,
}

/// What a join, cut or intersect does to the bodies it meets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Operation {
    #[default]
    NewBody,
    Join,
    Cut,
    Intersect,
}

/// The kinds of feature analysis tools reason about, with their key dimensions; any other kind
/// is [`FeatureKind::Other`] (its inputs are still in [`Feature::inputs`]).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum FeatureKind {
    /// A sketch's profiles pushed along `direction` (the sketch normal, flipped for a negative
    /// extrude) by `distance`, both ways when `two_sided`.
    Extrude {
        operation: Operation,
        sketch: Option<Sketch>,
        direction: Option<Direction>,
        distance: Option<Mm>,
        two_sided: bool,
        targets: Vec<BodyName>,
    },
    Revolve {
        operation: Operation,
        sketch: Option<Sketch>,
        angle: Option<Radians>,
        targets: Vec<BodyName>,
    },
    Sweep {
        operation: Operation,
        sketch: Option<Sketch>,
        targets: Vec<BodyName>,
    },
    Loft {
        operation: Operation,
        targets: Vec<BodyName>,
    },
    /// A box from `corner`, its length, width and height along x, y and z.
    Box {
        operation: Operation,
        corner: Option<Point>,
        size: Option<[Mm; 3]>,
    },
    Cylinder {
        operation: Operation,
        base: Option<Point>,
        axis: Option<Direction>,
        radius: Option<Mm>,
        height: Option<Mm>,
    },
    Sphere {
        operation: Operation,
        center: Option<Point>,
        radius: Option<Mm>,
    },
    Torus {
        operation: Operation,
        center: Option<Point>,
    },
    /// A hole drilled at `position` along `direction`.
    Hole {
        position: Point,
        direction: Option<Direction>,
        diameter: Option<Mm>,
        depth: Option<Mm>,
    },
    Combine {
        operation: Operation,
        target: BodyName,
        tools: Vec<BodyName>,
    },
    Fillet {
        bodies: Vec<BodyName>,
        radius: Option<Mm>,
    },
    Chamfer {
        bodies: Vec<BodyName>,
        distance: Option<Mm>,
    },
    Shell {
        bodies: Vec<BodyName>,
        thickness: Option<Mm>,
    },
    Split {
        body: BodyName,
    },
    /// Copies of `bodies`, one per transform (the original's identity left out).
    Pattern {
        bodies: Vec<BodyName>,
        transforms: Vec<Transform>,
    },
    Mirror {
        bodies: Vec<BodyName>,
        transform: Option<Transform>,
    },
    /// Any other kind ([`Feature::kind_name`] names it), or one this version does not know.
    #[default]
    #[serde(other)]
    Other,
}

/// A sketch and the closed profiles a feature uses from it, in the sketch's plane.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct Sketch {
    pub id: FeatureId,
    pub name: String,
    pub frame: Frame,
    pub profiles: Vec<Profile>,
}

/// A closed profile: its outer loop and holes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct Profile {
    /// Its place in the solved sketch's profile list.
    pub index: u32,
    /// Area (mm²).
    pub area: f64,
    pub outer: Vec<SketchCurve>,
    pub holes: Vec<Vec<SketchCurve>>,
}

/// A curve of a profile loop. A loop of arcs about one centre that closes is one circle.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum SketchCurve {
    Line {
        a: Point2,
        b: Point2,
    },
    /// From angle `start` through `sweep` (counter-clockwise when positive).
    Arc {
        center: Point2,
        radius: Mm,
        start: Radians,
        sweep: Radians,
    },
    Circle {
        center: Point2,
        radius: Mm,
    },
    /// A spline, conic or other curve, or a kind this version does not know.
    #[default]
    #[serde(other)]
    Other,
}
