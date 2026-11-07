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
use solidcore_record::Seg2;
use solidcraft_doc::drawing_view::{TangentEdges, ViewDirection, ViewSettings};
use solidcraft_geom::drawing_sheet::{Orientation, PaperSize, SheetSize};
use solidcraft_geom::drawing_view::{NamedView, ProjectedSide, Scale};
use solidcraft_geom::paper::line::LineGroup;
use solidcraft_geom::paper::{MAX_DASH_MM, ObjectKind};
use solidcraft_geom::section::CurveAccuracy;
use solidcraft_render::VisualStyle;

use crate::drawing_view::{CHORD_MM, CHORD_RANGE, Stats};

use super::Point2;
use super::drawing_dimension::EdgeParam;
use super::types::Point3;

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
    pub visual_style: StyleParam,
    pub reference_style: StyleParam,
    /// Whether reference parts hide the model's lines behind them (false when not given).
    pub reference_hides_model: bool,
    /// How far the view's boundary grows from the model's box towards the box that also holds its
    /// reference parts, each side on its own: 0 to 100 (0 when not given).
    pub reference_shown_percent: f64,
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
    /// Values to read on the view, as it shows them, with their labels: at most 100.
    pub measure: Vec<MeasureParam>,
    /// The length unit of the labels, `mm`, `cm`, `m`, `in` or `ft` (the design's when not given).
    #[serde(deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub units: Option<String>,
    /// The decimals of the labels, 0 to 6 (2 when not given).
    #[serde(deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u8")]
    pub decimals: Option<u8>,
}

/// A value to read on a view, from model points in the design's frame.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum MeasureParam {
    /// How far apart two points are along the view's x or y.
    Linear { along: AlongParam, from: Point3, to: Point3 },
    /// How far apart two points are as the view shows them.
    Aligned { from: Point3, to: Point3 },
    /// The angle of the line `edge` runs, from its first point, as the view shows it.
    Angle { edge: [Point3; 2], from: AngleFromParam },
    /// A circle's or an arc's radius: the view must see it face on.
    Radius { edge: EdgeParam },
}

/// Which way a linear measurement reads.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AlongParam {
    X,
    Y,
}

/// The line an angle is read from: the view's own, or a line through two model points.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum AngleFromParam {
    View(ViewLineParam),
    Line([Point3; 2]),
}

/// One of the view's own lines.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ViewLineParam {
    Horizontal,
    Vertical,
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
            visual_style: self.visual_style.0,
            reference_style: self.reference_style.0,
            reference_hides_model: self.reference_hides_model,
            reference_shown_percent: self.reference_shown_percent,
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

/// An ISO 128 line group, by its wide line. Every command that takes one refuses anything else in
/// these words, after the key.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineGroupParam(pub LineGroup);

impl<'de> Deserialize<'de> for LineGroupParam {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        serde_json::from_value(Value::deserialize(d)?).map(LineGroupParam).map_err(|_| {
            let (last, rest) = LineGroup::NAMES.split_last().map_or(("", &[][..]), |(last, rest)| (*last, rest));
            serde::de::Error::custom(format!("must be one of {} or {last}", rest.join(", ")))
        })
    }
}

impl JsonSchema for LineGroupParam {
    fn schema_name() -> Cow<'static, str> {
        "LineGroup".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "An ISO 128 line group, by its wide line in millimetres.", "type": "string", "enum": LineGroup::NAMES})
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

/// The settings `drawing.view.edit` changes, each only when given. For the eight that can follow
/// the view's parent, `null` means follow it (a base view's scale can't: it has no parent; a base
/// view's line group follows its sheet's, then its drawing's).
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct ViewSet {
    #[serde(deserialize_with = "super::present")]
    pub scale: Option<Option<ScaleParam>>,
    #[serde(deserialize_with = "super::present")]
    pub visual_style: Option<Option<StyleParam>>,
    #[serde(deserialize_with = "super::present")]
    pub reference_style: Option<Option<StyleParam>>,
    #[serde(deserialize_with = "super::present")]
    pub reference_hides_model: Option<Option<bool>>,
    #[serde(deserialize_with = "super::present")]
    pub tangent_edges: Option<Option<TangentEdgesParam>>,
    /// The line group its lines and dimensions are drawn in.
    #[serde(deserialize_with = "super::present")]
    pub line_group: Option<Option<LineGroupParam>>,
    /// The occurrence subtrees it draws.
    #[serde(deserialize_with = "super::present")]
    pub source: Option<Option<SourceParam>>,
    /// Occurrence paths it leaves out, with everything under them.
    #[serde(deserialize_with = "super::present")]
    pub hide: Option<Option<Vec<Vec<u64>>>>,
    /// How far the view's boundary grows from the model's box towards the box that also holds its
    /// reference parts, each side on its own: 0 to 100.
    #[serde(deserialize_with = "super::present")]
    #[schemars(with = "f64")]
    pub reference_shown_percent: Option<f64>,
    /// A base view's direction.
    #[serde(deserialize_with = "super::present")]
    #[schemars(with = "ViewParam")]
    pub view: Option<ViewParam>,
    #[serde(deserialize_with = "super::present")]
    #[schemars(with = "PlaceParam")]
    pub place: Option<PlaceParam>,
    /// A projected view's display settings (styles, reference parts, tangent edges, line group) all following
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
    /// No view of the chain sets it: the view's sheet does.
    Sheet,
    /// No view of the chain nor its sheet sets it: the drawing does.
    Drawing,
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
    pub visual_style: SettingResult<String>,
    pub reference_style: SettingResult<String>,
    pub reference_hides_model: SettingResult<bool>,
    pub tangent_edges: SettingResult<String>,
    /// The ISO 128 line group it is drawn in, by its wide line.
    pub line_group: SettingResult<String>,
    /// The occurrence subtrees it draws: every body when null.
    pub occurrences: SettingResult<Option<Vec<Vec<u64>>>>,
    /// Occurrence paths it leaves out, with everything under them: none when null.
    pub hide: SettingResult<Option<Vec<Vec<u64>>>>,
}

