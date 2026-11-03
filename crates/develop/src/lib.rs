//! Developing a sheet metal part from its surfaces into its flat pattern.
//!
//! A [`Sheet`] is a sheet metal part's surfaces, kept as the master: its panels, each drawn in its
//! own chart (a map of the panel's surface onto the plane that keeps lengths and angles), and the
//! joints where panels meet. A bend joint is anchored where the two panels' surfaces would meet
//! (the virtual sharp), so a bend's radius, the thickness and the K-factor stay values of the
//! bend: nothing in the panels depends on them. A hem, which has no sharp, is anchored at its
//! tangent lines. A roll is a panel of its own, on a cylinder or a cone, whose radius is its
//! geometry; a seam joins it to its neighbours, and a rip cuts panels apart.
//!
//! [`develop`] lays the sheet out flat. Each panel goes onto its neutral surface first (for a
//! planar panel, its own shape; for a roll, longer or shorter round it). One panel's chart is
//! the flat frame; every panel a chain of bends and seams reaches from it is turned and shifted
//! so that each bend's zone, as wide as its neutral arc is long, fits between its two panels and
//! each seam's panels meet edge to edge. A rigid motion maps every kind of profile segment to
//! one of its own kind, so lines, arcs, conics and splines come out as the panels have them. The
//! pieces (each panel less its trims, and the zones) are joined by the caller's planar booleans.
//!
//! [`fold`] puts the developed sheet back in space: each piece where the caller says it lies, and
//! each of its panels where its bends and seams put it, so that every point of the flat pattern,
//! at any depth through the sheet, has its place on the folded sheet.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod chart;
mod develop;
mod fitted;
mod fold;
mod rigid;
mod sheet;

pub use develop::{DevelopError, Development, EdgeFrom, FlatPanel, Mismatch, Trim, Zone, crossings, curve_through, develop};
pub use fold::{Bent, Fold, fold};
pub use rigid::{Rigid2, Rigid3};
pub use sheet::{Anchor, Bend, Corner, CornerRelief, Cut, Datum, Joint, JointKind, Lap, Panel, Relief, Rip, Sheet, Surface, Tail, ZoneEnds};

#[cfg(test)]
mod tests;
