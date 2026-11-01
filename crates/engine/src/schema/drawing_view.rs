//! Contracts and result types for the commands in `cmd/drawing_view.rs`.
//!
//! The view's own settings are `solidcraft_doc::drawing_view::ViewSettings`, which a saved view
//! (W12) keeps too. The contract lists the same fields, each read by that type's own reader, and
//! `the_contract_lists_the_view_settings` keeps the two lists the same. A saved view's own
//! settings (`ViewOverrides`) are read and changed through `drawing.view.inspect` and
//! `drawing.view.edit`, each setting with where its value comes from.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use solidcraft_doc::drawing_view::{TangentEdges, ViewDirection, ViewSettings};
use solidcraft_geom::Seg2;
use solidcraft_geom::drawing_sheet::{Orientation, PaperSize, SheetSize};
use solidcraft_geom::drawing_view::{NamedView, ProjectedSide, Scale};
use solidcraft_geom::paper::ObjectKind;
use solidcraft_geom::section::CurveAccuracy;
use solidcraft_render::VisualStyle;

use crate::drawing_view::{CHORD_MM, CHORD_RANGE, Stats};

use super::Point2;

/// The lowest and highest picture resolution a sheet takes, in dots per inch.
pub const DPI_RANGE: std::ops::RangeInclusive<f64> = 25.0..=2400.0;

/// What `drawing.view.snapshot` takes: the view's settings, the page it fits on, and where its
/// centre goes.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct SnapshotParams {
    pub view: ViewParam,
    #[serde(deserialize_with = "super::present")]
    #[schemars(with = "ScaleParam")]
    pub scale: Option<ScaleParam>,
    pub display_style: StyleParam,
    pub reference_style: StyleParam,
    /// Whether reference parts hide the model's lines behind them (false when not given).
    pub reference_hides_model: bool,
    /// Model millimetres added on every side of the view's extents, so reference parts around
    /// the model show inside its boundary: 0 to 100000 (0 when not given).
    pub reference_margin_mm: f64,
    pub tangent_edges: TangentEdgesParam,
    pub sheet: SheetParam,
    /// Where the view's centre goes on the sheet, in paper millimetres from its lower-left
    /// corner (the sheet's centre when not given).
    #[serde(deserialize_with = "super::present")]
    #[schemars(with = "Point2")]
    pub position_mm: Option<Point2>,
    pub chord_mm: ChordParam,
    /// The accuracy the lines must certify, in paper millimetres: a check, never a setting. When
    /// not given, the reply says what they achieved.
    #[serde(deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub max_error_mm: Option<f64>,
    /// Whether the reply carries the view's lines as a paper scene of the sheet (`scene`), the
    /// strokes the sheet writers draw (false when not given).
    pub scene: bool,
    /// The occurrence subtrees the view draws (every body the design places when not given).
    #[serde(deserialize_with = "super::present")]
    #[schemars(with = "SourceParam")]
    pub source: Option<SourceParam>,
    /// Occurrence paths left out of this view, with everything under them (none when not given).
    pub hide: Vec<Vec<u64>>,
}

/// What a view draws: occurrence paths from the design's root, each the subtree it places, with
/// `[]` for the design's own bodies.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct SourceParam {
    pub occurrences: Vec<Vec<u64>>,
}

impl SnapshotParams {
    pub fn settings(&self) -> ViewSettings {
        ViewSettings {
            view: self.view.0,
            scale: self.scale.map(|s| s.0),
            display_style: self.display_style.0,
            reference_style: self.reference_style.0,
            reference_hides_model: self.reference_hides_model,
            reference_margin_mm: self.reference_margin_mm,
            tangent_edges: self.tangent_edges.0,
        }
    }
}

// Each parameter type below reads as the type it wraps, so the contract accepts exactly what the
// view's settings do; it is there for its schema.

/// Which way the view looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
#[serde(transparent)]
pub struct ViewParam(pub ViewDirection);

