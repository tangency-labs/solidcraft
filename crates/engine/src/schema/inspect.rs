//! Contracts and result types for the commands in `cmd/inspect.rs`.
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with the handler's message. Whether a body, a sketch or a material exists
//! stays the handler's to check. A key the handler refuses when it is left out (`material.assign`'s
//! `bodies` and `material`) is optional in the type and required in the schema, so that the
//! handler's message, not the check's "is required", is what a caller sees (#12498).

use std::borrow::Cow;
use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value, json};

use super::check::{located, whole};
use super::materials::MaterialReply;
use crate::params;

/// Which command to describe.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct DescribeParams {
    /// The command's id, such as `solid.extrude`.
    #[serde(deserialize_with = "command_id")]
    pub command: String,
}

/// A command id, with the `engine.describe` handler's message for anything else.
fn command_id<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(s),
        _ => Err(serde::de::Error::custom("must be a command id")),
    }
}

/// What `engine.describe` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DescribeResult {
    pub id: String,
    pub label: String,
    /// The command's parameter documentation, as prose.
    pub params: String,
    /// The parameters' JSON Schema, or `null` when the command has no contract yet.
    pub input: Option<Value>,
    /// The successful result's JSON Schema, or `null` when not described yet.
    pub output: Option<Value>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// List a body's edges or faces, with what picks each one.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct BodyListParams {
    /// The body, by name.
    pub body: String,
}

/// List the design's cosmetic threads.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ModelThreadsParams {}

/// List the timeline features that built each body (#11945).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct BodyFeaturesParams {
    /// One body, by name (a placed copy's name means the body it shows); every body when left out.
    #[serde(default)]
    pub body: Option<String>,
}

/// What `model.body_features` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ModelBodyFeatures {
    /// The bodies, in the model's order (the one asked for alone).
    pub bodies: Vec<BodyFeatures>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A body and the features that built it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BodyFeatures {
    /// The body's name.
    pub body: String,
    /// In timeline order: the feature that created it, every later one that changed its
    /// geometry, and those of the bodies consumed into it (a Combine's tools, a body joined in).
    pub features: Vec<BodyFeature>,
}

/// A timeline feature.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BodyFeature {
    /// The feature's id.
    pub id: u64,
    /// Its name in the timeline.
    pub name: String,
}

/// What `model.edges` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ModelEdges {
    /// The body's name.
    pub body: String,
    /// Its edges, in the kernel's order.
    pub edges: Vec<ModelEdge>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One edge of a body.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ModelEdge {
    /// Its index, which changes when the body is rebuilt differently.
    pub index: usize,
    /// Its name, `<face A>|<face B>` from the faces it joins (numbered when repeated); `null`
    /// when it has none.
    pub name: Option<String>,
    /// The point halfway along it, which picks it.
    pub mid: [f64; 3],
    /// Its length (mm).
    pub length: f64,
    /// Where it starts; `null` for an edge with no points.
    pub start: Option<[f64; 3]>,
    /// Where it ends; `null` for an edge with no points.
    pub end: Option<[f64; 3]>,
}

/// What `model.faces` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ModelFaces {
    /// The body's name.
    pub body: String,
    /// Its faces, in the kernel's order.
    pub faces: Vec<ModelFace>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One face of a body.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ModelFace {
    /// Its index, which changes when the body is rebuilt differently.
    pub index: usize,
    /// Its name: what `face.delete`, `sheet.convert`'s `face`, `solid.shell` and others take in
    /// place of a point. `null` when it has none.
    pub name: Option<String>,
    /// Whether the name follows the face's exact history; `false` when it was worked out from
    /// the geometry, which an edit earlier in the timeline can fool.
    pub exact: bool,
    /// Its area (mm²).
    pub area: f64,
    /// Its centroid, which isn't always on the face.
    pub centroid: [f64; 3],
    /// Its outward unit normal if it is planar, else `null`.
    pub plane_normal: Option<[f64; 3]>,
    /// A point on the face that picks it; `null` when none was found.
    pub point: Option<[f64; 3]>,
    /// For a cylindrical face: its axis and radius.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cylinder: Option<FaceCylinder>,
    /// For a folded sheet's face: `sheet:<body>:<panel>:<side>`, where the panel is `base` or a
    /// flange's tag `F<feature>.<k>` and the side is `top`, `bottom`, `rim.<edge>`, `bend.inner`
    /// or `bend.outer`. It is taken wherever a face name is, and holds while sizes change and
    /// flanges come and go.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sheet_face: Option<String>,
}

