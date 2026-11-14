//! Developing a sheet metal part from its surfaces into its flat pattern, and folding the flat
//! back onto the part.
//!
//! A [`Sheet`] is a sheet metal part's surfaces, kept as the master: its panels, each drawn in its
//! own chart (a map of the panel's surface onto the plane that keeps lengths and angles), and the
//! joints where panels meet. A bend joint is anchored where the two panels' surfaces would meet
//! (the virtual sharp), so a bend's radius, the thickness and the K-factor stay values of the
//! bend: nothing in the panels depends on them. A hem, which has no sharp, is anchored at its
//! tangent lines. A roll is a panel of its own, on a cylinder or a cone, whose radius is its
//! geometry; a seam joins it to its neighbours, and a rip cuts panels apart. Every panel lies on a
//! plane, a right circular cylinder or a right circular cone, whose flat has a closed form
//! (ε = 0); a free-form surface's has none, and is no panel's: [`Surface`] cannot name one.
//!
//! [`develop`] lays the sheet out flat. Each panel goes onto its neutral surface first (for a
//! planar panel, its own shape; for a roll, longer or shorter round it). One panel's chart is
//! the flat frame; every panel a chain of bends and seams reaches from it is turned and shifted
//! so that each bend's zone, as wide as its neutral arc is long (`θ (R + K T)`), fits between its
//! two panels and each seam's panels meet edge to edge. A rigid motion maps every kind of profile
//! segment to one of its own kind, so lines, arcs, conics and splines come out as the panels have
//! them. The pieces (each panel less its trims, and the zones) are joined by the caller's planar
//! booleans.
//!
//! [`fold`] puts the developed sheet back in space: each piece where the caller says it lies, and
//! each of its panels where its bends and seams put it, so that every point of the flat pattern,
//! at any depth through the sheet, has its place on the folded sheet.
//!
//! What is exact and what is not:
//! - A plane's flat is its own chart, a cylinder's an affine map of its chart (a line stays a
//!   line, an arc becomes exact elliptical conics, every Bézier and conic keeps its kind), a cone's
//!   polar: a ruling stays a line and a circle round the axis becomes an arc about the apex. A
//!   bend's zone is its neutral arc's length wide, and its fold and unfold are inverses
//!   ([`Bent::point`], [`Bent::unwrapped`]). A surface's first fundamental form is its closed form.
//! - Followed within a stated distance, not exactly: a line across both of a cone's parameters (a
//!   spiral in the flat) by cubic pieces sized from the Hermite remainder's bound, within 1e-4 mm;
//!   any other curve on a cone by chords; a rip's edge given only as points by a cubic spline
//!   through them; a torn end of a zone by a cubic spline checked against its curve at samples.
//!   None of these is a certificate.
//!
//! The data has a record twin (`solidcore-record`'s `develop`), copied field by field by
//! `solidcore-convert`; `solidcore-ops`' `develop` is the call applications make.
//!
//! Not upstream: SolidCraft's sheet development (`crates/develop`, MIT OR Apache-2.0), moved into
//! the kernel so that the application computes no geometry (docs/layers.md §1).
//!
//! | Rust                         | SolidCraft (`crates/develop`)        |
//! |------------------------------|--------------------------------------|
//! | [`develop`]                  | `develop::develop`                   |
//! | [`fold`]                     | `fold::fold`                         |
//! | [`crossings`]                | `develop::crossings`                 |
//! | [`curve_through`]            | `develop::curve_through`             |
//! | [`Refusal`]                  | `DevelopError` (typed: no strings)   |
//!
//! Deviations: none (no upstream counterpart). No recursion: layouts and folds walk the joints
//! breadth first, each joint once; root isolation, span crossings and refinements run on explicit
//! stacks with fixed budgets. Work limits: [`MAX_PANELS`], [`MAX_JOINTS`].

// A comparison that is not true takes the refusing branch, so a NaN is refused too.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

mod chart;
mod flat;
mod fold;
mod layout;
pub(crate) mod plane;
mod refusal;
mod rigid;
mod seg;
mod sheet;

pub use flat::{Development, EdgeFrom, FlatPanel, Mismatch, Trim, Zone};
pub use fold::{fold, Bent, Fold};
pub use layout::{crossings, curve_through, develop};
pub use refusal::{JointRefusal, PanelRefusal, Refusal, RuleRefusal};
pub use rigid::{Rigid2, Rigid3};
pub use sheet::{
    Anchor, Bend, Corner, CornerRelief, Cut, Datum, Joint, JointKind, Lap, Panel, Relief, Rip,
    Sheet, Surface, Tail, ZoneEnds,
};

/// The most panels one development takes: a work limit, never a truncation.
pub const MAX_PANELS: usize = 10_000;
/// The most joints one development takes: a work limit, never a truncation.
pub const MAX_JOINTS: usize = 20_000;

#[cfg(test)]
mod tests;
