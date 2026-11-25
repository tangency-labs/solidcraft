//! 3D mouse input that doesn't depend on how the device is read or what the motion drives. The
//! apps open the device their own way (hidraw on Linux, hidapi on Windows), pass its report
//! descriptor to [`Descriptor::parse`] and each input report to [`Layout::decode`], and apply the
//! six axes and the buttons they get back to their own camera and commands.
//!
//! Any device that declares a HID Multi-axis Controller is read the same way: 3Dconnexion's,
//! which send translation and rotation in two reports and their buttons in a third, and homemade
//! ones, which often send all six axes in one report, sometimes after their buttons. The
//! descriptor says where each axis and button is.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod button_set;
pub mod buttons;
mod calibration;
mod descriptor;

pub use button_set::ButtonSet;
pub use calibration::AxisCalibration;
pub use calibration::Calibration;
pub use calibration::FEATHER_SHARE;
pub use calibration::HOLD_SECONDS;
pub use calibration::Kind;
pub use calibration::MAX_DEAD_ZONE_SHARE;
pub use calibration::MIN_SHARE;
pub use calibration::NO_SOURCE;
pub use calibration::Problem;
pub use calibration::REST_SECONDS;
pub use calibration::RESTARTS_FOR_TRACKBALL;
pub use calibration::Recording;
pub use calibration::SETTLE_AFTER_SECONDS;
pub use calibration::SETTLE_SHARE;
pub use calibration::SETTLE_WANDER;
pub use calibration::STEPS;
pub use calibration::Settle;
pub use calibration::Stage;
pub use calibration::Step;
pub use calibration::Wizard;
pub use descriptor::Descriptor;
pub use descriptor::FULL_SCALE;
pub use descriptor::Field;
pub use descriptor::Layout;
pub use descriptor::MAX_BUTTONS;
pub use descriptor::Report;
