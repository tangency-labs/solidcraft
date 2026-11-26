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
use super::types::{AngleExpr, FeatureCommon, LengthExpr, Point3, feature_common};

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
    /// What is left of the two surfaces. `both` (the default): each is cut back to the round and
    /// sewn to it, one body. `first`: only `a` is cut back and sewn to the round; `b` is left as
    /// it was. `none`: neither changes, and the round alone is a new surface body.
    #[serde(default)]
    pub trim: Option<FilletTrimParam>,
    /// Extend a surface that stops short of the other to meet it. Default: false.
    #[serde(default)]
    pub extend: Option<bool>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// What Surface Fillet leaves of the two surfaces, as the handler spells it (any case).
#[derive(Clone, Copy, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FilletTrimParam {
    Both,
    First,
    None,
}

impl<'de> Deserialize<'de> for FilletTrimParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) => match s.to_ascii_lowercase().as_str() {
                "both" => Ok(FilletTrimParam::Both),
                "first" => Ok(FilletTrimParam::First),
                "none" => Ok(FilletTrimParam::None),
                _ => Err(D::Error::custom(whole("`trim` must be both, first or none"))),
            },
            _ => Err(D::Error::custom(whole("`trim` must be both, first or none"))),
        }
    }
}

/// Bevel the seam between two surface bodies (`surface.chamfer`, #769): each is cut where the
/// other meets it, keeping the side its point is on; the kept pieces are sewn into `a`'s body
/// (`b`'s is then gone, with `trim` both) and the seam is bevelled `distance` back along each
/// surface, or `distance` along the first and `distance2` along the second, or `distance` along
/// the first at `angle` to it. Each body is one face. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SurfaceChamferParams {
    /// The first face.
    pub a: FilletFace,
    /// The second face.
    pub b: FilletFace,
    /// How far back along the first surface the bevel reaches (along both, with neither
    /// `distance2` nor `angle`).
    pub distance: LengthExpr,
    /// How far back along the second surface the bevel reaches. Not with `angle`.
    #[serde(default)]
    pub distance2: Option<LengthExpr>,
    /// The angle the bevel makes with the first surface. Not with `distance2`.
    #[serde(default)]
    pub angle: Option<AngleExpr>,
    /// Swap which surface is the first (by default the one facing up most). Default: false.
    #[serde(default)]
    pub flip: Option<bool>,
    /// What is left of the two surfaces, as for `surface.fillet`. Default: both.
    #[serde(default)]
    pub trim: Option<FilletTrimParam>,
    /// Extend a surface that stops short of the other to meet it. Default: false.
    #[serde(default)]
    pub extend: Option<bool>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}
