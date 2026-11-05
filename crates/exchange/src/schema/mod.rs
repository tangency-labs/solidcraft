//! The snapshot: plain, versioned data (docs/formats/exchange-snapshot.md).
//!
//! Every value has a type saying what it is: names and indices ([`ids`]), lengths, angles,
//! points and directions ([`geometry`]), kinds as enums. Compatibility rules, for writers and
//! readers alike:
//! - Fields are only ever added, never renamed, retyped or removed, and a field added later has a
//!   default (empty, zero or `None`), so a reader built against an older or newer version reads
//!   the data with the fields it knows. Every struct reads missing fields as their defaults and
//!   ignores fields it does not know.
//! - A kind a reader does not know (a surface, curve, feature, property, parameter source) reads
//!   as that enum's `Other`.
//! - [`SCHEMA`] goes up only for a change that breaks these rules (a field that means something
//!   else); a reader refuses a schema above the one it was built for.

mod body;
mod edit;
mod feature;
mod geometry;
mod ids;
mod material;
mod parameter;
mod validate;

use serde::{Deserialize, Serialize};

pub use body::{Body, Bounds, ControlPoint, Curve, Edge, Face, Knots, Mass, Surface, Vertex};
pub use edit::{AppliedEdit, Edit};
pub use feature::{Feature, FeatureKind, Input, Operation, Profile, Sketch, SketchCurve};
pub use geometry::{Direction, Frame, LengthUnit, Mm, Plane, Point, Point2, Quantity, Radians, Transform};
pub use ids::{BodyName, EdgeIndex, EdgeName, FaceIndex, FaceName, FeatureId, MaterialName, MeshVertex, ParameterName, VertexIndex, VertexName};
pub use material::{Conditions, Elasticity, Material, MaterialAxes, MaterialProperty, Property};
pub use parameter::{Parameter, ParameterSource, ParameterValue};

/// The snapshot schema this crate writes and reads.
pub const SCHEMA: u32 = 1;

/// A design evaluated at a set of parameter values, packaged for analysis.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct Snapshot {
    /// [`SCHEMA`] when written by this crate.
    pub schema: u32,
    pub unit: LengthUnit,
    /// The design's name.
    pub design: String,
    /// Every parameter at the values the snapshot was taken with, in the design's order.
    pub parameters: Vec<Parameter>,
    /// The physical materials the bodies use.
    pub materials: Vec<Material>,
    /// The bodies, placed in world coordinates, in the design's order.
    pub bodies: Vec<Body>,
    /// The timeline's modelling features, in order, suppressed ones included.
    pub features: Vec<Feature>,
    /// The edits the snapshot was taken with, in the order given.
    pub edits: Vec<AppliedEdit>,
    /// Timeline features that failed, when the snapshot was taken with failures allowed
    /// ([`crate::Options::with_failures_allowed`]); the bodies are what the rest built.
    pub failures: Vec<Failure>,
}

/// A timeline feature that failed to build.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct Failure {
    pub feature: FeatureId,
    pub name: String,
    pub message: String,
}

impl Snapshot {
    /// Checks that every index refers to something there, every direction is a unit vector,
    /// every coordinate is finite and every NURBS patch is well formed. [`crate::from_json`]
    /// refuses a snapshot that fails it, and snapshots this crate takes pass it, so a
    /// `Snapshot` in hand can be walked without bounds surprises.
    pub fn validate(&self) -> crate::Result<()> {
        validate::snapshot(self).map_err(crate::Error::Invalid)
    }

    /// The body with this name.
    pub fn body(&self, name: &str) -> Option<&Body> {
        self.bodies.iter().find(|b| b.name == *name)
    }

    /// The feature with this id.
    pub fn feature(&self, id: FeatureId) -> Option<&Feature> {
        self.features.iter().find(|f| f.id == id)
    }
}
