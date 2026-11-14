//! Render shell requests. The UI applies validated echoes without design edits.
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use std::borrow::Cow;

use super::check::whole;
use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::Vec3Geom;

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

/// Pause or resume the current document's live Render preview without saving a setup.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderLive {
    /// True pauses refinement; false resumes it using the current visual settings.
    pub paused: bool,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderBrowser {
    /// Open the floating Browser card in a narrow Render window.
    pub open: bool,
}

/// Show or hide one of the Render window's two panels: the Inspector (`view.render_inspector`) or
/// the Shots strip (`view.shots_strip`). Both read the one flag the same way, so one contract
/// describes both calls. Unlike the viewport toggles below, neither command has a toggle form: the
/// flag is required, and the schema says so.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderPanelVisibilityParams {
    /// True shows the panel, false hides it.
    pub visible: bool,
}

/// What the two Render window panels answer: the flag they were given. The window applies it; it is
/// per-document UI state and never saved with the design.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RenderPanelVisibilityResult {
    pub visible: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Latch or release the viewport's Zoom tool. With no flag, the UI toggles it.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ZoomTool {
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub on: Option<bool>,
}

/// Show or hide a viewport display setting: the Grid, the Ground Shadow or Continuous Orbit. The
/// three commands read the one key the same way (`cmd/view_navigation.rs`'s `view_toggle`), so one
/// contract describes all three. Omitting the key asks the UI to toggle the setting; `null` is not
/// omitting it, which is what the handlers have always refused.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DisplayToggleParams {
    /// True shows it or starts it, false hides it or stops it; omitted toggles it.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub on: Option<bool>,
}

/// What the viewport display toggles answer: the flag they were given, and nothing when the call
/// asked for a toggle. The UI applies the echo; the setting is the app's, never saved in the design.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DisplayToggleResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on: Option<bool>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Show or hide the navigation bar. A window setting the app keeps, not saved with the design; the
/// same optional flag as the other viewport toggles, under the key this command uses.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NavigationBarParams {
    /// True shows the bar, false hides it; omitted toggles it.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub visible: Option<bool>,
}

/// What `view.navigation_bar` answers: the flag it was given, and nothing when the call asked for
/// a toggle.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NavigationBarResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
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

/// A required display-setting boolean, preserving the handlers' messages for anything else.
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

/// Show long origin axes across the viewport, or keep the short axes. An application
/// preference only: not saved with the design, not an undo step, and never opens a dialog.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct OriginAxesAcrossViewParams {
    /// True extends the axes across the view, false keeps the short axes.
    #[serde(deserialize_with = "enabled_flag")]
    pub enabled: bool,
}

/// The accepted origin-axis display setting, applied by the app.
#[derive(Debug, Serialize, JsonSchema)]
pub struct OriginAxesAcrossViewResult {
    pub enabled: bool,
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
    pub camera: solidcraft_viewport::Camera,
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

/// `view.ribbon_tabs_only`: fold the ribbon to its tab row, or show it whole (#10112).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RibbonTabsOnlyParams {
    /// True folds the ribbon to its tab row, false shows it whole; omitted switches between the two.
    #[serde(default)]
    pub on: Option<bool>,
}

/// What `view.ribbon_tabs_only` returns: the request echoed. The window applies it to its
/// preference; the design is not changed and nothing is added to undo.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RibbonTabsOnly {
    /// The requested state; absent when the call asked to switch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on: Option<bool>,
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

/// Set the view navigator at the view's corner: its type, and for the custom one the body it
/// draws, its texture and the controls around it. The navigator is the app's, kept with the
/// preferences and settings profiles, never in the design; without an app the call only checks
/// its parameters. Never opens a dialog.
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NavigatorParams {
    /// The navigator: signed axes, the cube, or a custom body (the cube until a body is
    /// chosen). Default: unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<NavigatorStyle>,
    /// A body of the open design for the custom navigator to draw; the app keeps a snapshot of
    /// it, so it stays when the design closes. `null`: no body (the cube again). A body over
    /// 10,000 triangles, or whose thinnest width is under a fifth of its widest, is refused.
    /// Default: unchanged.
    #[serde(default, deserialize_with = "nullable", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub body: Option<Option<String>>,
    /// The custom navigator's colours: the cube's (`themed`) or the body's own appearance and
    /// edge colour (`modeled`). Default: unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<NavigatorTexture>,
    /// The cube's orbit and roll arrows around the custom navigator. Default: unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrows: Option<bool>,
    /// The cube's Home button beside the custom navigator. Default: unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<bool>,
}