impl JsonSchema for ViewParam {
    fn schema_name() -> Cow<'static, str> {
        "DrawingView".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let names: Vec<Value> = NamedView::ALL.iter().filter_map(|v| serde_json::to_value(v).ok()).chain([json!("iso")]).collect();
        let vector = json!({"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3});
        json_schema!({
            "description": "Which way the view looks: a named view (front when not given; iso is iso_front_right), or {direction, up, origin?}: the line of sight, the way up the paper (not along the direction) and the point the view's plane passes through (the world origin when not given).",
            "anyOf": [
                {"type": "string", "enum": names},
                {"type": "object", "properties": {"direction": vector, "up": vector, "origin": vector}, "required": ["direction", "up"], "additionalProperties": false}
            ]
        })
    }
}

/// A drawing scale, paper:model.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(transparent)]
pub struct ScaleParam(pub Scale);

impl JsonSchema for ScaleParam {
    fn schema_name() -> Cow<'static, str> {
        "DrawingScale".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "The scale as paper:model text, such as \"1:2\" or \"5:1\", from 1:100000 to 1000:1. When not given, the largest ISO 5455 scale at which the view fits the sheet's drawing area.",
            "type": "string",
            "pattern": "^[0-9]+(\\.[0-9]+)?:[0-9]+(\\.[0-9]+)?$"
        })
    }
}

/// How the model draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
#[serde(transparent)]
pub struct StyleParam(pub VisualStyle);

impl JsonSchema for StyleParam {
    fn schema_name() -> Cow<'static, str> {
        "VisualStyle".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let names: Vec<Value> = VisualStyle::ALL.iter().filter_map(|s| serde_json::to_value(s).ok()).collect();
        json_schema!({
            "description": "How the model draws: visible (the default), visible_hidden, shaded, shaded_visible or shaded_visible_hidden.",
            "type": "string",
            "enum": names
        })
    }
}

/// Tangent edges hidden or shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
#[serde(transparent)]
pub struct TangentEdgesParam(pub TangentEdges);

impl JsonSchema for TangentEdgesParam {
    fn schema_name() -> Cow<'static, str> {
        "TangentEdges".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "Edges where faces meet smoothly: hide (the default) or show.", "type": "string", "enum": ["hide", "show"]})
    }
}

/// The chord the lines are sampled at, in paper millimetres.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(transparent)]
pub struct ChordParam(pub f64);

impl Default for ChordParam {
    fn default() -> Self {
        ChordParam(CHORD_MM)
    }
}

impl JsonSchema for ChordParam {
    fn schema_name() -> Cow<'static, str> {
        "ViewChord".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "The chord, in paper millimetres, of the meshes a view's lines are sampled from (0.01 when not given): a sampling setting, never an accuracy promise.",
            "type": "number",
            "minimum": *CHORD_RANGE.start(),
            "maximum": *CHORD_RANGE.end()
        })
    }
}

/// The page the view fits on: its size, the way it lies and its picture resolution.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SheetParam {
    pub size: SheetSize,
    pub orientation: Orientation,
    pub dpi: f64,
}

impl Default for SheetParam {
    fn default() -> Self {
        SheetParam { size: SheetSize::Standard(PaperSize::A3), orientation: Orientation::Landscape, dpi: 300.0 }
    }
}

impl JsonSchema for SheetParam {
    fn schema_name() -> Cow<'static, str> {
        "SnapshotSheet".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let papers: Vec<Value> = PaperSize::ALL.iter().filter_map(|p| serde_json::to_value(p).ok()).collect();
        let sides = json!({"type": "array", "items": {"type": "number", "exclusiveMinimum": 0}, "minItems": 2, "maxItems": 2});
        let standard = json!({"type": "object", "properties": {"standard": {"type": "string", "enum": papers}}, "required": ["standard"], "additionalProperties": false});
        let custom = json!({
            "type": "object",
            "properties": {"custom": {"type": "object", "properties": {"sides_mm": sides}, "required": ["sides_mm"], "additionalProperties": false}},
            "required": ["custom"],
            "additionalProperties": false
        });
        json_schema!({
            "description": "The page the view fits and centres on (A3 landscape at 300 dpi when not given).",
            "type": "object",
            "properties": {
                "size": {"anyOf": [standard, custom]},
                "orientation": {"type": "string", "enum": ["landscape", "portrait"]},
                "dpi": {"type": "number", "minimum": *DPI_RANGE.start(), "maximum": *DPI_RANGE.end()}
            },
            "additionalProperties": false
        })
    }
}

/// A saved view, by its id as `drawing.inspect` lists it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewIdParam(pub u64);

impl<'de> Deserialize<'de> for ViewIdParam {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) => solidcraft_doc::ids::decode(&s).map(ViewIdParam).ok_or_else(|| serde::de::Error::custom("must be a view's id")),
            _ => Err(serde::de::Error::custom("must be a view's id")),
        }
    }
}

