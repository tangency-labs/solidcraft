//! Why a sheet is not laid out or folded, as data: the panel or joint it is about and the cause.
//! An application words these for its users.
//!
//! Not upstream (SolidCraft's sheet development, moved into the kernel, docs/layers.md §1).
//!
//! Deviations: none (no upstream counterpart). No recursion.

/// Why a sheet is not laid out ([`super::develop`]) or folded ([`super::fold`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// An anchor, a cut or the call names a panel the sheet does not have.
    NoPanel(usize),
    /// A panel the development cannot lay out, and why.
    Panel {
        /// The panel.
        panel: usize,
        /// Why.
        why: PanelRefusal,
    },
    /// A joint the development cannot lay out, and why.
    Joint {
        /// The joint.
        joint: usize,
        /// Why.
        why: JointRefusal,
    },
    /// More panels or joints than one development takes ([`super::MAX_PANELS`],
    /// [`super::MAX_JOINTS`]): a work limit, never a truncation.
    TooLarge,
    /// The sheet's thickness or K-factor, and which.
    Rule(RuleRefusal),
    /// The fold takes one place in space for each of the flat's pieces: how many it has.
    Roots(usize),
}

/// Why a panel is not laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelRefusal {
    /// A roll whose radius is 0 or less, or whose neutral surface would be (the material
    /// inward, the sheet thicker than the roll's radius allows).
    RollRadius,
    /// A cone whose rulings make 0 or less, or a quarter turn or more, with its axis.
    ConeHalfAngle,
    /// A cone whose neutral surface would pass its apex inside the panel.
    ConeApex,
    /// A cut drawn on a panel the flat has no place for.
    CutUnplaced,
    /// A line across both of a cone's parameters that more cubic pieces than a flat takes would
    /// be needed to follow within its distance (a panel far past any sheet's size): refused
    /// rather than followed less closely.
    ConeSpiral,
}

/// Why a joint is not laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JointRefusal {
    /// An anchor's direction has no length where the layout reads it.
    NoDirection,
    /// An anchor's origin is not finite, or its direction is not a unit vector.
    NotUnit,
    /// A bend or a seam with both anchors on one panel (a rip may part a panel from itself).
    OnePanel,
    /// A bend that turns 0 or less, or more than a half turn, or round a radius below 0 or not
    /// finite.
    BendRange,
    /// A half turn anchored at its virtual sharp, which it has none of.
    HalfTurnSharp,
    /// A bend zone whose ends are not finite.
    ZoneEnds,
    /// A bend on the edge of a roll: not laid out yet.
    BendOnRoll,
    /// A rip without two or more points of its edge on each panel and a turn at each.
    RipPoints,
    /// A rip whose points, turns or curves are not finite.
    RipNotFinite,
    /// A rip that cuts less than 0, or a number that is not finite, from a side.
    RipLap,
    /// A rip's edge that leaves its panel's surface where it is cut back.
    RipOffSurface,
    /// A rip's edge that is not on its panel's outline.
    RipOffOutline,
    /// A rip's edge that runs against its panel's outline.
    RipAgainstOutline,
    /// A torn end of the zone that no spline of the most spans keeps within its tolerance of
    /// the curve it follows.
    TornEnd,
    /// An anchor with no frame on its panel's surface.
    NoFrame,
}

/// Which of the sheet's rule values is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleRefusal {
    /// The thickness: not more than 0, or not finite.
    Thickness,
    /// The K-factor: not from 0 to 1.
    KFactor,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::NoPanel(i) => write!(f, "no panel {i}"),
            Refusal::Panel { panel, why } => write!(f, "panel {panel}: {why:?}"),
            Refusal::Joint { joint, why } => write!(f, "joint {joint}: {why:?}"),
            Refusal::TooLarge => write!(f, "past the development's work limit"),
            Refusal::Rule(why) => write!(f, "rule: {why:?}"),
            Refusal::Roots(n) => write!(f, "{n} pieces need {n} roots"),
        }
    }
}

impl std::error::Error for Refusal {}
