//! Colour rules and data shared by every crate: the sRGB transfer function ([`srgb`]) and the surface analysis palette
//! ([`AnalysisColors`]). No app types and no dependencies, so the renderer, the document and the theme all read the same
//! rule and the same colours.

mod analysis;
pub mod srgb;

pub use analysis::AnalysisColors;