/// A view navigator type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NavigatorStyle {
    Axes,
    Cube,
    Custom,
}

/// How the custom view navigator colours its body.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NavigatorTexture {
    Themed,
    Modeled,
}

/// A key that may be given as `null` (`Some(None)`), apart from absent (`None`).
fn nullable<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(d).map(Some)
}

/// A key's value as its handler reads it: `read` takes the raw JSON value, and anything it
/// refuses gets the handler's own message, whole (no key put in front).
pub(super) fn read_with<'de, D: Deserializer<'de>, T>(d: D, message: &str, read: impl FnOnce(&Value) -> Option<T>) -> Result<Option<T>, D::Error> {
    let value = Value::deserialize(d)?;
    read(&value).map(Some).ok_or_else(|| serde::de::Error::custom(whole(message)))
}

/// A value `T` deserializes from, or the handler's message.
pub(super) fn read_as<'de, D: Deserializer<'de>, T: serde::de::DeserializeOwned>(d: D, message: &str) -> Result<Option<T>, D::Error> {
    read_with(d, message, |v| serde_json::from_value(v.clone()).ok())
}

/// A deserializer for a key, with the handler's message for a value it refuses.
macro_rules! reads {
    ($name:ident -> $ty:ty, $message:literal) => {
        fn $name<'de, D: Deserializer<'de>>(d: D) -> Result<Option<$ty>, D::Error> {
            read_as(d, $message)
        }
    };
    ($name:ident -> $ty:ty, $message:literal, $read:expr) => {
        fn $name<'de, D: Deserializer<'de>>(d: D) -> Result<Option<$ty>, D::Error> {
            read_with(d, $message, $read)
        }
    };
}

/// A key the handlers refuse when it is left out, with their message: optional in the type, so
/// the check leaves the refusal to the handler, and required in the schema.
fn require(schema: &mut Schema, key: &str) {
    super::motion::require(schema, key);
}

fn mode_required(schema: &mut Schema) {
    require(schema, "mode");
}

fn preset_required(schema: &mut Schema) {
    require(schema, "preset");
}

fn enabled_required(schema: &mut Schema) {
    require(schema, "enabled");
}

fn camera_required(schema: &mut Schema) {
    require(schema, "camera");
}

/// How the workspace is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LayoutMode {
    Floating,
    Compact,
}

reads!(layout_mode -> LayoutMode, "mode must be floating or compact");

/// `view.layout`: the workspace layout, floating or compact. A display preference the app keeps;
/// the design, the camera and the active tool are not changed, and it never opens a dialog.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = mode_required)]
pub struct LayoutParams {
    // Optional in the type and required in the schema (`mode_required`): left out, the handler
    // refuses it with its own message.
    /// `floating` or `compact`.
    #[serde(default, deserialize_with = "layout_mode", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "LayoutMode")]
    pub mode: Option<LayoutMode>,
}

/// What `view.layout` returns: the layout asked for.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LayoutResult {
    pub mode: LayoutMode,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Where the history sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HistoryDock {
    Bottom,
    Left,
    UnderBrowser,
    /// The history in the browser, with no timeline.
    Browser,
}

reads!(history_width -> f64, "width must be finite and within 1..1000000", |v| v.as_f64().filter(|w| w.is_finite() && (1.0..=1_000_000.0).contains(w)));
reads!(history_dock -> HistoryDock, "dock must be bottom, left, under_browser or browser");
reads!(history_collapsed -> bool, "collapsed must be a boolean");