/// A cylindrical face's axis and radius.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FaceCylinder {
    /// A point on the axis.
    pub origin: [f64; 3],
    /// The axis direction, a unit vector.
    pub axis: [f64; 3],
    /// The radius (mm).
    pub radius: f64,
}

/// What `model.threads` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ModelThreads {
    /// The cosmetic threads, in timeline order.
    pub threads: Vec<ModelThread>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A cosmetic thread: on which cylinder, its size, and the threaded part of the axis.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ModelThread {
    /// The id of the thread feature.
    pub feature: u64,
    /// The body it is on.
    pub body: String,
    /// Its size, such as `M6x1`.
    pub designation: String,
    /// Its major diameter (mm).
    pub major_diameter: f64,
    /// The radius of the cylindrical face it is on (mm).
    pub radius: f64,
    /// Its pitch (mm).
    pub pitch: f64,
    /// Whether it is cut into a hole rather than on a shaft.
    pub internal: bool,
    /// Its form.
    pub form: ThreadForm,
    /// Which way it turns.
    pub hand: ThreadHand,
    /// How many starts it has.
    pub starts: u32,
    /// A point on the axis, which `start` and `end` are measured from.
    pub axis_point: [f64; 3],
    /// The axis direction, a unit vector.
    pub axis: [f64; 3],
    /// Where the threaded part starts along the axis from `axis_point` (mm).
    pub start: f64,
    /// Where it ends (mm).
    pub end: f64,
}

/// Which way a thread turns.
#[derive(Clone, Copy, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ThreadHand {
    Right,
    Left,
    /// A right- and a left-handed groove crossing on one face (a reversing screw).
    Both,
}

/// A thread form: ISO metric 60°, metric trapezoidal 30°, Acme 29°, metric buttress 3°/30°,
/// buttress inch 7°/45°, square, and the 55° and 60° pipe threads.
#[derive(Clone, Copy, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ThreadForm {
    Metric,
    Trapezoidal,
    Acme,
    Buttress,
    ButtressAsme,
    Square,
    Bspp,
    Rp,
    Bspt,
    Npt,
}

impl From<&solidcraft_doc::ThreadInfo> for ModelThread {
    fn from(t: &solidcraft_doc::ThreadInfo) -> Self {
        use solidcraft_doc::thread_forms::{ThreadForm as Form, ThreadHand as Hand};
        ModelThread {
            feature: t.feature,
            body: t.body.clone(),
            designation: t.designation.clone(),
            major_diameter: t.major_diameter,
            radius: t.radius,
            pitch: t.pitch,
            internal: t.internal,
            form: match t.form {
                Form::Metric => ThreadForm::Metric,
                Form::Trapezoidal => ThreadForm::Trapezoidal,
                Form::Acme => ThreadForm::Acme,
                Form::Buttress => ThreadForm::Buttress,
                Form::ButtressAsme => ThreadForm::ButtressAsme,
                Form::Square => ThreadForm::Square,
                Form::Bspp => ThreadForm::Bspp,
                Form::Rp => ThreadForm::Rp,
                Form::Bspt => ThreadForm::Bspt,
                Form::Npt => ThreadForm::Npt,
            },
            hand: match t.hand {
                Hand::Right => ThreadHand::Right,
                Hand::Left => ThreadHand::Left,
                Hand::Both => ThreadHand::Both,
            },
            starts: t.starts,
            axis_point: t.axis_point.into(),
            axis: solidcore_record::Vec3::from(t.axis).into(),
            start: t.start,
            end: t.end,
        }
    }
}

/// Adds `keys` to the schema's `required` list: for a key the type has optional so that the
/// handler, not the check, refuses it when it's left out.
fn require_keys(schema: &mut Schema, keys: &[&str]) {
    let mut required = match schema.get("required") {
        Some(Value::Array(required)) => required.clone(),
        _ => Vec::new(),
    };
    for key in keys {
        if !required.iter().any(|k| k == key) {
            required.push((*key).into());
        }
    }
    schema.insert("required".into(), Value::Array(required));
}

fn bodies_and_material_required(schema: &mut Schema) {
    require_keys(schema, &["bodies", "material"]);
}

/// The message of a handler's parameter error, for a reader the contract shares.
fn message(e: crate::EngineError) -> String {
    match e {
        crate::EngineError::BadParams { msg, .. } => msg,
        other => other.to_string(),
    }
}

