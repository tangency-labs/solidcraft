//! Contract for Surface Fillet (`surface.fillet`) in `cmd/surface.rs` (#769). It replies with the
//! plain feature reply, [`super::features::FeatureResult`].
//!
//! Whether the bodies exist and are surfaces, the points lie on their faces, the surfaces meet and
//! the radius evaluates are the handler's to check.

use schemars::JsonSchema;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use super::check::whole;
use super::types::{FeatureCommon, LengthExpr, Point3, feature_common};

/// One of the two faces to round between: a surface body and a point on its (one) face.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct FilletFace {
    /// The surface body, by name.
    #[serde(deserialize_with = "body")]
    pub body: String,
    /// A point [x, y, z] in mm on the face. It also picks the side of the other surface that is
    /// kept: the side the rolling ball stays on.
    pub point: Point3,
}

/// The handler's message for `body` when it isn't text.
fn body<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(s),
        _ => Err(D::Error::custom(whole("`body` is required"))),
    }
}

/// Round the seam between two surface bodies: each is cut where the other meets it, keeping the
/// side its point is on; the kept pieces are sewn into `a`'s body (`b`'s is then gone) and the
/// seam is rounded with a fillet of `radius`. Each body is one face. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SurfaceFilletParams {
    /// The first face.
    pub a: FilletFace,
    /// The second face.
    pub b: FilletFace,
    /// The radius of the rolling ball.
    pub radius: LengthExpr,
    /// Extend a surface that stops short of the other to meet it. Default: false.
    #[serde(default)]
    pub extend: Option<bool>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}
