//! Render shell requests. The UI applies validated echoes without design edits.
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::borrow::Cow;

use super::types::{CameraParam, present};

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderDock {
    pub panel: RenderPanel,
    pub side: DockSide,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum RenderPanel {
    Inspector,
    Browser,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DockSide {
    Left,
    Right,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderFocus {
    /// Hide the Render docks while retaining their layout and the current camera.
    pub on: bool,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderBrowser {
    /// Open the floating Browser card in a narrow Render window.
    pub open: bool,
}

/// Empty input for the read-only viewport effect query.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StatusItemsParams {}

/// Show or hide the View angles readout under the orientation gizmo (#6032): the view's pitch, yaw
/// and roll in degrees (the angles of `view.apply`'s camera, which takes them in radians) and Ortho
/// or Persp for the projection. A display setting the app keeps: not saved with the design, not an
/// undo step, and it never opens a dialog.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AngleReadoutParams {
    /// True shows the readout, false hides it.
    #[serde(deserialize_with = "enabled_flag")]
    pub enabled: bool,
}

/// A boolean, with the `view.angle_readout` handler's message for anything else.
fn enabled_flag<'de, D: serde::Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    match Value::deserialize(d)? {
        Value::Bool(b) => Ok(b),
        _ => Err(serde::de::Error::custom("must be a boolean")),
    }
}

/// What `view.angle_readout` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AngleReadoutResult {
    /// Whether the readout is shown now.
    pub enabled: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Turn the view to the command's standard orientation (front, back, left, right, top, bottom
/// or isometric), as the view cube does, keeping the camera's target, distance, projection and
/// up axis. Display only: the design isn't changed and nothing is added to undo.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct StandardViewParams {
    /// The camera to turn. Default: the session's camera, or the home view when there is none.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "CameraParam")]
    pub camera: Option<CameraParam>,
}

/// Look straight at the first selected planar face or plane, centred on the point it was picked
/// at (a plane: its origin); asked again while already facing it, look from the other side.
/// Keeps the camera's distance, projection and up axis. Needs a planar face or plane in the
/// selection. Display only: the design isn't changed and nothing is added to undo.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct NormalToParams {
    /// The camera to turn. Default: the session's camera, or the home view when there is none.
    /// Its yaw and roll must also be within ±1e6 and its pitch within ±π/2.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "CameraParam")]
    pub camera: Option<CameraParam>,
}

/// What the standard views and `view.normal_to` return: the camera after the turn, which the
/// session keeps as its view.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CameraReply {
    /// The camera now.
    #[schemars(with = "CameraParam")]
    pub camera: solidcraft_render::Camera,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// `view.next_tab` and `view.previous_tab` take no parameters (#8565).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TabStepParams {}

/// What `view.next_tab` and `view.previous_tab` return. The window shows the design tab `step`
/// places along the strip, going round at either end; the design is not changed and nothing is
/// added to undo.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TabStep {
    /// 1: the tab to the right; -1: the tab to the left.
    pub step: i8,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Validated request and echoed result of `view.render_select`.
#[derive(Debug, Serialize)]
pub struct RenderSelect {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

impl<'de> Deserialize<'de> for RenderSelect {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Request {
            kind: String,
            id: Option<String>,
        }
        let request = Request::deserialize(d)?;
        let identified = matches!(request.kind.as_str(), "light" | "shot");
        if !matches!(
            request.kind.as_str(),
            "nothing" | "environment" | "lighting" | "light" | "shot" | "output" | "wear" | "sampling" | "effects" | "check"
        ) || identified != request.id.is_some()
            || request.id.as_ref().is_some_and(|id| id.trim().is_empty() || id.len() > 1024)
        {
            return Err(serde::de::Error::custom("expected a Browser kind and an id only for light or shot"));
        }
        Ok(Self { kind: request.kind, id: request.id })
    }
}

impl JsonSchema for RenderSelect {
    fn schema_name() -> Cow<'static, str> {
        "RenderSelect".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "oneOf": [
                {
                    "type": "object",
                    "properties": {
                        "kind": {"type": "string", "enum": ["nothing", "environment", "lighting", "output", "wear", "sampling", "effects", "check"]},
                        "id": {"type": "null"}
                    },
                    "required": ["kind"],
                    "additionalProperties": false
                },
                {
                    "type": "object",
                    "properties": {
                        "kind": {"type": "string", "enum": ["light", "shot"]},
                        "id": {"type": "string", "minLength": 1, "maxLength": 1024, "pattern": super::types::not_blank_pattern(),
                               "description": "A nonblank identity, at most 1024 UTF-8 bytes."}
                    },
                    "required": ["kind", "id"],
                    "additionalProperties": false
                }
            ]
        })
    }
}

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

#[cfg(test)]
#[path = "view_navigation_tests.rs"]
mod tests;