/// `bodies`: a name or a list of names, read by `params::string_list` itself, with its messages
/// (`null` is no names).
fn body_names<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<String>>, D::Error> {
    let v = Value::deserialize(d)?;
    params::string_list("inspect", &json!({ "bodies": v }), "bodies").map(Some).map_err(|e| D::Error::custom(whole(message(e))))
}

fn body_names_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"anyOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}, "maxItems": params::MAX_STRING_LIST}, {"type": "null"}]})
}

/// Read the design: its name, units, properties, parameters, timeline, bodies and sketches, the
/// selection and the undo and redo depth. Read-only.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct DocumentInspectParams {
    /// Measure each body too (volume, area, centre of mass, box, counts, as `inspect.measure`
    /// does), in place of its feature. Default: false.
    #[serde(default)]
    pub measure: Option<bool>,
    /// Give each sketch also its plane and its number of points and constraints. Default: false.
    #[serde(default)]
    pub sketch_counts: Option<bool>,
    /// How much of the timeline to give: true (the default) each feature with its state and
    /// timing; `"summary"` each as its id, name and type, with `error`, `warning`, `suppressed`
    /// and `rolled_back` only when it has them (a reply a large design keeps small); false leaves
    /// `timeline` out.
    #[serde(default)]
    pub timeline: Option<TimelineDetail>,
    /// How much of each body to give: true (the default) as `measure` says; `"names"` only their
    /// names, nothing measured; false leaves `bodies` out.
    #[serde(default)]
    pub bodies: Option<BodiesDetail>,
}

/// `document.inspect`'s `timeline`: the whole timeline, a summary of it, or none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimelineDetail {
    Full,
    Summary,
    Left,
}

const TIMELINE_FORMS: &str = "`timeline` must be true, false or \"summary\"";

impl TimelineDetail {
    /// `timeline` as the handler reads it: absent or `null` is the whole timeline.
    pub fn read(v: Option<&Value>) -> Result<Self, &'static str> {
        match v {
            None | Some(Value::Null) | Some(Value::Bool(true)) => Ok(TimelineDetail::Full),
            Some(Value::Bool(false)) => Ok(TimelineDetail::Left),
            Some(Value::String(s)) if s == "summary" => Ok(TimelineDetail::Summary),
            Some(_) => Err(TIMELINE_FORMS),
        }
    }
}

impl<'de> Deserialize<'de> for TimelineDetail {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        TimelineDetail::read(Some(&v)).map_err(|e| D::Error::custom(whole(e)))
    }
}

impl JsonSchema for TimelineDetail {
    fn schema_name() -> Cow<'static, str> {
        "TimelineDetail".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "true: every feature with its state and timing; \"summary\": id, name and type, with error, warning, suppressed and rolled_back only when set; false: no timeline.",
            "anyOf": [{"type": "boolean"}, {"type": "string", "enum": ["summary"]}]
        })
    }
}

/// `document.inspect`'s `bodies`: each body in full, only the names, or none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodiesDetail {
    Full,
    Names,
    Left,
}

const BODIES_FORMS: &str = "`bodies` must be true, false or \"names\"";

impl BodiesDetail {
    /// `bodies` as the handler reads it: absent or `null` is every body in full.
    pub fn read(v: Option<&Value>) -> Result<Self, &'static str> {
        match v {
            None | Some(Value::Null) | Some(Value::Bool(true)) => Ok(BodiesDetail::Full),
            Some(Value::Bool(false)) => Ok(BodiesDetail::Left),
            Some(Value::String(s)) if s == "names" => Ok(BodiesDetail::Names),
            Some(_) => Err(BODIES_FORMS),
        }
    }
}

impl<'de> Deserialize<'de> for BodiesDetail {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        BodiesDetail::read(Some(&v)).map_err(|e| D::Error::custom(whole(e)))
    }
}

impl JsonSchema for BodiesDetail {
    fn schema_name() -> Cow<'static, str> {
        "BodiesDetail".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "true: each body as `measure` says; \"names\": only the body names, nothing measured; false: no bodies.",
            "anyOf": [{"type": "boolean"}, {"type": "string", "enum": ["names"]}]
        })
    }
}

