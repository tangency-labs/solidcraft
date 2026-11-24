//! Application/workspace layout, deliberately independent of Session and egui.
//! Consumers must use `LayoutService` (or its `ActionSink`) for committed mutations.
//! The serde values are wire data, not proof of validity; see `validate` and `decode`.
pub mod commands;
mod fixtures;

pub use fixtures::*;
// The placement core is app-neutral and lives in solidcraft-panel-layout; the engine re-exports it.
pub use solidcraft_panel_layout::*;

#[cfg(test)]
mod tests;
