//! Frame infrastructure shared by the UI, the window procedure and the window host.
//!
//! Everything here is pure: no GPU, no windowing, no clock of its own (callers pass time in), so
//! it builds for every target including wasm and is tested deterministically.
//!
//! - [`caption`]: the title bar's regions in window pixels, published by the UI thread and read
//!   lock-free by the window procedure's hit test.

pub mod caption;

pub use caption::{CaptionHit, CaptionMap, PxRect};