/// What `document.inspect` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DocumentInspectResult {
    /// The design's name.
    pub name: String,
    /// Its length unit.
    pub units: String,
    /// Its metadata object as saved, other tools' keys included.
    pub metadata: Map<String, Value>,
    /// The file it is saved in; `null` when it has none.
    pub path: Option<String>,
    /// Whether it has changes that are not saved.
    pub dirty: bool,
    /// How many times the session's design changed.
    pub revision: u64,
    /// The sketch being edited, by feature id; `null` when none.
    pub active_sketch: Option<u64>,
    /// The timeline marker: features from this index on are rolled back.
    pub marker: usize,
    /// The design's parameters.
    pub params: Vec<DocumentParam>,
    /// Its features, in timeline order (in short with `timeline: "summary"`); left out with
    /// `timeline: false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeline: Option<Vec<TimelineRow>>,
    /// Its bodies (only their names with `bodies: "names"`); left out with `bodies: false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bodies: Option<DocumentBodies>,
    /// Its sketches.
    pub sketches: Vec<DocumentSketch>,
    /// What is selected, each as the selection commands take it.
    #[schemars(with = "Vec<Map<String, Value>>")]
    pub selection: Value,
    /// How many undo steps there are.
    pub undo: usize,
    /// How many redo steps there are.
    pub redo: usize,
    /// The parts of the origin that are hidden, by name.
    pub hidden_origin: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A parameter of the design.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DocumentParam {
    pub name: String,
    /// What it is defined as.
    pub expression: String,
    /// `mm`, `deg` or empty (no unit).
    pub unit: String,
    /// What the expression comes to; `null` when it doesn't evaluate.
    pub value: Option<f64>,
    /// Whether a dimension or feature made it, not the user.
    pub model: bool,
    pub comment: String,
    /// Why the expression doesn't evaluate; `null` when it does.
    pub error: Option<String>,
}

/// A feature of the timeline.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TimelineEntry {
    pub id: u64,
    pub name: String,
    /// The feature's kind, such as `Extrude`.
    #[serde(rename = "type")]
    pub kind: String,
    pub suppressed: bool,
    /// Whether the timeline marker has rolled it back.
    pub rolled_back: bool,
    /// Why it failed; `null` when it did not.
    pub error: Option<String>,
    pub warning: Option<String>,
    /// How long it took to evaluate (ms); `null` when it has no result.
    pub ms: Option<f64>,
}

/// A feature of `document.inspect`'s timeline: in full, or in short with `timeline: "summary"`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum TimelineRow {
    Full(TimelineEntry),
    Summary(TimelineSummary),
}

/// A feature in short: what it is, and only the states it has.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TimelineSummary {
    pub id: u64,
    pub name: String,
    /// The feature's kind, such as `Extrude`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Why it failed; present only when it did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Present only when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// Present, and true, only when it is suppressed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suppressed: Option<bool>,
    /// Present, and true, only when the timeline marker has rolled it back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rolled_back: Option<bool>,
}

/// `document.inspect`'s bodies: each in full, or only their names with `bodies: "names"`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum DocumentBodies {
    Full(Vec<DocumentBody>),
    Names(Vec<String>),
}

/// A body of the design: measured with `measure`, else with the feature that made it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DocumentBody {
    #[serde(flatten)]
    pub detail: DocumentBodyDetail,
    /// Whether it is shown.
    pub visible: bool,
    /// Present, and true, when the timeline is rolled back and the body is deleted further on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed_later: Option<bool>,
}

/// What a body entry says of the body itself.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum DocumentBodyDetail {
    /// Without `measure`.
    Listed {
        name: String,
        /// The feature that made it, by id.
        feature: u64,
    },
    /// With `measure`.
    Measured(Box<BodyMeasurement>),
}

/// One body measured: its name and the measurement or why there is none.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BodyMeasurement {
    pub name: String,
    #[serde(flatten)]
    pub outcome: BodyOutcome,
}

/// A body's measurement.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum BodyOutcome {
    Measured(BodyFigures),
    Failed(BodyFailure),
}

/// The measurements of a body.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BodyFigures {
    /// Whether it encloses a volume.
    pub closed: bool,
    /// Its volume (mm³); `null` unless it is closed.
    pub volume_mm3: Option<f64>,
    /// Its surface area (mm²).
    pub area_mm2: f64,
    /// Its centre of mass at uniform density; `null` unless it is closed.
    pub center_of_mass: Option<[f64; 3]>,
    pub bbox: BoundingBox,
    /// Faces, with split pieces of one analytic surface counted once (a sheet metal body's in
    /// `inspect.measure`: with its round holes counted as the design shows them).
    pub faces: i64,
    pub edges: i64,
    pub vertices: i64,
    /// The merged faces by kind of surface.
    pub face_types: BTreeMap<String, usize>,
    /// The kernel's own counts, which split closed curves and surfaces.
    pub kernel: KernelCounts,
    /// What volume, area and centre come from: `smlib` (exact) or `mesh` (a mesh body's own
    /// triangles).
    pub measured_by: String,
    /// Anything the measurement says about itself.
    pub notes: Vec<String>,
}

