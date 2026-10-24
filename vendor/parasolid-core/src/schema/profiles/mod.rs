//! Reviewed compiled profiles, registered by `BuiltinProfileRegistry::compiled`.

pub(crate) mod icad_legacy;
pub(crate) mod icad_standard;
mod onshape_current;
mod sch13006;
mod sch30000;
mod solidworks;
pub use icad_legacy::icad_legacy_13006;
pub use icad_standard::icad_legacy_standard;

pub(crate) use onshape_current::PROFILE_SHA256 as ONSHAPE_CURRENT_PROFILE_SHA256;
pub(crate) use onshape_current::definitions as onshape_current_definitions;
pub use onshape_current::onshape_sch37102_13006;

pub(crate) use solidworks::PROFILE_SHA256 as SOLIDWORKS_PROFILE_SHA256;
pub use solidworks::solidworks_sch37102_13006;

pub(crate) use sch13006::bspline_surface_definitions as sch13006_bspline_surface_definitions;
pub(crate) use sch13006::definitions as sch13006_definitions;
pub(crate) use sch13006::icad_sch30000_13006_definitions;
pub(crate) use sch13006::sp_curve_definitions as sch13006_sp_curve_definitions;
pub use sch13006::{icad_sch30000_13006, onshape_sch13006};
pub use sch30000::onshape_sch30000;

mod icad_v34;
pub(crate) use icad_v34::PROFILE_SHA256 as ICAD_V34_PROFILE_SHA256;
pub(crate) use icad_v34::definitions as icad_v34_definitions;
pub use icad_v34::icad_sch34101_13006;

mod solidworks_35102;
pub use solidworks_35102::solidworks_sch35102_13006;
pub(crate) const SOLIDWORKS_35102_PROFILE_SHA256: &str = "4b31ac661d97c0868b8c94204c12d748ad47e25cbb3983666d22ca2868673129";