/// `view.history_layout`: the history's width, dock and whether it is collapsed; each given
/// key is checked and echoed, and a key left out stays as it is. A display preference the app
/// keeps; the design is not changed and it never opens a dialog.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct HistoryLayoutParams {
    /// The history's width, 1 to 1000000.
    #[serde(default, deserialize_with = "history_width", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64", range(min = 1.0, max = 1000000.0))]
    pub width: Option<f64>,
    /// Where the history sits.
    #[serde(default, deserialize_with = "history_dock", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "HistoryDock")]
    pub dock: Option<HistoryDock>,
    /// True collapses the history, false opens it.
    #[serde(default, deserialize_with = "history_collapsed", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub collapsed: Option<bool>,
}

/// What `view.history_layout` returns: the keys given, checked.
#[derive(Debug, Serialize, JsonSchema)]
pub struct HistoryLayoutResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dock: Option<HistoryDock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collapsed: Option<bool>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A graphics preset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GraphicsPreset {
    Performance,
    Quality,
}

/// The tessellation a graphics preset asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PresetQuality {
    Fine,
    Normal,
}

reads!(graphics_preset_value -> GraphicsPreset, "preset must be performance or quality");

/// `view.graphics_preset`: one preset for ambient occlusion, tessellation and supersampling.
/// Quality: ambient occlusion on, Fine, adaptive 2x supersampling. Performance: no ambient
/// occlusion, Normal, no supersampling. MSAA stays independent. A display setting the app keeps;
/// the design is not changed.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = preset_required)]
pub struct GraphicsPresetParams {
    // Optional in the type and required in the schema (`preset_required`), as `mode` above.
    /// `performance` or `quality`.
    #[serde(default, deserialize_with = "graphics_preset_value", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "GraphicsPreset")]
    pub preset: Option<GraphicsPreset>,
}

/// What `view.graphics_preset` returns: the preset and the settings it stands for.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GraphicsPresetResult {
    pub preset: GraphicsPreset,
    /// Ambient occlusion on or off.
    pub enabled: bool,
    pub display_quality: PresetQuality,
    /// The supersampling factor: 2 for Quality, 1 for Performance.
    pub supersampling: f64,
    pub supersampling_adaptive: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

reads!(ambient_occlusion_enabled -> bool, "enabled must be a boolean");

/// `view.ambient_occlusion`: soft contact shading in the GPU viewport, on or off. A display
/// setting the app keeps; the design is not changed.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = enabled_required)]
pub struct AmbientOcclusionParams {
    // Optional in the type and required in the schema (`enabled_required`), as `mode` above.
    /// True turns it on, false off.
    #[serde(default, deserialize_with = "ambient_occlusion_enabled", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub enabled: Option<bool>,
}

/// What `view.ambient_occlusion` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AmbientOcclusionResult {
    /// Whether it is on now.
    pub enabled: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What a navigation turns the camera by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NavigateAction {
    Left,
    Right,
    Up,
    Down,
    Clockwise,
    Counterclockwise,
    /// The isometric view, framed on the whole model.
    Home,
    /// Frame the whole model.
    Fit,
}

/// A direct manipulation of the camera.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NavigateMotion {
    Pan,
    Orbit,
    Zoom,
}

/// A point of finite numbers, `[x, y]` or `[x, y, z]` (or an object with those keys), each
/// within `lo..=hi`.
fn bounded_point(keys: &[&str], lo: f64, hi: f64) -> Schema {
    let n = keys.len();
    // `params::finite` takes numbers below 1e9 in size; the others' bounds are inclusive.
    let number = if hi >= 1e9 {
        json!({"type": "number", "exclusiveMinimum": lo, "exclusiveMaximum": hi})
    } else {
        json!({"type": "number", "minimum": lo, "maximum": hi})
    };
    let properties: serde_json::Map<String, Value> = keys.iter().map(|k| ((*k).to_string(), number.clone())).collect();
    json_schema!({
        "anyOf": [
            {"type": "array", "items": number, "minItems": n, "maxItems": n},
            {"type": "object", "properties": properties, "required": keys}
        ]
    })
}

fn degrees_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"description": "Degrees, above 0 and at most 180.", "type": "number", "exclusiveMinimum": 0, "maximum": 180})
}

fn point3_schema(_: &mut SchemaGenerator) -> Schema {
    let mut s = bounded_point(&["x", "y", "z"], -1e9, 1e9);
    s.insert("description".into(), "A point [x, y, z] or {\"x\", \"y\", \"z\"}, of numbers.".into());
    s
}