/// Why a body has no measurement.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BodyFailure {
    pub error: String,
    /// `inspect.measure` only, for a sheet metal body: its hole-corrected count, which has
    /// nothing to correct.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub faces: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edges: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertices: Option<i64>,
}

/// An axis-aligned box (mm).
#[derive(Debug, Serialize, JsonSchema)]
pub struct BoundingBox {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

/// A body's topology counts as the kernel has them.
#[derive(Debug, Serialize, JsonSchema)]
pub struct KernelCounts {
    pub faces: usize,
    pub edges: usize,
    pub vertices: usize,
    pub shells: usize,
}

/// A sketch of the design, in short.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DocumentSketch {
    /// Its feature id.
    pub id: u64,
    pub name: String,
    /// Its remaining degrees of freedom (0: fully constrained).
    pub dof: usize,
    pub status: SketchStatus,
    /// How many curves it has.
    pub curves: usize,
    /// How many profiles it closes.
    pub profiles: usize,
    /// Whether it is shown.
    pub visible: bool,
    /// With `sketch_counts`: where it lies.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Map<String, Value>>")]
    pub plane: Option<Value>,
    /// With `sketch_counts`: how many points it has.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<usize>,
    /// With `sketch_counts`: how many constraints it has.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub constraints: Option<usize>,
}

/// Whether a sketch's constraints solved.
#[derive(Clone, Copy, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SketchStatus {
    Solved,
    Failed,
}

impl From<solidcraft_sketch::SolveStatus> for SketchStatus {
    fn from(s: solidcraft_sketch::SolveStatus) -> Self {
        match s {
            solidcraft_sketch::SolveStatus::Solved => SketchStatus::Solved,
            solidcraft_sketch::SolveStatus::Failed => SketchStatus::Failed,
        }
    }
}

/// How `inspect.measure` selects: bodies, or one or two picked items. With `items`, `bodies` and
/// `names_only` are not read, so the check leaves them alone.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MeasureKeys {
    /// The bodies to measure, by name. Default: all. Not read with `items`.
    #[serde(default, deserialize_with = "body_names", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "body_names_schema")]
    pub bodies: Option<Vec<String>>,
    /// Give each body's name and its definition's and measure nothing. Default: false. Not read
    /// with `items`.
    #[serde(default)]
    pub names_only: Option<bool>,
    /// One or two selections to measure, instead of bodies: a face or edge `{type, body, point}`,
    /// `{type, body, index}` (as `model.faces` and `model.edges` list it) or both, or a body,
    /// vertex, sketch curve or profile as `Sel` has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "items_schema")]
    pub items: Option<Vec<Value>>,
}

fn items_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "array", "items": {"type": "object"}, "minItems": 1, "maxItems": 1000})
}

/// `inspect.measure`'s parameters: [`MeasureKeys`], read as the handler reads them: with `items`
/// given (even as `null`) only `items` is read; without it, `bodies` and `names_only`.
#[derive(Debug)]
pub struct MeasureParams(pub MeasureKeys);

impl<'de> Deserialize<'de> for MeasureParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(d)?;
        if let Some(items) = value.get("items") {
            let list = items.as_array().ok_or_else(|| D::Error::custom(whole("`items` must be a list of selections")))?;
            if list.is_empty() || list.len() > 1000 {
                return Err(D::Error::custom(whole("measure one or two items")));
            }
            return Ok(MeasureParams(MeasureKeys { bodies: None, names_only: None, items: Some(list.clone()) }));
        }
        serde_path_to_error::deserialize(&value).map(MeasureParams).map_err(|e| D::Error::custom(located(&e)))
    }
}

impl JsonSchema for MeasureParams {
    fn schema_name() -> Cow<'static, str> {
        "MeasureParams".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        MeasureKeys::json_schema(generator)
    }
}

/// What `inspect.measure` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MeasureResult {
    #[serde(flatten)]
    pub measured: Measured,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// The shapes of `inspect.measure`'s reply.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Measured {
    /// With `items`.
    Items(Box<MeasuredItems>),
    /// With `names_only`.
    Names(BodyNames),
    /// Bodies.
    Bodies(MeasuredBodies),
}

/// Each world body's name and its definition's.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BodyNames {
    pub body_count: usize,
    pub bodies: Vec<BodyName>,
}

/// A body by its name and its definition's.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BodyName {
    pub name: String,
    pub definition: String,
}