impl JsonSchema for ViewIdParam {
    fn schema_name() -> Cow<'static, str> {
        "DrawingViewId".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A view's id: 13 characters of lowercase Crockford base32.", "type": "string", "pattern": "^[0-9a-hjkmnp-tv-z]{13}$"})
    }
}

/// Where a saved view's centre sits on its sheet, in paper millimetres.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PlaceParam {
    /// At this point of the sheet, from its lower-left corner.
    At([f64; 2]),
    /// Lined up with its parent, this far along the free axis (a projected view only).
    Aligned(f64),
}

/// What `drawing.view.inspect` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct InspectParams {
    pub view: ViewIdParam,
}

/// What `drawing.view.edit` takes: the view, and what to change.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct EditParams {
    pub view: ViewIdParam,
    pub set: ViewSet,
}

/// The settings `drawing.view.edit` changes, each only when given. For the five that can follow
/// the view's parent, `null` means follow it (a base view's scale can't: it has no parent).
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct ViewSet {
    #[serde(deserialize_with = "super::present")]
    pub scale: Option<Option<ScaleParam>>,
    #[serde(deserialize_with = "super::present")]
    pub display_style: Option<Option<StyleParam>>,
    #[serde(deserialize_with = "super::present")]
    pub reference_style: Option<Option<StyleParam>>,
    #[serde(deserialize_with = "super::present")]
    pub reference_hides_model: Option<Option<bool>>,
    #[serde(deserialize_with = "super::present")]
    pub tangent_edges: Option<Option<TangentEdgesParam>>,
    /// Model millimetres added on every side of the view's extents: 0 to 100000.
    #[serde(deserialize_with = "super::present")]
    #[schemars(with = "f64")]
    pub reference_margin_mm: Option<f64>,
    /// A base view's direction.
    #[serde(deserialize_with = "super::present")]
    #[schemars(with = "ViewParam")]
    pub view: Option<ViewParam>,
    #[serde(deserialize_with = "super::present")]
    #[schemars(with = "PlaceParam")]
    pub place: Option<PlaceParam>,
    /// A projected view's display settings (styles, reference parts, tangent edges) all following
    /// its parent (true), or all its own, at the values they have now (false).
    #[serde(deserialize_with = "super::present")]
    #[schemars(with = "bool")]
    pub linked: Option<bool>,
}

/// Where a setting's value comes from.
#[derive(Clone, Copy, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FromResult {
    This,
    Parent,
    Default,
}

/// One of a saved view's settings: its value, where it comes from (the parent's id when it is a
/// parent), and what the view sets itself (null when it follows).
#[derive(Debug, Serialize, JsonSchema)]
pub struct SettingResult<T> {
    pub value: T,
    pub from: FromResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub set: Option<T>,
}

/// A saved view's settings.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SettingsResult {
    pub scale: SettingResult<String>,
    pub display_style: SettingResult<String>,
    pub reference_style: SettingResult<String>,
    pub reference_hides_model: SettingResult<bool>,
    pub tangent_edges: SettingResult<String>,
}

/// The values a window offers for a view's settings.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ChoicesResult {
    pub display_style: Vec<String>,
    /// ISO 5455's preset scales, largest first.
    pub scale: Vec<String>,
}

