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
/// The existing model Cancel reply keeps its numeric feature id.
#[derive(Serialize, JsonSchema)]
pub struct ModelCancelled {
    pub sketch: u64,
    pub removed: bool,
}

/// The sheet Cancel reply names its exact drawing, sheet and sketch by encoded ids.
#[derive(Serialize, JsonSchema)]
pub struct Cancelled {
    pub drawing: String,
    pub sheet: String,
    pub sketch: String,
    pub removed: bool,
}

/// The one Cancel command reports the active receiver's existing reply, without
/// converting model feature ids into drawing object ids or adding a wire tag.
#[derive(Serialize, JsonSchema)]
#[serde(untagged)]
pub enum CancelResult {
    Model(ModelCancelled),
    Sheet(Cancelled),
}

pub static CREATE: TypedContract<Create> = TypedContract::new();
pub static EDIT: TypedContract<Edit> = TypedContract::new();
pub static INSPECT: TypedContract<Inspect> = TypedContract::new();
pub static EMPTY: TypedContract<Empty> = TypedContract::new();

/// Cancel authors no parameters. As with other converted model commands, Off
/// keeps the legacy handler's ignored-input behavior; Warn/Strict check this shape.
pub static CANCEL: TypedContract<Empty> = TypedContract::new();

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{CancelResult, Empty};
    use crate::Session;
    use crate::schema::tests::{assert_keys, conforms};
    use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

    fn run(session: &mut Session, command: &str, params: Value) -> Value {
        session.execute(command, &params).unwrap_or_else(|error| panic!("{command}: {error}"))
    }

    #[test]
    fn cancel_authors_no_parameter_keys() {
        assert_keys(input_schema_for::<Empty>(), &[]);
    }

    #[test]
    fn model_and_sheet_cancel_replies_match_the_shared_schema_off_and_strict() {
        let schema = output_schema_for::<CancelResult>();
        let schema = schema.as_value();
        for level in [ParamCheck::Off, ParamCheck::Strict] {
            let mut model = Session { param_check: level, ..Session::default() };
            let made = run(&mut model, "sketch.create", json!({"plane":"XY"}));
            let reply = run(&mut model, "sketch.cancel", json!({}));
            assert_eq!(reply["sketch"], made["sketch"]);
            assert!(reply["sketch"].is_u64() && reply["removed"] == true, "{reply}");
            assert!(conforms(schema, schema, &reply), "{level:?}: {reply} does not match {schema}");

            let mut sheet = Session { param_check: level, ..Session::default() };
            run(&mut sheet, "sketch.create", json!({"plane":"XY"}));
            run(&mut sheet, "sketch.finish", json!({}));
            let drawing = run(&mut sheet, "drawing.create", json!({}));
            let made = run(&mut sheet, "drawing.sketch.create", json!({"owner":drawing["sheets"][0]["id"]}));
            let reply = run(&mut sheet, "sketch.cancel", json!({}));
            assert_eq!(reply["drawing"], drawing["id"]);
            assert_eq!(reply["sheet"], made["sheet"]);
            assert_eq!(reply["sketch"], made["sketch"]);
            assert!(reply["sketch"].is_string() && reply["removed"] == true, "{reply}");
            assert!(conforms(schema, schema, &reply), "{level:?}: {reply} does not match {schema}");
        }
    }

    #[test]
    fn canonical_model_cancel_keeps_off_compatibility_and_strict_refusal_is_atomic() {
        let mut off = Session::default();
        run(&mut off, "sketch.create", json!({"plane":"XY"}));
        assert_eq!(run(&mut off, "sketch.cancel", json!({"legacy_ignored":true}))["removed"], true);

        let mut strict = Session { param_check: ParamCheck::Strict, ..Session::default() };
        run(&mut strict, "sketch.create", json!({"plane":"XY"}));
        let before = strict.doc.clone();
        let editing = strict.active_sketch;
        let history = (strict.undo.len(), strict.redo.len());
        assert!(strict.execute("sketch.cancel", &json!({"legacy_ignored":true})).is_err());
        assert!(std::sync::Arc::ptr_eq(&strict.doc, &before));
        assert_eq!(strict.active_sketch, editing);
        assert_eq!((strict.undo.len(), strict.redo.len()), history);
        assert_eq!(run(&mut strict, "sketch.cancel", json!({}))["removed"], true);
    }
}