/// The bodies measured, with their totals.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MeasuredBodies {
    pub body_count: usize,
    pub bodies: Vec<MeasuredBody>,
    pub total: MeasureTotal,
}

/// A body measured, with its material and mass.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MeasuredBody {
    #[serde(flatten)]
    pub measurement: BodyMeasurement,
    /// Its material's name.
    pub material: String,
    /// The material's density; only for a body with a volume.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub density_g_cm3: Option<f64>,
    /// Its mass (g); only for a body with a volume.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mass_g: Option<f64>,
    /// True for a sheet metal body, whose counts are its own, with round holes as the design
    /// shows them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sheet_metal: Option<bool>,
}

/// The sum over the bodies measured.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MeasureTotal {
    /// How many bodies are left out of the volume and mass because they enclose none.
    pub open_bodies_skipped: usize,
    pub volume_mm3: f64,
    pub area_mm2: f64,
    pub mass_g: f64,
    pub faces: u64,
    pub edges: u64,
    pub vertices: u64,
}

/// One or two picked items measured: each item, then what its shape gives (for one item) or what
/// lies between the two.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct MeasuredItems {
    /// Each item described: its kind, what it is and its own figures.
    pub items: Vec<Map<String, Value>>,
    /// One item that is round: its diameter (mm).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<f64>")]
    pub diameter: Option<Value>,
    /// Its radius (mm).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<f64>")]
    pub radius: Option<Value>,
    /// A full circle's circumference (mm).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<f64>")]
    pub circumference: Option<Value>,
    /// An arc's sweep in degrees; two items: the angle between them.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<f64>")]
    pub angle_deg: Option<Value>,
    /// An arc's length (mm).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<f64>")]
    pub arc_length: Option<Value>,
    /// One round item's centre.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<[f64; 3]>")]
    pub center: Option<Value>,
    /// One slot: its `width`, `center_distance`, `overall_length` and the two `centers`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Map<String, Value>>")]
    pub slot: Option<Value>,
    /// One slot: where its first end is centred; two items: the first item's centre.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub center_from: Option<[f64; 3]>,
    /// One slot: its second end's centre; two items: the second's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub center_to: Option<[f64; 3]>,
    /// Two items: the closest distance between them (mm).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance_mm: Option<f64>,
    /// Two items: where it starts on the first. A coordinate is `null` where it isn't a number: two planes that meet have no
    /// such point to name (their distance is 0).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<[Option<f64>; 3]>")]
    pub from: Option<[f64; 3]>,
    /// Two items: where it ends on the second. A coordinate is `null` where it isn't a number: two planes that meet have no
    /// such point to name (their distance is 0).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<[Option<f64>; 3]>")]
    pub to: Option<[f64; 3]>,
    /// Two items: `to` minus `from`. A coordinate is `null` where it isn't a number: two planes that meet have no
    /// such point to name (their distance is 0).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<[Option<f64>; 3]>")]
    pub delta: Option<[f64; 3]>,
    /// Two items: the closest distance, as `distance_mm`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    /// Two items: the farthest distance (mm).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_from: Option<[f64; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_to: Option<[f64; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_delta: Option<[f64; 3]>,
    /// Two items: the distance between their centres (or from an axis) (mm).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub center_distance: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub center_delta: Option<[f64; 3]>,
    /// Two items: which distance is the one to read, `center` or `min`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub measure: Option<MeasureKind>,
}

/// Which of two items' distances is the one to read.
#[derive(Clone, Copy, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MeasureKind {
    Center,
    Min,
}

/// List every command: its id, label, place on the toolbar, icon, shortcut, parameter prose and
/// whether it can run now.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CommandsParams {}

/// One command of `engine.commands`' list.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CommandEntry {
    /// The command's id, such as `solid.extrude`.
    pub id: String,
    pub label: String,
    /// The toolbar tab it is on; empty when it is on none.
    pub tab: String,
    /// The panel of that tab.
    pub panel: String,
    pub icon: String,
    pub shortcut: Option<String>,
    /// The parameter documentation, as prose (`engine.describe` has the schemas).
    pub params: String,
    /// Whether it can run in the session's state now.
    pub enabled: bool,
    /// Why it can't; `null` when it can.
    pub disabled_reason: Option<String>,
    /// For a command that is planned and not built yet: what is planned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planned: Option<String>,
}

