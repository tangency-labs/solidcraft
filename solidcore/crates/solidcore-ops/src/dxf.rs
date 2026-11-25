//! DXF, as an application's reader and writer call it: [`import`] scales what a file holds and
//! checks each read entity, [`export`] converts paths for the file, bounds and places them, and
//! checks each written entity. The maths is the kernel's general `prism::{arc_conversion, bounds,
//! transform}`; this module adds the record conversion and the budgets, and computes nothing
//! (docs/layers.md §1).

pub mod export;
pub mod import;
