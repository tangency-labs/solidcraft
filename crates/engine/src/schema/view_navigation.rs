//! Transient Render framing requests. Neither input nor reply belongs to a design.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
pub enum FrameAspect {
    #[serde(rename = "16:9")]
    Wide,
    #[serde(rename = "3:2")]
    Photo,
    #[serde(rename = "1:1")]
    Square,
    #[serde(rename = "4:5")]
    Portrait,
    #[serde(rename = "9:16")]
    Tall,
    #[serde(rename = "custom")]
    Custom,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderFrame {
    /// Overlay aspect selection only; output dimensions require render.settings.set.
    #[serde(default, deserialize_with = "super::types::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FrameAspect")]
    pub aspect: Option<FrameAspect>,
    #[serde(default, deserialize_with = "super::types::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub thirds: Option<bool>,
    #[serde(default, deserialize_with = "super::types::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub safe_area: Option<bool>,
    #[serde(default, deserialize_with = "super::types::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub centre_mark: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderSelect {
    #[schemars(with = "RenderKind")]
    pub kind: String,
    /// Required only for a light or a shot; bounded to 1024 bytes by the command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RenderKind {
    Nothing,
    Environment,
    Lighting,
    Light,
    Shot,
    Output,
    Sampling,
    Effects,
    Check,
    Wear,
}