impl From<crate::cmd::CommandInfo> for CommandEntry {
    fn from(c: crate::cmd::CommandInfo) -> Self {
        CommandEntry {
            id: c.id.into(),
            label: c.label.into(),
            tab: c.tab.into(),
            panel: c.panel.into(),
            icon: c.icon.into(),
            shortcut: c.shortcut.map(Into::into),
            params: c.params.into(),
            enabled: c.enabled,
            disabled_reason: c.disabled_reason,
            planned: c.planned.map(Into::into),
        }
    }
}

/// `material.list` takes no parameters.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MaterialListParams {}

/// What `material.list` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MaterialListResult {
    /// The built-in materials, then the design's own.
    pub materials: Vec<ListedMaterial>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A material in the list.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ListedMaterial {
    /// A built-in material.
    BuiltIn {
        name: String,
        /// Its density (g/cm³).
        density_g_cm3: f64,
    },
    /// One this design defines, with `user` true, its look and its physical card when it has
    /// one.
    Design(Box<MaterialReply>),
}

/// A name as `material.assign` reads `material`: text, with the handler's message for anything
/// else, `null` included.
fn material_name<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(Some(s)),
        _ => Err(D::Error::custom(whole("`material` must be a material name"))),
    }
}

/// Give bodies a physical material (a built-in one, or one made with `material.create`), which
/// their mass and density come from. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = bodies_and_material_required)]
pub struct MaterialAssignParams {
    // Optional in the type and required in the schema (`bodies_and_material_required`).
    /// The bodies, by name: one or a list, at least one, each an existing body.
    #[serde(default, deserialize_with = "body_names", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "material_bodies_schema")]
    pub bodies: Option<Vec<String>>,
    /// The material's name (Steel, Aluminum, ABS Plastic, …, or one made with `material.create`;
    /// `material.list` lists them). Letter case and spaces at either end don't matter.
    #[serde(default, deserialize_with = "material_name", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "material_schema")]
    pub material: Option<String>,
}

fn material_bodies_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"anyOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": params::MAX_STRING_LIST}]})
}

fn material_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string"})
}

/// What `material.assign` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MaterialAssignResult {
    /// The bodies that got the material.
    pub bodies: Vec<String>,
    /// The material's name as the design has it.
    pub material: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A sketch by its feature id (a number) or its name, as `sketch.inspect` reads `sketch`.
#[derive(Clone, Debug, PartialEq)]
pub enum SketchTarget {
    Id(u64),
    Name(String),
}

impl<'de> Deserialize<'de> for SketchTarget {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Number(n) => n.as_u64().map(SketchTarget::Id),
            Value::String(s) => Some(SketchTarget::Name(s)),
            _ => None,
        }
        .ok_or_else(|| D::Error::custom(whole("`sketch` must be an id or a name")))
    }
}

impl JsonSchema for SketchTarget {
    fn schema_name() -> Cow<'static, str> {
        "SketchTarget".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A sketch's feature id (a number, never a name) or its name.",
            "anyOf": [{"type": "integer", "minimum": 0, "maximum": u64::MAX}, {"type": "string"}]
        })
    }
}

/// Read a sketch: its curves, points, constraints, profiles, links and 3D curves, with what is
/// determined and what is not. Read-only.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SketchInspectParams {
    /// The sketch. Default (also `null`): the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