/// What `drawing.view.inspect` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct InspectResult {
    pub view: String,
    pub sheet: String,
    /// `base` or `projected`.
    pub kind: &'static str,
    /// The design it draws: its own source, or its base view's.
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// The side of its parent a projected view sits on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
    /// Which way it looks, as resolved through its parents.
    pub direction: ViewResult,
    pub place: PlaceParam,
    pub settings: SettingsResult,
    pub reference_margin_mm: f64,
    /// Where it sits on its sheet: its extents grown by its reference margin, at its scale, round
    /// its centre; a placeholder 20 mm each way when there's nothing to measure.
    pub boundary_mm: BoxResult,
    /// Its design's extents on its plane.
    pub extents: ExtentsResult,
    /// A projected view's display settings: `true` when all follow its parent, `false` when none
    /// does, `"partly"` otherwise. None for a base view.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Value")]
    pub linked: Option<Value>,
    /// Its dimensions' ids, in id order.
    pub dimensions: Vec<String>,
    /// Its balloons' ids, in id order.
    pub balloons: Vec<String>,
    pub choices: ChoicesResult,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `drawing.view.edit` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EditResult {
    pub view: String,
    /// The fields the edit changed.
    pub changed: Vec<String>,
    /// The other views whose resolved settings changed with it: the views projected from it.
    pub followers: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `drawing.view.move_to_sheet` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct MoveParams {
    pub view: ViewIdParam,
    /// The sheet, by id or name.
    #[serde(deserialize_with = "super::drawing_sheet::sheet_ref")]
    pub sheet: String,
    /// Where its centre goes, in paper millimetres from the sheet's lower-left corner.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Point2")]
    pub position_mm: Option<Point2>,
    /// A view lined up with its parent moves only when this is true, and is then placed apart.
    #[serde(default)]
    pub break_alignment: bool,
}

/// What `drawing.view.copy_to_sheet` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct CopyParams {
    pub view: ViewIdParam,
    /// The sheet, by id or name.
    #[serde(deserialize_with = "super::drawing_sheet::sheet_ref")]
    pub sheet: String,
    /// Where the copy's centre goes, in paper millimetres from the sheet's lower-left corner.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Point2")]
    pub position_mm: Option<Point2>,
    /// Whether the views lined up with it are copied too, lined up with the copy. Needed when it
    /// has any.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub with_linked: Option<bool>,
}

/// What `drawing.view.base` takes: the sheet, which way the view looks, its scale and where its
/// centre goes, and each setting it sets itself (a setting left out follows the default).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct BaseParams {
    /// The sheet, by id or name.
    #[serde(deserialize_with = "super::drawing_sheet::sheet_ref")]
    pub sheet: String,
    /// Which way it looks (front when not given).
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ViewParam")]
    pub view: Option<ViewParam>,
    /// Its scale (when not given, the largest preset at which it lies inside the sheet's drawing
    /// space, or 1:1 with nothing to measure); saved either way.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ScaleParam")]
    pub scale: Option<ScaleParam>,
    /// Where its centre goes, in paper millimetres from the sheet's lower-left corner (the sheet's
    /// centre when not given).
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Point2")]
    pub position_mm: Option<Point2>,
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "StyleParam")]
    pub display_style: Option<StyleParam>,
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "StyleParam")]
    pub reference_style: Option<StyleParam>,
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub reference_hides_model: Option<bool>,
    /// Model millimetres added on every side of its extents: 0 to 100000 (0 when not given).
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub reference_margin_mm: Option<f64>,
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "TangentEdgesParam")]
    pub tangent_edges: Option<TangentEdgesParam>,
}

/// What `drawing.view.projected` takes: its parent, the side of it it sits on, and where.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct ProjectedParams {
    pub parent: ViewIdParam,
    pub side: SideParam,
    /// Where it sits (when not given, lined up with its parent, a gap past the parent's boundary).
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PlaceParam")]
    pub place: Option<PlaceParam>,
}

/// The side of its parent a projected view sits on.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(transparent)]
pub struct SideParam(pub ProjectedSide);

impl JsonSchema for SideParam {
    fn schema_name() -> Cow<'static, str> {
        "ProjectedSide".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let names: Vec<Value> = ProjectedSide::ALL.iter().filter_map(|s| serde_json::to_value(s).ok()).collect();
        json_schema!({
            "description": "The side of its parent it sits on: left, right, above or below. The drawing's projection says which side of the model it shows.",
            "type": "string",
            "enum": names
        })
    }
}

/// What `drawing.view.base` and `drawing.view.projected` return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CreateResult {
    /// The new view.
    pub view: String,
    pub sheet: String,
    /// The scale it draws at: given or fitted for a base view, its parent's for a projected one.
    pub scale: ScaleResult,
    pub boundary_mm: BoxResult,
    /// It, when its boundary leaves the sheet's drawing space; it isn't moved back in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outside: Vec<OutsideResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `drawing.view.move_to_sheet` and `drawing.view.copy_to_sheet` return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ToSheetResult {
    /// The view moved, or its copy.
    pub view: String,
    pub sheet: String,
    /// Every view moved or made: the view's first, then the views lined up with it.
    pub views: Vec<String>,
    /// Those of them whose boundary leaves the sheet's drawing space; none is moved back in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outside: Vec<OutsideResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A view whose boundary leaves its sheet's drawing space, and where it sits.
