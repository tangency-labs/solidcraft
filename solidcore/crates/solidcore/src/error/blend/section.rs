//! Why a G^n section fails: what its certificate cannot prove, its degeneracies, limits and inputs.
//!
//! Upstream: none. One of the modules of the blend failure causes, re-exported from
//! [`super`].

#[cfg(doc)]
use super::BlendFailure;

/// What a G^n section's certificate could not prove ([`BlendFailure::SectionNotProved`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SectionProof {
    /// A bound or a box the certificate computes finite.
    BoundFinite,
    /// A curve's weight positive over a span.
    CurveWeightPositive,
    /// The definition's enclosure over a box.
    Enclosure,
    /// A face's weights positive over a box.
    FaceWeightPositive,
    /// The ball's radius positive over a box.
    RadiusPositive,
    /// The section's weight positive over an extension.
    SectionWeightPositive,
}

impl SectionProof {
    /// What was not proved, as a phrase.
    #[must_use]
    pub const fn what(self) -> &'static str {
        match self {
            Self::BoundFinite => "a bound of the certificate finite",
            Self::CurveWeightPositive => "the curve's weight positive over the span",
            Self::Enclosure => "the definition's enclosure over the box",
            Self::FaceWeightPositive => "a face's weights positive over the box",
            Self::RadiusPositive => "the ball's radius positive over the box",
            Self::SectionWeightPositive => "the section's weight positive over the extension",
        }
    }
}

/// How a G^n section degenerates ([`BlendFailure::SectionDegenerate`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SectionDegeneracy {
    /// A section of the ball's circle is a half turn or more.
    ArcHalfTurn,
    /// The section's area does not fall as its thumbweight grows, so no thumbweight is the
    /// equal area one.
    AreaDoesNotFall,
    /// The ball has no radius at the rails' start.
    BallHasNoRadius,
    /// The varying balls' centres stand still.
    CentresStandStill,
    /// The rails, or the round's stations, have no length.
    NoLength,
    /// The parameter interval lies in no span of the curve (a curve whose knots are all
    /// one value).
    NoSpanCoversInterval,
    /// The blend's system is singular at a station: its faces' normals agree there.
    NormalsAgree,
    /// A span to be halved has no parameter strictly inside it.
    SpanCannotSplit,
    /// A vector the section normalises vanishes (a face's normal at a foot, the leader's
    /// tangent).
    VectorVanishes,
    /// A weight or a length of the section is zero at the point.
    WeightOrLengthZero,
}

impl SectionDegeneracy {
    /// How it degenerates, as a phrase.
    #[must_use]
    pub const fn what(self) -> &'static str {
        match self {
            Self::ArcHalfTurn => "a section of the ball's circle is a half turn or more",
            Self::AreaDoesNotFall => "the section's area does not fall with its thumbweight",
            Self::BallHasNoRadius => "the ball has no radius at the rails' start",
            Self::CentresStandStill => "the balls' centres stand still",
            Self::NoLength => "the rails or the round's stations have no length",
            Self::NoSpanCoversInterval => "no span of the curve covers the parameter",
            Self::NormalsAgree => {
                "the blend's system is singular at a station (its faces' normals agree there)"
            }
            Self::SpanCannotSplit => "a span has no midpoint inside it to be split at",
            Self::VectorVanishes => {
                "a vector the section normalises vanishes (a face's normal at a foot, or the leader's tangent)"
            }
            Self::WeightOrLengthZero => "a weight or a length of the section is zero",
        }
    }
}

/// What a G^n section does not handle ([`BlendFailure::SectionUnsupported`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SectionLimit {
    /// A curve's weight that is not positive.
    CurveWeightNotPositive,
    /// A face that is a blend by its definition, enclosed only for a round of one radius on
    /// a round of one radius between NURBS faces.
    DefinitionFaceNotOneRadius,
    /// A curve or face of degree 0 in a direction.
    DegreeBelowOne,
    /// A face that is not a NURBS surface: its section of order 2 and up, and its
    /// certificate, need one.
    FaceNotNurbs,
    /// A face's weight that is not positive.
    FaceWeightNotPositive,
    /// A derivative order past the most the series hold.
    OrderPastSeries {
        /// The order asked.
        order: usize,
        /// The most the series hold.
        most: usize,
    },
}

/// Which inputs of a G^n section do not fit together ([`BlendFailure::SectionInputInvalid`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SectionInput {
    /// The rails and the centre line run over different intervals.
    IntervalsDiffer,
    /// The round's range is not within its stations.
    RangeOutsideStations,
    /// The span asked for starts at or past the definition's last station.
    SpanPastLastStation,
}

impl SectionInput {
    /// What does not fit, as a phrase.
    #[must_use]
    pub const fn what(self) -> &'static str {
        match self {
            Self::IntervalsDiffer => "the rails and the centre line run over different intervals",
            Self::RangeOutsideStations => "the round's range is not within its stations",
            Self::SpanPastLastStation => "the span asked for is past the definition's last station",
        }
    }
}
