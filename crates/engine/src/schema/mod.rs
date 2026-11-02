//! Typed parameter contracts for commands.
//!
//! A converted command declares a [`Contract`]: a Rust type its parameters must deserialize
//! into, from which the input JSON Schema is generated. The contract is checked *in front of*
//! the unchanged handler, which still reads the raw parameters, so a command's behaviour does
//! not change unless the session's [`ParamCheck`] level asks for warnings or refusals.
//!
//! - **Off** (the default): nothing is parsed or checked; the handler runs as before.
//! - **Warn**: the handler runs; a successful object result gains `param_warnings`.
//! - **Strict**: a call with an unknown key or a parameter of the wrong shape is refused, and
//!   nothing runs.
//!
//! Unknown keys are the top-level keys a call sends that the contract's schema does not list.
//! Keys the engine reads around a command count too: `frames::to_local` reads `body` for
//! commands with picks (extrude's `face`), which the extrude contract declares; it otherwise
//! rewrites only keys a call already has. `live` is read only for commands with `.live(…)`, and
//! `timeline.redefine` adds only `convention` and `participants`, which the contracts declare.
//!
//! The shared field types in [`types`] decide acceptance by calling the same helpers the
//! handlers use (`params::vec3`, `params::expr`, `Operation::parse`, `features::profiles`…), so
//! "the contract accepts it" means "the handler's own reader accepts it". Where the handler is
//! more lenient (a value of the wrong type read as if the key were absent), the contract is the
//! stricter of the two; that difference is what warn and strict surface.
//!
//! Contracts live in one module per command file: `features.rs` for `cmd/features.rs`,
//! `inspect.rs` for `cmd/inspect.rs`, and so on, each with its own tests (`features_tests.rs`),
//! so conversions of different command files touch different files (apart from a new
//! module's line below).
//!
//! To convert a command: declare its parameters type in the module for its command file (and
//! its reply type, for an output schema), creating the module if it is the file's first
//! contract, from the shared types in [`types`] (the keys every feature takes come from
//! [`FeatureCommon`], which `solid.box` and `solid.extrude` predate); give its `CommandSpec`
//! `checked!(…)`, `.contract(…)` and `.output(…)` as `solid.extrude` does; and add it to that
//! module's `schemas_and_the_key_check_agree` and, for messages it already gives,
//! `strict_keeps_todays_errors`. Then run the whole test suite with the audit recorder (see
//! `record.rs`): a call that succeeds today but breaks the contract is either a form the
//! contract is missing or a caller's mistake.
//!
//! New commands come with both schemas: `coverage_tests.rs` warns about a command without a
//! contract or an output type (it will fail once `SCHEMA_RULE_STRICT` flips), unless it is a `planned!` placeholder or listed in
//! `unconverted.txt`, which holds the commands that predate the rule. That list only shrinks:
//! converting a command removes its line, and nothing is added to it. How to write a contract,
//! with the shared types and a checklist: docs/book/src/dev/schemas.md.

mod check;
#[cfg_attr(target_arch = "wasm32", path = "record_wasm.rs")]
mod record;
mod types;

#[cfg(test)]
mod coverage_tests;
#[cfg(test)]
mod tests;

// The contracts, by command file.
pub mod component;
pub mod construct;
pub mod edit;
pub mod features;
pub mod file;
pub mod inspect;
pub mod joints;
pub mod mass_properties;
pub mod print_face;
pub mod render;
pub mod render_lights;
pub mod render_ui;
pub mod section;
pub mod sheet;
pub mod sketch_3d;
pub mod sketch_constraints;
pub mod sketch_create;
pub mod sketch_equation;
pub mod surface_analysis;
pub mod ui_prefs;
pub mod view_navigation;

pub use check::{Contract, Issue, ParamCheck, TypedContract, input_schema_for, issues, output_schema_for, run_checked, run_checked_as};
pub use types::*;

/// A command's `run` that checks the call against the contract command `$id` publishes
/// (`.contract(…)` on its `CommandSpec`) before running `$handler` (see [`run_checked_as`]), so
/// the contract enforced is always the one described. Expands to a non-capturing closure, which
/// coerces to the registry's `Run` function pointer.
#[macro_export]
#[doc(hidden)]
macro_rules! checked {
    ($id:literal, $handler:path) => {
        |s: &mut $crate::Session, p: &serde_json::Value| $crate::schema::run_checked_as(s, p, $id, $handler)
    };
}
