//! Input/output contracts for the first sheet-sketch command lifecycle.
use super::TypedContract;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Create {
    /// Exact sheet id in the open drawing, as returned by drawing.inspect.
    pub owner: String,
    pub name: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub sketch: String,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Inspect {
    pub sketch: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}

#[derive(Serialize, JsonSchema)]
pub struct Point {
    pub id: String,
    pub at: [f64; 2],
    pub fixed: bool,
}
#[derive(Serialize, JsonSchema)]
pub struct Reading {
    pub drawing: String,
    pub sheet: String,
    pub sketch: String,
    pub name: String,
    pub editing: bool,
    pub solved: bool,
    pub dof: usize,
    pub points: Vec<Point>,
    pub curves: Vec<String>,
    pub constraints: Vec<String>,
}
#[derive(Serialize, JsonSchema)]
pub struct Cancelled {
    pub drawing: String,
    pub sheet: String,
    pub sketch: String,
    pub removed: bool,
}

pub static CREATE: TypedContract<Create> = TypedContract::new();
pub static EDIT: TypedContract<Edit> = TypedContract::new();
pub static INSPECT: TypedContract<Inspect> = TypedContract::new();
pub static EMPTY: TypedContract<Empty> = TypedContract::new();