/// The values a window offers for a view's settings.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ChoicesResult {
    pub visual_style: Vec<String>,
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
    pub reference_shown_percent: f64,
    /// Where it sits on its sheet: its extents grown by `reference_shown_percent`, at its scale,
    /// with the model's middle on its centre; a placeholder 20 mm each way when there's nothing to measure.
    pub boundary_mm: BoxResult,
    /// The extents on its plane of what it shows.
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
    pub visual_style: Option<StyleParam>,
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "StyleParam")]
    pub reference_style: Option<StyleParam>,
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub reference_hides_model: Option<bool>,
    /// How far the view's boundary grows from the model's box towards the box that also holds its
    /// reference parts, each side on its own: 0 to 100 (0 when not given).
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub reference_shown_percent: Option<f64>,
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "TangentEdgesParam")]
    pub tangent_edges: Option<TangentEdgesParam>,
    /// The occurrence subtrees it draws (every body the design places when not given).
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SourceParam")]
    pub source: Option<SourceParam>,
    /// Occurrence paths it leaves out, with everything under them (none when not given).
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<Vec<u64>>")]
    pub hide: Option<Vec<Vec<u64>>>,
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
    /// `display_mesh`; with a placeholder, why: `past_walk_limit`, `no_bodies`, `not_placed` or
    /// `nothing_chosen`.
    pub provenance: &'static str,
}

/// A view's picture: the pixel grid of its boundary at the sheet's dpi, rounded outward.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PictureResult {
    pub pixels: [u32; 2],
    pub dpi: f64,
    pub rect_mm: BoxResult,
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

