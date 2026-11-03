//! Application/workspace layout, deliberately independent of Session and egui.
//! Consumers must use `LayoutService` (or its `ActionSink`) for committed mutations.
//! The serde values are wire data, not proof of validity; see `validate` and `decode`.
pub mod commands;
mod fixtures;
mod geometry;
mod model;
mod persistence;
mod reducer;

pub use fixtures::*;
pub use geometry::*;
pub use model::*;
pub use persistence::*;
pub use reducer::*;

#[cfg(test)]
mod tests;