#[derive(Debug, Serialize, JsonSchema)]
pub struct OutsideResult {
    pub view: String,
    pub boundary_mm: BoxResult,
    /// With nothing to measure, why its boundary is the placeholder's, as `extents.provenance`
    /// names it in `drawing.view.inspect`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unmeasured: Option<&'static str>,
}

/// A view's extents on its own plane, in model millimetres.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ExtentsResult {
    pub min: [f64; 2],
    pub max: [f64; 2],
    /// How far the display mesh may lie inside a curved face: the largest display tolerance.
    pub error_mm: f64,
    /// `approximate`: measured on the display mesh; `placeholder`: nothing to measure, so the box is
    /// the placeholder's.
    pub status: &'static str,
    /// `display_mesh`; with a placeholder, why: `past_walk_limit` or `no_bodies`.
    pub provenance: &'static str,
}

/// A box on the sheet, in paper millimetres.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BoxResult {
    pub min: [f64; 2],
    pub max: [f64; 2],
}

/// The scale the view was drawn at.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ScaleResult {
    pub text: String,
    pub factor: f64,
    /// Whether the scale was worked out to fit the sheet rather than given.
    pub fitted: bool,
}

/// The view's basis as resolved: its name when it has one, the line of sight and the paper axes.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ViewResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub named: Option<String>,
    pub direction: [f64; 3],
    pub x: [f64; 3],
    pub y: [f64; 3],
    pub origin: [f64; 3],
}

/// What the view was drawn from.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SourceResult {
    pub bodies: usize,
    /// Whether the timeline marker is at the end; otherwise the view shows the rolled-back model.
    pub timeline_at_end: bool,
}

/// One source's lines in a view: a placed body's edge or silhouette.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LineResult {
    /// The occurrences that place its body, from the design's root.
    pub occurrence: Vec<u64>,
    /// Its body's name in its definition.
    pub body: String,
    /// Whether the body is a reference part: an occurrence on its path counts as reference.
    pub reference: bool,
    pub source: LineSourceResult,
    pub pieces: Vec<PieceResult>,
}

/// What drew a line: an edge of the body (as `model.edges` numbers them), or a silhouette across
/// one of its faces.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LineSourceResult {
    Edge { edge: usize, tangent: bool },
    Silhouette { face: usize },
}

/// A run of a line at one visibility, on the sheet.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PieceResult {
    /// `visible` or `hidden`.
    pub visibility: &'static str,
    pub path: Vec<PathSegResult>,
    pub accuracy: AccuracyResult,
}

/// A segment of a piece in paper millimetres: a line, or an arc from `start` sweeping `sweep`
/// radians (counter-clockwise when positive).
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct PathSegResult(pub Seg2);

impl JsonSchema for PathSegResult {
    fn schema_name() -> Cow<'static, str> {
        "PaperSegment".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let point = json!({"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2});
        json_schema!({
            "anyOf": [
                {"type": "object", "properties": {"type": {"const": "line"}, "a": point, "b": point}, "required": ["type", "a", "b"], "additionalProperties": false},
                {"type": "object", "properties": {"type": {"const": "arc"}, "center": point, "radius": {"type": "number"}, "start": {"type": "number"}, "sweep": {"type": "number"}}, "required": ["type", "center", "radius", "start", "sweep"], "additionalProperties": false}
            ]
        })
    }
}

/// How closely a piece follows the true projected curve, on paper: `analytic`, `bounded` by
/// `max_deviation_mm`, or `sampled` (measured at its points only, `max_sample_deviation_mm`).
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct AccuracyResult(pub CurveAccuracy);

impl JsonSchema for AccuracyResult {
    fn schema_name() -> Cow<'static, str> {
        "CurveAccuracy".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let mm = json!({"type": "number", "minimum": 0});
        json_schema!({
            "anyOf": [
                {"type": "object", "properties": {"kind": {"const": "analytic"}}, "required": ["kind"], "additionalProperties": false},
                {"type": "object", "properties": {"kind": {"const": "bounded"}, "max_deviation_mm": mm}, "required": ["kind", "max_deviation_mm"], "additionalProperties": false},
                {"type": "object", "properties": {"kind": {"const": "sampled"}, "max_sample_deviation_mm": mm}, "required": ["kind", "max_sample_deviation_mm"], "additionalProperties": false}
            ]
        })
    }
}