fn delta_schema(_: &mut SchemaGenerator) -> Schema {
    let mut s = bounded_point(&["x", "y"], -1e4, 1e4);
    s.insert("description".into(), "Pixels [dx, dy] (or {\"x\", \"y\"}), each within ±10000.".into());
    s
}

fn viewport_schema(_: &mut SchemaGenerator) -> Schema {
    let mut s = bounded_point(&["x", "y"], 1.0, 1e6);
    s.insert("description".into(), "The viewport's size in pixels [width, height] (or {\"x\", \"y\"}), each 1 to 1000000.".into());
    s
}

/// The camera `view.navigate` turns: as the other view commands read it, and also with yaw and
/// roll within ±1e6 and pitch within ±π/2 (else "invalid camera"). Read before anything else.
fn navigate_camera<'de, D: Deserializer<'de>>(d: D) -> Result<Option<CameraParam>, D::Error> {
    read_with(d, "invalid camera", |v| {
        crate::cmd::camera_form(v)
            .filter(|c| c.yaw.abs() <= 1e6 && c.roll.abs() <= 1e6 && c.pitch.abs() <= std::f64::consts::FRAC_PI_2)
            .map(CameraParam)
    })
}

reads!(navigate_fit -> bool, "fit must be a boolean");
reads!(
    navigate_direction -> Vec3,
    "direction must be a finite nonzero vector of length at most 2",
    |v| crate::params::vec3(v).filter(|v| v.is_finite() && v.len() <= 2.0 && v.normalized().is_some())
);
reads!(navigate_action -> NavigateAction, "unknown navigation action");
reads!(
    navigate_degrees -> f64,
    "degrees must be a number above 0 and at most 180",
    |v| v.as_f64().filter(|d| d.is_finite() && *d > 0.0 && *d <= 180.0)
);
reads!(navigate_motion -> NavigateMotion, "unknown navigation motion");
reads!(navigate_free_orbit -> bool, "free_orbit must be a boolean");
/// A pan or orbit's `delta` and an orbit's `viewport` are read in two steps, as the handler
/// does: a point, then its bounds, each with its own message.
fn bounded_point_key<'de, D: Deserializer<'de>>(d: D, key: &str, in_bounds: fn(Vec2) -> bool, out_of_bounds: &str) -> Result<Option<Vec2>, D::Error> {
    let point = read_with(d, &format!("`{key}` must be a point [x, y]"), crate::params::vec2)?;
    match point {
        Some(p) if !in_bounds(p) => Err(serde::de::Error::custom(whole(out_of_bounds))),
        point => Ok(point),
    }
}

fn navigate_delta<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec2>, D::Error> {
    bounded_point_key(d, "delta", |p| p.x.abs() <= 1e4 && p.y.abs() <= 1e4, "motion deltas must be within ±10000 pixels")
}

fn navigate_viewport<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec2>, D::Error> {
    bounded_point_key(d, "viewport", |p| (1.0..=1e6).contains(&p.x) && (1.0..=1e6).contains(&p.y), "viewport dimensions must be in 1..1000000")
}
reads!(navigate_pivot -> Vec3, "`pivot` must be a point [x, y, z]", crate::params::vec3);
reads!(navigate_anchor -> Vec3, "`anchor` must be a point [x, y, z]", crate::params::vec3);
reads!(
    navigate_factor -> f64,
    "factor must be in 0.05..20",
    |v| v.as_f64().filter(|f| f.is_finite() && f.abs() < 1e9 && (0.05..=20.0).contains(f))
);

