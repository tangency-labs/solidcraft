//! Edits: changes made to a copy of the design before it is evaluated.

use serde::{Deserialize, Serialize};

use super::geometry::Plane;
use super::ids::{BodyName, FaceName, FeatureId};

/// A change made to the design before it is evaluated, on a copy: the design itself is left as
/// it is. Edits apply in order, after the parameter overrides, as timeline features at the end
/// of the timeline, so faces they make get persistent names that name them
/// ([`AppliedEdit::features`]).
///
/// JSON: `{"suppress": 4}`, `{"partition": {"name", "plane": {"point", "normal"}, "bodies"?}}`,
/// `{"split_face": {"name", "body", "face", "plane"}}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[non_exhaustive]
pub enum Edit {
    /// Rebuild without the feature (a hole filled, a fillet back to its sharp edge, a boss
    /// gone). Suppressing a feature that makes a body takes that body away; one a later feature
    /// needs fails that feature.
    Suppress(FeatureId),
    /// Cut bodies by a plane: each body it crosses becomes two (only `bodies`, when not empty).
    /// The cut faces are named after the split.
    Partition {
        name: String,
        plane: Plane,
        #[serde(default)]
        bodies: Vec<BodyName>,
    },
    /// Split a face of a body where a plane meets it; the solid is unchanged, the pieces are
    /// named after the face.
    SplitFace { name: String, body: BodyName, face: FaceName, plane: Plane },
}

/// An edit a snapshot was taken with and what it became.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AppliedEdit {
    pub edit: Edit,
    /// The features it is: the one suppressed, or those it added (a partition adds one split per
    /// body it cuts). Faces it made name these.
    pub features: Vec<FeatureId>,
    /// The bodies it cut or split faces of.
    pub bodies: Vec<BodyName>,
}