/// What a view's lines are and how they were made.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LinesStatusResult {
    /// `approximate`: from the meshes, until the exact lines (W06).
    pub lines: &'static str,
    /// `mesh_projection`.
    pub provenance: &'static str,
    /// The chord asked for on paper, and of the meshes in model millimetres.
    pub chord_mm: f64,
    pub model_chord_mm: f64,
    /// Pieces whose visibility couldn't be told (none from the meshes).
    pub uncertain_pieces: usize,
    /// Pieces drawn where another source lay too.
    pub covered_pieces: usize,
}

/// What working out the lines cost, as the view DXF reports it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LinesStatsResult {
    pub bodies: usize,
    pub shared_meshes: usize,
    pub triangles: usize,
    pub segments: usize,
    pub prepare_ms: f64,
    pub projection_ms: f64,
    pub occlusion_ms: f64,
}

impl From<Stats> for LinesStatsResult {
    fn from(s: Stats) -> Self {
        LinesStatsResult {
            bodies: s.bodies,
            shared_meshes: s.shared_meshes,
            triangles: s.triangles,
            segments: s.segments,
            prepare_ms: s.prepare_ms,
            projection_ms: s.projection_ms,
            occlusion_ms: s.occlusion_ms,
        }
    }
}

/// A view's lines as a paper scene: the sheet's size and the view's one group of strokes (kind
/// `view`, object 0), in paint order, in paper millimetres from the sheet's lower-left corner.
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct SceneResult(pub solidcraft_geom::paper::Scene);

impl JsonSchema for SceneResult {
    fn schema_name() -> Cow<'static, str> {
        "ViewPaperScene".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        paper_scene_schema(generator, &[ObjectKind::View], false)
    }
}

/// A paper scene's schema ([`solidcraft_geom::paper::Scene`]): its groups of `kinds`, each holding
/// strokes, and fills too when `fills`.
pub(super) fn paper_scene_schema(generator: &mut SchemaGenerator, kinds: &[ObjectKind], fills: bool) -> Schema {
    let point = json!({"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2});
    let segment = generator.subschema_for::<PathSegResult>();
    let stroke = json!({
        "type": "object",
        "properties": {"kind": {"const": "stroke"}, "path": {"type": "array", "items": segment}, "closed": {"type": "boolean"}, "width_mm": {"type": "number", "exclusiveMinimum": 0}},
        "required": ["kind", "path", "closed", "width_mm"],
        "additionalProperties": false
    });
    let ring = json!({
        "type": "object",
        "properties": {"segs": {"type": "array", "items": segment}},
        "required": ["segs"],
        "additionalProperties": false
    });
    let region = json!({
        "type": "object",
        "properties": {"outer": ring, "holes": {"type": "array", "items": ring}},
        "required": ["outer", "holes"],
        "additionalProperties": false
    });
    let fill = json!({
        "type": "object",
        "properties": {"kind": {"const": "fill"}, "region": region},
        "required": ["kind", "region"],
        "additionalProperties": false
    });
    let items = if fills { json!({"anyOf": [stroke, fill]}) } else { stroke };
    let kinds: Vec<Value> = kinds.iter().filter_map(|kind| serde_json::to_value(kind).ok()).collect();
    let group = json!({
        "type": "object",
        "properties": {"object": {"type": "integer", "minimum": 0}, "kind": {"enum": kinds}, "items": {"type": "array", "items": items}},
        "required": ["object", "kind", "items"],
        "additionalProperties": false
    });
    json_schema!({
        "type": "object",
        "properties": {"size_mm": point, "groups": {"type": "array", "items": group}},
        "required": ["size_mm", "groups"],
        "additionalProperties": false
    })
}

/// What `drawing.view.snapshot` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SnapshotResult {
    pub view: ViewResult,
    pub scale: ScaleResult,
    pub position_mm: [f64; 2],
    /// The sheet the view is placed on, its defaults filled in.
    pub sheet: SheetParam,
    pub display_style: String,
    pub reference_style: String,
    pub extents: ExtentsResult,
    /// The view's box on the sheet: `position_mm` at its centre, the extents at the scale.
    pub boundary_mm: BoxResult,
    /// Each placed source's lines, in placement order, then edges before silhouettes.
    pub lines: Vec<LineResult>,
    pub status: LinesStatusResult,
    pub stats: LinesStatsResult,
    /// With `scene: true`: the lines as a paper scene of the sheet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scene: Option<SceneResult>,
    pub source: SourceResult,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "drawing_view_tests.rs"]
mod tests;
