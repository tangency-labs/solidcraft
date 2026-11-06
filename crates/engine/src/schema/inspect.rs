//! Contracts and result types for the commands in `cmd/inspect.rs`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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

#[cfg(test)]
#[path = "inspect_tests.rs"]
mod tests;
