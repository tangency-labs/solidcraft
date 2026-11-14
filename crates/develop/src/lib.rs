//! Sheet development for SolidCraft: solidcore's, re-exported with SolidCraft's wording of its
//! refusals. A sheet metal part's surfaces are the record's
//! (`solidcore_record::develop`, as the saved model keeps them); laying them out flat, folding the
//! flat back and the motions that place panels are solidcore's (`solidcore_ops::develop`, the
//! kernel's `develop` module). Nothing here computes: this crate only names the calls for the
//! sheet features and words a refusal for the user.
//!
//! The record's motions and bends take their maths as methods of extension traits: `use
//! solidcraft_develop::prelude::*` to call `place.region(&r)` or `bend.allowance(t, k)`.
//!
//! [`Query`] answers exact questions of a B-rep record, as reading a sheet off a body asks them:
//! where a point lies on the record's faces and which way they face there, how far it is from an
//! edge, and which faces a ray meets (solidcore's, `solidcore_ops::query`).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub use solidcore_ops::develop::{Bent, Fold, crossings, curve_through};
pub use solidcore_ops::query::Query;
pub use solidcore_record::develop::{
    Anchor, Bend, Corner, CornerRelief, Cut, Datum, Development, EdgeFrom, FlatPanel, Joint, JointKind, Lap, Mismatch, Panel, Relief, Rigid2, Rigid3,
    Rip, Sheet, Surface, Tail, Trim, Zone, ZoneEnds,
};

/// The maths of the record's motions, bends and laps (solidcore's), as methods.
pub mod prelude {
    pub use solidcore_ops::develop::{BendOps as _, LapOps as _, Rigid2Ops as _, Rigid3Ops as _};
}

use solidcore_ops::develop::{JointRefusal, PanelRefusal, Refusal, RuleRefusal};

/// Why a sheet is not laid out or folded, in SolidCraft's words (solidcore's [`Refusal`]).
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum DevelopError {
    #[error("the sheet has no panel {0}")]
    NoPanel(usize),
    #[error("panel {0}: {1}")]
    Panel(usize, &'static str),
    #[error("joint {0}: {1}")]
    Joint(usize, &'static str),
    #[error("the sheet exceeds the development's work limit")]
    TooLarge,
    #[error("the sheet's rule: {0}")]
    Rule(&'static str),
    #[error("the fold takes a place in space for each of the flat's {0} pieces")]
    Roots(usize),
}

impl From<Refusal> for DevelopError {
    fn from(r: Refusal) -> DevelopError {
        match r {
            Refusal::NoPanel(i) => DevelopError::NoPanel(i),
            Refusal::Panel { panel, why } => DevelopError::Panel(panel, panel_words(why)),
            Refusal::Joint { joint, why } => DevelopError::Joint(joint, joint_words(why)),
            Refusal::TooLarge => DevelopError::TooLarge,
            Refusal::Rule(RuleRefusal::Thickness) => DevelopError::Rule("the thickness must be more than 0"),
            Refusal::Rule(RuleRefusal::KFactor) => DevelopError::Rule("the K-factor must be between 0 and 1"),
            Refusal::Roots(n) => DevelopError::Roots(n),
        }
    }
}

fn panel_words(why: PanelRefusal) -> &'static str {
    match why {
        PanelRefusal::RollRadius => "a roll's radius must be more than the sheet's thickness",
        PanelRefusal::ConeHalfAngle => "a cone's rulings make more than 0 and less than a quarter turn with its axis",
        PanelRefusal::ConeApex => "the cone's neutral surface would pass its apex inside the panel",
        PanelRefusal::CutUnplaced => "a cut lies on a panel the flat has no place for",
        PanelRefusal::ConeSpiral => "a line across the cone's panel needs more pieces than a flat takes to follow",
    }
}

fn joint_words(why: JointRefusal) -> &'static str {
    match why {
        JointRefusal::NoDirection => "an anchor has no direction",
        JointRefusal::NotUnit => "an anchor's direction is not a unit vector",
        JointRefusal::OnePanel => "both anchors are on one panel",
        JointRefusal::BendRange => "a bend turns more than 0 and at most a half turn, round a radius of 0 or more",
        JointRefusal::HalfTurnSharp => "a half turn has no virtual sharp: anchor it at its tangent lines",
        JointRefusal::ZoneEnds => "the zone's ends are not numbers",
        JointRefusal::BendOnRoll => "not supported yet: a bend on the edge of a roll",
        JointRefusal::RipPoints => "a rip's edge has two or more points on each panel, and a turn at each",
        JointRefusal::RipNotFinite => "a rip's edge is not numbers",
        JointRefusal::RipLap => "a rip cuts 0 or more from each side",
        JointRefusal::RipOffSurface => "a rip's edge leaves its panel's surface",
        JointRefusal::RipOffOutline => "a rip's edge is not on its panel's outline",
        JointRefusal::RipAgainstOutline => "a rip's edge runs against its panel's outline",
        JointRefusal::TornEnd => "a torn end of the zone strays further from the curve it follows than its tolerance",
        JointRefusal::NoFrame => "an anchor has no frame on its panel",
    }
}

/// Lays the sheet out flat (solidcore's `develop`): `anchors` says where some panels' neutral
/// charts lie in the flat frame.
pub fn develop(sheet: &Sheet, anchors: &[(usize, Rigid2)]) -> Result<Development, DevelopError> {
    solidcore_ops::develop::develop(sheet, anchors).map_err(DevelopError::from)
}

/// Folds a developed sheet (solidcore's `fold`): `roots` says where the first panel of each of
/// the development's pieces lies in space.
pub fn fold(sheet: &Sheet, dev: &Development, roots: &[Rigid3]) -> Result<Fold, DevelopError> {
    solidcore_ops::develop::fold(sheet, dev, roots).map_err(DevelopError::from)
}