/// `view.navigate`: turn, pan, orbit or zoom a camera without changing the design or adding to
/// undo. `camera` is always needed, and then one of three: `direction` (look from it), `action`
/// (a turn, a roll, home or fit) or `motion` (direct manipulation, which wins when given and
/// does not take `fit`). Keys of the other forms are not used.
// Deserialize is written out below rather than derived: the handler reads only the keys of the
// form it takes, so a key of another form is neither read nor checked.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = camera_required)]
pub struct NavigateParams {
    // Optional in the type and required in the schema (`camera_required`), as `mode` above.
    /// The camera to move, as `ui.inspect` gives it. Its yaw and roll must also be within ±1e6
    /// and its pitch within ±π/2.
    #[serde(default, deserialize_with = "navigate_camera", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "CameraParam")]
    pub camera: Option<CameraParam>,
    /// Also frame the whole model from the new direction, as home does. Not with `motion`.
    /// Default: false.
    #[serde(default, deserialize_with = "navigate_fit", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub fit: Option<bool>,
    /// Look from this direction, in the camera's up-axis frame: a finite nonzero vector of
    /// length at most 2.
    #[serde(default, deserialize_with = "navigate_direction", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "point3_schema")]
    pub direction: Option<Vec3>,
    /// A turn without `direction`: left, right, up or down turn the view `degrees`, clockwise and
    /// counterclockwise roll it a quarter turn, home shows the isometric view framed, fit frames
    /// the model.
    #[serde(default, deserialize_with = "navigate_action", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "NavigateAction")]
    pub action: Option<NavigateAction>,
    /// How far left, right, up or down turn the view, above 0 and at most 180. Default: 90.
    #[serde(default, deserialize_with = "navigate_degrees", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "degrees_schema")]
    pub degrees: Option<f64>,
    /// Direct manipulation: pan and orbit need `delta`, pan and an orbit about a `pivot` need
    /// `viewport`, zoom needs `factor`.
    #[serde(default, deserialize_with = "navigate_motion", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "NavigateMotion")]
    pub motion: Option<NavigateMotion>,
    /// Pan or orbit by this many pixels.
    #[serde(default, deserialize_with = "navigate_delta", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "delta_schema")]
    pub delta: Option<Vec2>,
    /// Orbit through the poles. Default: false.
    #[serde(default, deserialize_with = "navigate_free_orbit", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub free_orbit: Option<bool>,
    /// The viewport's size in pixels, for a pan or an orbit about a pivot.
    #[serde(default, deserialize_with = "navigate_viewport", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "viewport_schema")]
    pub viewport: Option<Vec2>,
    /// Orbit about this point (mm) rather than the camera's target.
    #[serde(default, deserialize_with = "navigate_pivot", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "point3_schema")]
    pub pivot: Option<Vec3>,
    /// The zoom factor, 0.05 to 20.
    #[serde(default, deserialize_with = "navigate_factor", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64", range(min = 0.05, max = 20.0))]
    pub factor: Option<f64>,
    /// Zoom towards this point (mm). Default: the camera's target.
    #[serde(default, deserialize_with = "navigate_anchor", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "point3_schema")]
    pub anchor: Option<Vec3>,
}

impl<'de> Deserialize<'de> for NavigateParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let p = serde_json::Map::<String, Value>::deserialize(d)?;
        navigate_form(&p).map_err(serde::de::Error::custom)
    }
}

/// A key's value through its reader, or `None` when the key is left out.
fn navigate_key<'a, T>(
    p: &'a serde_json::Map<String, Value>,
    key: &str,
    read: impl FnOnce(&'a Value) -> Result<Option<T>, serde_json::Error>,
) -> Result<Option<T>, serde_json::Error> {
    p.get(key).map_or(Ok(None), read)
}

/// The keys `view.navigate`'s handler reads, in its order: the camera and `fit`, then those of
/// a motion (each motion its own), of a direction, or of an action.
fn navigate_form(p: &serde_json::Map<String, Value>) -> Result<NavigateParams, serde_json::Error> {
    let mut form = NavigateParams {
        camera: navigate_key(p, "camera", navigate_camera)?,
        fit: navigate_key(p, "fit", navigate_fit)?,
        direction: None,
        action: None,
        degrees: None,
        motion: None,
        delta: None,
        free_orbit: None,
        viewport: None,
        pivot: None,
        factor: None,
        anchor: None,
    };
    if p.contains_key("motion") {
        form.free_orbit = navigate_key(p, "free_orbit", navigate_free_orbit)?;
        form.motion = navigate_key(p, "motion", navigate_motion)?;
        match form.motion {
            Some(NavigateMotion::Pan) => {
                form.delta = navigate_key(p, "delta", navigate_delta)?;
                form.viewport = navigate_key(p, "viewport", navigate_viewport)?;
            }
            Some(NavigateMotion::Orbit) => {
                form.delta = navigate_key(p, "delta", navigate_delta)?;
                // An orbit needs the viewport only about a pivot.
                if p.contains_key("pivot") {
                    form.pivot = navigate_key(p, "pivot", navigate_pivot)?;
                    form.viewport = navigate_key(p, "viewport", navigate_viewport)?;
                }
            }
            Some(NavigateMotion::Zoom) => {
                form.factor = navigate_key(p, "factor", navigate_factor)?;
                form.anchor = navigate_key(p, "anchor", navigate_anchor)?;
            }
            None => {}
        }
    } else if p.contains_key("direction") {
        form.direction = navigate_key(p, "direction", navigate_direction)?;
    } else {
        form.degrees = navigate_key(p, "degrees", navigate_degrees)?;
        form.action = navigate_key(p, "action", navigate_action)?;
    }
    Ok(form)
}