/// What `sketch.inspect` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SketchInspected {
    /// The sketch's feature id.
    pub id: u64,
    pub name: String,
    /// Where it lies, as saved.
    #[schemars(with = "Map<String, Value>")]
    pub plane: Value,
    pub status: SketchStatus,
    /// Its remaining degrees of freedom (0: fully constrained).
    pub dof: usize,
    pub fully_constrained: bool,
    /// The constraints still violated after a failed solve.
    pub failing: Vec<String>,
    pub curves: Vec<SketchCurve>,
    pub points: Vec<SketchPoint>,
    pub constraints: Vec<SketchConstraint>,
    pub profiles: Vec<SketchProfile>,
    /// Planar closed loops the 3D sketch curves take part in, in planes of their own (#741):
    /// each has `index`, `area_mm2`, `plane` (origin and unit normal), `centroid`, `curves`, `holes`.
    #[schemars(with = "Vec<Map<String, Value>>")]
    pub profiles_3d: Vec<Value>,
    /// Links to geometry outside the sketch (projections, intersections, includes), as saved.
    #[schemars(with = "Vec<Map<String, Value>>")]
    pub links: Value,
    /// Its 3D curves.
    pub wires: Vec<SketchWire>,
    /// Whether it is shown.
    pub visible: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A curve of a sketch: its id, what it is and how it is held.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SketchCurve {
    pub id: String,
    #[serde(flatten)]
    pub shape: CurveShape,
    pub construction: bool,
    /// Whether it is locked (fixed, or linked).
    pub fixed: bool,
    /// Whether everything that defines it is determined by constraints.
    pub fully_constrained: bool,
    /// For a linked curve: the link that owns it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

/// A fit spline's tangent handle (#6502).
#[derive(Debug, Serialize, JsonSchema)]
pub struct SplineTangentHandle {
    /// The handle's end point.
    pub point: String,
    pub at: Option<[f64; 2]>,
    /// The construction line from its fit point to the handle's end.
    pub curve: Option<String>,
}

/// A curve's kind and its points, by id and (`_at`) by position `[x, y]` in the sketch's plane.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CurveShape {
    Line {
        start: String,
        end: String,
        start_at: Option<[f64; 2]>,
        end_at: Option<[f64; 2]>,
    },
    Circle {
        center: String,
        center_at: Option<[f64; 2]>,
        radius: f64,
    },
    Arc {
        center: String,
        start: String,
        end: String,
        center_at: Option<[f64; 2]>,
        start_at: Option<[f64; 2]>,
        end_at: Option<[f64; 2]>,
        radius: Option<f64>,
    },
    Ellipse {
        center: String,
        /// The end of the major axis.
        major: String,
        center_at: Option<[f64; 2]>,
        major_at: Option<[f64; 2]>,
        minor_radius: f64,
    },
    Spline {
        points: Vec<String>,
        points_at: Vec<Option<[f64; 2]>>,
        /// Whether `points` are control points of a B-spline, not fit points.
        control: bool,
        degree: u8,
        periodic: bool,
        /// A fit spline's tangent handles: the end of each handle and the construction line to it.
        tangent_handles: Vec<SplineTangentHandle>,
        /// A control spline with its own knots (a projected free-form edge).
        #[serde(skip_serializing_if = "Option::is_none")]
        knots: Option<Vec<f64>>,
        /// Its weights, when it has any.
        #[serde(skip_serializing_if = "Option::is_none")]
        weights: Option<Vec<f64>>,
    },
    Conic {
        start: String,
        end: String,
        apex: String,
        start_at: Option<[f64; 2]>,
        end_at: Option<[f64; 2]>,
        apex_at: Option<[f64; 2]>,
        rho: f64,
    },
}

/// A point of a sketch.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SketchPoint {
    pub id: String,
    /// Its position `[x, y]` in the sketch's plane (mm).
    pub at: [f64; 2],
    /// Where that is in the design (mm).
    pub world: [f64; 3],
    /// Whether it is locked (fixed, or linked).
    pub fixed: bool,
    /// For a linked point: the link that owns it; `null` otherwise.
    pub link: Option<String>,
    pub fully_constrained: bool,
}

/// A constraint or dimension: its id, its kind's name and the kind's own fields (`type`, then
/// the points and curves it ties, a value…), as saved.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SketchConstraint {
    pub id: String,
    /// The kind in short, such as `Horizontal`.
    pub name: String,
    /// For a dimension: the parameter that drives it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
    /// With `param`: the parameter's expression; `null` when the design has no such parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<String>")]
    pub expression: Option<Option<String>>,
    #[serde(flatten)]
    pub kind: Map<String, Value>,
}

/// A region the sketch closes.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SketchProfile {
    pub index: usize,
    pub area_mm2: f64,
    /// Its centroid `[x, y]` in the sketch's plane.
    pub centroid_sketch: [f64; 2],
    /// The curves of its outer loop, each once, in loop order.
    pub curves: Vec<String>,
    /// The curves of each hole's loop.
    pub holes: Vec<Vec<String>>,
}

/// A 3D curve a sketch carries.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SketchWire {
    pub id: String,
    /// How many points its polyline has.
    pub points: usize,
    pub start: Option<[f64; 3]>,
    pub end: Option<[f64; 3]>,
    /// For a linked curve: the link that owns it; `null` otherwise.
    pub link: Option<String>,
    /// For an exact circular arc: its geometry; `null` otherwise.
    pub arc: Option<WireArcInfo>,
}

/// An exact circular arc of a 3D curve.
#[derive(Debug, Serialize, JsonSchema)]
pub struct WireArcInfo {
    pub center: [f64; 3],
    pub radius: f64,
    pub start: [f64; 3],
    pub end: [f64; 3],
    /// Its sweep (degrees).
    pub sweep_deg: f64,
}

#[cfg(test)]
#[path = "inspect_tests.rs"]
mod tests;
