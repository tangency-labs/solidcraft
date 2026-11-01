//! Reusable egui widgets for SolidCraft and its siblings.
//!
//! The first widget is the signed-axis navigation compass behind
//! [`Navigator::show`]. It depends only on `egui` and `solidcraft-render`, so another
//! egui application can link the widget without linking the SolidCraft front end.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod navigator;

pub use navigator::{
    AXES_SIZE, AXIS_CENTER_LIFT, AXIS_RADIUS, NavAction, Navigator, NavigatorColors, NavigatorOptions, axis_hit, axis_tip, axis_tips,
    nearest_quarter_turn,
};