/// What `view.navigate` returns: the camera after the move, and whether the window should also
/// frame the whole model. The window applies both; the design is not changed.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NavigateResult {
    /// The camera now.
    #[schemars(with = "CameraParam")]
    pub camera: solidcraft_viewport::Camera,
    /// Frame the whole model from the new direction.
    pub fit: bool,
    /// True for a `motion`; absent for a turn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continuous: Option<bool>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// The camera tool the viewport latches: `view.tool`'s `tool`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum NavigationTool {
    Orbit,
    Pan,
    Zoom,
    /// No tool: releases the one that was latched.
    None,
}

reads!(navigation_tool -> NavigationTool, "tool must be orbit, pan, zoom or none");

/// `view.tool`: latch one of the viewport's camera tools, or `none` to release the one that is
/// latched. A display setting the window applies; the design and the camera are not changed, and
/// it never opens a dialog.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = tool_required)]
pub struct ToolParams {
    // Optional in the type and required in the schema (`tool_required`): left out, the handler
    // refuses it with its own message.
    /// `orbit`, `pan`, `zoom` or `none`.
    #[serde(default, deserialize_with = "navigation_tool", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "NavigationTool")]
    pub tool: Option<NavigationTool>,
}

/// What `view.tool` returns: the tool the window latches.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ToolResult {
    pub tool: NavigationTool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// How the model is drawn in the viewport: `view.visual_style`'s `style`. The order of the
/// variants is the index the window uses for the style.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VisualStyle {
    ShadedWithEdges,
    Shaded,
    Wireframe,
    ShadedWithHiddenEdges,
}

impl VisualStyle {
    /// The index the window keeps the style under, in the order the names are listed.
    pub fn index(self) -> u8 {
        match self {
            VisualStyle::ShadedWithEdges => 0,
            VisualStyle::Shaded => 1,
            VisualStyle::Wireframe => 2,
            VisualStyle::ShadedWithHiddenEdges => 3,
        }
    }
}

reads!(
    visual_style_value -> VisualStyle,
    "style must be shaded_with_edges, shaded, wireframe or shaded_with_hidden_edges"
);

/// `view.visual_style`: how the model is drawn in the viewport. A display setting the window
/// applies; the design and the camera are not changed, and it never opens a dialog.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = style_required)]
pub struct VisualStyleParams {
    // Optional in the type and required in the schema (`style_required`): left out, the handler
    // refuses it with its own message.
    /// `shaded_with_edges`, `shaded`, `wireframe` or `shaded_with_hidden_edges`.
    #[serde(default, deserialize_with = "visual_style_value", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "VisualStyle")]
    pub style: Option<VisualStyle>,
}

/// What `view.visual_style` returns: the style asked for and the index the window uses for it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct VisualStyleResult {
    pub style: VisualStyle,
    /// 0 for `shaded_with_edges`, 1 for `shaded`, 2 for `wireframe`, 3 for `shaded_with_hidden_edges`.
    pub index: u8,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

fn tool_required(schema: &mut Schema) {
    require(schema, "tool");
}

fn style_required(schema: &mut Schema) {
    require(schema, "style");
}

#[cfg(test)]
#[path = "view_navigation_tests.rs"]
mod tests;