impl From<solidcraft_geom::hlr::Source> for LineSourceResult {
    fn from(source: solidcraft_geom::hlr::Source) -> Self {
        match source {
            solidcraft_geom::hlr::Source::Edge { edge, tangent } => LineSourceResult::Edge { edge, tangent },
            solidcraft_geom::hlr::Source::Silhouette { face } => LineSourceResult::Silhouette { face },
        }
    }
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
/// strokes, and fills, pictures, text and its fonts too when it is a `whole_sheet`'s.
pub(super) fn paper_scene_schema(generator: &mut SchemaGenerator, kinds: &[ObjectKind], whole_sheet: bool) -> Schema {
    let point = json!({"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2});
    let segment = generator.subschema_for::<PathSegResult>();
    // A dashed stroke's pattern, as `solidcraft_geom::paper::Dashes` checks it; a solid one has none.
    let dashes = json!({
        "type": "object",
        "properties": {
            "lengths_mm": {"type": "array", "items": {"type": "number", "exclusiveMinimum": 0, "maximum": MAX_DASH_MM}, "minItems": 2, "maxItems": 12},
            "phase_mm": {"type": "number", "minimum": 0}
        },
        "required": ["lengths_mm", "phase_mm"],
        "additionalProperties": false
    });
    let stroke = json!({
        "type": "object",
        "properties": {
            "kind": {"const": "stroke"},
            "path": {"type": "array", "items": segment},
            "closed": {"type": "boolean"},
            "width_mm": {"type": "number", "exclusiveMinimum": 0},
            "pattern": dashes
        },
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
    let rect = json!({
        "type": "object",
        "properties": {"min": point, "max": point},
        "required": ["min", "max"],
        "additionalProperties": false
    });
    let picture = json!({
        "type": "object",
        "properties": {"kind": {"const": "picture"}, "source": {"type": "integer", "minimum": 0}, "rect": rect, "opacity": {"type": "number"}},
        "required": ["kind", "source", "rect", "opacity"],
        "additionalProperties": false
    });
    let items = if whole_sheet { json!({"anyOf": [stroke, fill, picture, text_item_schema(point.clone())]}) } else { stroke };
    let kinds: Vec<Value> = kinds.iter().filter_map(|kind| serde_json::to_value(kind).ok()).collect();
    let group = json!({
        "type": "object",
        "properties": {"object": {"type": "integer", "minimum": 0}, "kind": {"enum": kinds}, "items": {"type": "array", "items": items}},
        "required": ["object", "kind", "items"],
        "additionalProperties": false
    });
    let mut properties = json!({"size_mm": point, "groups": {"type": "array", "items": group}});
    if whole_sheet {
        properties["fonts"] = json!({"type": "array", "items": font_schema(ring)});
    }
    json_schema!({
        "type": "object",
        "properties": properties,
        "required": ["size_mm", "groups"],
        "additionalProperties": false
    })
}

/// A font's id: a hash of its face.
fn font_id_schema() -> Value {
    json!({"type": "array", "items": {"type": "integer", "minimum": 0, "maximum": 255}, "minItems": 32, "maxItems": 32})
}

/// One run of text in a paper scene.
fn text_item_schema(point: Value) -> Value {
    let glyph = json!({
        "type": "object",
        "properties": {"id": {"type": "integer", "minimum": 0}, "x": {"type": "number"}, "rise": {"type": "number"}, "advance": {"type": "number"}},
        "required": ["id", "x", "rise", "advance"],
        "additionalProperties": false
    });
    let range = json!({
        "type": "object",
        "properties": {"start": {"type": "integer", "minimum": 0}, "end": {"type": "integer", "minimum": 0}},
        "required": ["start", "end"],
        "additionalProperties": false
    });
    let cluster = json!({
        "type": "object",
        "properties": {"glyphs": range, "text": range},
        "required": ["glyphs", "text"],
        "additionalProperties": false
    });
    json!({
        "type": "object",
        "properties": {
            "kind": {"const": "text"}, "font": font_id_schema(), "scale": {"type": "number"}, "origin": point, "angle": {"type": "number"},
            "slant": {"type": "number"}, "glyphs": {"type": "array", "items": glyph}, "text": {"type": "string"},
            "clusters": {"type": "array", "items": cluster}, "rtl": {"type": "boolean"}
        },
        "required": ["kind", "font", "scale", "origin", "angle", "slant", "glyphs", "text", "clusters", "rtl"],
        "additionalProperties": false
    })
}

/// One font a paper scene's text uses: never its bytes.
fn font_schema(ring: Value) -> Value {
    let int = json!({"type": "integer"});
    let flags = json!({
        "type": "object",
        "properties": {"fixed_pitch": {"type": "boolean"}, "serif": {"type": "boolean"}, "italic": {"type": "boolean"}, "symbolic": {"type": "boolean"}},
        "required": ["fixed_pitch", "serif", "italic", "symbolic"],
        "additionalProperties": false
    });
    let metrics = json!({
        "type": "object",
        "properties": {
            "ascent": int, "descent": int, "cap_height": int, "bbox": {"type": "array", "items": int, "minItems": 4, "maxItems": 4},
            "italic_angle": {"type": "number"}, "flags": flags, "stem_v": int
        },
        "required": ["ascent", "descent", "cap_height", "bbox", "italic_angle", "flags", "stem_v"],
        "additionalProperties": false
    });
    let glyph = json!({
        "type": "object",
        "properties": {"advance": int, "outline": {"type": "array", "items": ring}},
        "required": ["advance", "outline"],
        "additionalProperties": false
    });
    json!({
        "type": "object",
        "properties": {
            "id": font_id_schema(), "face": int, "postscript_name": {"type": "string"}, "family": {"type": "string"},
            "embed": {"enum": ["subset", "whole", "outlines"]}, "num_glyphs": int, "units_per_em": int, "metrics": metrics,
            "glyphs": {"type": "object", "additionalProperties": glyph}
        },
        "required": ["id", "face", "postscript_name", "family", "embed", "num_glyphs", "units_per_em", "metrics", "glyphs"],
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
    pub visual_style: String,
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
    /// The view's picture, when something in it is shaded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture: Option<PictureResult>,
    /// What `measure` asked for, in its order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub measurements: Vec<MeasurementResult>,
    pub source: SourceResult,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A value read on a view.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MeasurementResult {
    /// `linear`, `aligned`, `angle` or `radius`.
    pub kind: &'static str,
    /// A length, in model millimetres.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_mm: Option<f64>,
    /// That length on paper, at the view's scale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paper_mm: Option<f64>,
    /// An angle, in radians.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_rad: Option<f64>,
    pub label: String,
    /// Always `projected`: as the view shows it.
    pub meaning: &'static str,
    /// A measured line isn't parallel to the view's plane, so the model's value differs.
    pub foreshortened: bool,
    /// `exact`, or `approximate` for an edge found again only by its pieces.
    pub status: &'static str,
}

#[cfg(test)]
#[path = "drawing_view_tests.rs"]
mod tests;
