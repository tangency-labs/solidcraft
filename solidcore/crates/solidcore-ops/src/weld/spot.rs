//! The named looks of a spot weld: each a [`SpotStyle`] of fixed proportions, the data the
//! kernel's `solidcore::weld::spot::spot_weld` builds a spot from. Which look a user picked
//! is the application's to store; the kernel never sees a name.
//!
//! Every size is a fraction: of the spot diameter `d`, or of a sheet's thickness for a
//! pin's dish. This module holds those numbers and computes nothing.

pub use solidcore::weld::spot::{
    MarkFaces, MarkTop, SpotBall, SpotMarks, SpotPin, SpotStyle, SpotWeld,
};

/// The marks every look but [`SpotLook::Ball`] draws on both outer faces: a core of
/// `0.55 d`, a band out to `d` and an edge out to `1.05 d`, flat, `0.006 d` thick.
pub const MARKS: SpotMarks = SpotMarks {
    core: 0.55,
    band: 1.0,
    edge: 1.05,
    top: MarkTop::Flat { thickness: 0.006 },
    faces: MarkFaces::Both,
};

/// How a spot weld is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpotLook {
    /// The marks on both outer faces, nothing else (the default, [`SpotLook::DEFAULT`]).
    Marks,
    /// A ball of the spot diameter centred on the faying face; the marks only on an outer
    /// face the ball does not reach, so the spot always shows on both faces.
    Ball,
    /// The marks and a ball of `0.6 d` centred on the faying face.
    MarksSmallBall,
    /// The marks and a pin of the core's diameter through both sheets, flush with the
    /// outer faces.
    MarksThinPin,
    /// The marks and a pin of the spot diameter through both sheets, each end dished in by
    /// `0.3` of the sheet it lies in (at most a half sphere).
    MarksPinConcaveEnds,
    /// The marks dished into a shallow inverted dome on each outer face: `0.04 d` high at
    /// the rim, `0.004 d` in the middle, read as the electrode's dent.
    Dent,
}

impl SpotLook {
    /// Every look, in the order a weld tool lists them.
    pub const ALL: [Self; 6] = [
        Self::Marks,
        Self::Ball,
        Self::MarksSmallBall,
        Self::MarksThinPin,
        Self::MarksPinConcaveEnds,
        Self::Dent,
    ];

    /// The look a new spot weld starts with.
    pub const DEFAULT: Self = Self::Marks;

    /// The look's style, the data the kernel builds the spot from.
    #[must_use]
    pub const fn style(self) -> SpotStyle {
        let none = SpotStyle {
            marks: None,
            ball: None,
            pin: None,
        };
        match self {
            Self::Marks => SpotStyle {
                marks: Some(MARKS),
                ..none
            },
            Self::Ball => SpotStyle {
                marks: Some(SpotMarks {
                    faces: MarkFaces::WhereBallHidden,
                    ..MARKS
                }),
                ball: Some(SpotBall { diameter: 1.0 }),
                ..none
            },
            Self::MarksSmallBall => SpotStyle {
                marks: Some(MARKS),
                ball: Some(SpotBall { diameter: 0.6 }),
                ..none
            },
            Self::MarksThinPin => SpotStyle {
                marks: Some(MARKS),
                pin: Some(SpotPin {
                    diameter: 0.55,
                    dish: None,
                }),
                ..none
            },
            Self::MarksPinConcaveEnds => SpotStyle {
                marks: Some(MARKS),
                pin: Some(SpotPin {
                    diameter: 1.0,
                    dish: Some(0.3),
                }),
                ..none
            },
            Self::Dent => SpotStyle {
                marks: Some(SpotMarks {
                    top: MarkTop::Dish {
                        rim: 0.04,
                        middle: 0.004,
                    },
                    ..MARKS
                }),
                ..none
            },
        }
    }
}
