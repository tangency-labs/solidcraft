//! Which link of the fillet structures a routine is given is not there or not as it needs.
//!
//! Upstream: none. One of the modules of the blend failure causes, re-exported from
//! [`super`].

#[cfg(doc)]
use super::BlendFailure;

/// The link of the fillet structures that is not there or not as the routine needs
/// ([`BlendFailure::FilletSetUpInvalid`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FilletSetUp {
    /// The UV curves either side of a gap the fillet face's boundary loop caps.
    BoundaryUvCurve,
    /// A fillet corner's extended surface of a side face.
    CornerExtendedSurface,
    /// The body vertex a fillet corner rounds.
    CornerVertex,
    /// The fillet geometry a cross-section's primary edge use spans.
    CrossSectionGeom,
    /// The fillet geometry an edge use of a fillet edge other than a cross-section spans.
    EdgeUseGeom,
    /// The vertex use an edge use of the body starts at.
    EdgeUseVertexUse,
    /// The two fillet vertices a fillet edge joins.
    EdgeVertices,
    /// A rail/rail interpolation edge among its corner's fillet edges.
    InterpolationEdgeAtCorner,
    /// The rail ending at an end of a rail/rail interpolation edge.
    InterpolationEndRail,
    /// A Join's fillet vertex's mate.
    Mate,
    /// A mate vertex's main vertex (its first mate), whose geometry the mate's calculation
    /// reads: a fillet vertex a caller builds and passes to the public calculator need not
    /// have one.
    MateMainVertex,
    /// The cross-section opposite a rail/rail interpolation edge at its corner.
    OppositeCrossSection,
    /// The original face a fillet edge records, of any fillet edge type (a rail/rail
    /// interpolation edge's face it lies on included).
    OriginalFace,
    /// The side edge use a fillet vertex on a side edge is classified to.
    PointClassEdgeUse,
    /// The face a fillet vertex (or its mate) is classified to.
    PointClassFace,
    /// A fillet edge's primary edge use.
    PrimaryEdgeUse,
    /// The mate of a fillet edge's primary edge use.
    PrimaryEdgeUseMate,
    /// The radial edge use of an edge use at a corner (a side edge's or the filleted
    /// edge's).
    RadialEdgeUse,
    /// A rail edge's 3D curve, or the UV trim curve the 2D path reads.
    RailCurve,
    /// A rail's end vertex on the rail's curve.
    RailEndOffRail,
    /// The vertex use of a rail's edge at a fillet vertex at the rail's end.
    RailEndVertexUse,
    /// The offset surface of a fillet geometry's rail, its own or its solver's.
    RailOffsetSurface,
    /// The reference curve a variable-radius solver's recalculation of its fillet geometry
    /// is given.
    ReferenceCurve,
    /// A fillet solver's fillet executive.
    SolverExecutive,
    /// One of a fillet solver's two offset surfaces.
    SolverOffsetSurface,
    /// A curve-based fillet solver's original curve (the edge or centre curve it is set
    /// up on).
    SolverOriginalCurve,
    /// A fillet solver's surface generator.
    SurfaceGenerator,
    /// The B-spline UV trim curve of a rail's edge use.
    UvTrimCurve,
    /// The cross-section edge at a fillet vertex.
    VertexCrossSection,
    /// The end rib at a fillet vertex at a fillet's end.
    VertexEndRib,
    /// The two fillet/fillet edges at a fillet vertex where three fillets meet.
    VertexFilletFilletEdges,
    /// The fillet geometries a fillet vertex's kind joins.
    VertexGeoms,
    /// The rail at a fillet vertex at a fillet's end.
    VertexRail,
    /// The face use of a vertex use of a face's loop.
    VertexUseFaceUse,
}

impl FilletSetUp {
    /// What is wrong, as a clause.
    pub(super) const fn what(self) -> &'static str {
        match self {
            Self::BoundaryUvCurve => {
                "a gap in the fillet face's boundary loop has no UV curve on one side to cap from"
            }
            Self::CornerExtendedSurface => {
                "the fillet corner has no extended surface of the side face"
            }
            Self::CornerVertex => "the fillet corner records no body vertex it rounds",
            Self::CrossSectionGeom => "a cross-section's primary edge use spans no fillet geometry",
            Self::EdgeUseGeom => {
                "an edge use of a fillet edge other than a cross-section spans no fillet geometry"
            }
            Self::EdgeUseVertexUse => "an edge use of the body starts at no vertex use",
            Self::EdgeVertices => "a fillet edge does not join two fillet vertices",
            Self::InterpolationEdgeAtCorner => {
                "the rail/rail interpolation edge is not one of its corner's fillet edges"
            }
            Self::InterpolationEndRail => {
                "an end of the rail/rail interpolation edge is no rail's end"
            }
            Self::Mate => "a Join's fillet vertex has no mate",
            Self::MateMainVertex => "a mate vertex has no main vertex",
            Self::OppositeCrossSection => {
                "the fillet edge opposite the rail/rail interpolation edge is not a cross-section"
            }
            Self::OriginalFace => "a fillet edge records no original face",
            Self::PointClassEdgeUse => {
                "a fillet vertex on a side edge is not classified to that edge's edge use"
            }
            Self::PointClassFace => "a fillet vertex is not classified to a face",
            Self::PrimaryEdgeUse => "a fillet edge has no primary edge use",
            Self::PrimaryEdgeUseMate => "a fillet edge's primary edge use has no mate",
            Self::RadialEdgeUse => "an edge use of a side edge has no radial edge use",
            Self::RailCurve => {
                "a rail edge lacks its 3D curve or the UV trim curve the 2D path reads"
            }
            Self::RailEndOffRail => {
                "the fillet vertex at the rail's end does not lie on the rail's curve"
            }
            Self::RailEndVertexUse => {
                "a fillet vertex at a rail's end has no vertex use of the rail's edge"
            }
            Self::RailOffsetSurface => {
                "a rail of a fillet geometry has no offset surface, of its own or its solver's"
            }
            Self::ReferenceCurve => {
                "the variable-radius solver's recalculation is given no reference curve"
            }
            Self::SolverExecutive => "a fillet solver has no fillet executive",
            Self::SolverOffsetSurface => "a fillet solver lacks one of its two offset surfaces",
            Self::SolverOriginalCurve => "a curve-based fillet solver has no original curve",
            Self::SurfaceGenerator => "a fillet solver has no surface generator",
            Self::UvTrimCurve => "the rail's edge use has no B-spline UV trim curve",
            Self::VertexCrossSection => "a fillet vertex has no cross-section edge",
            Self::VertexEndRib => "a fillet vertex at a fillet's end has no end rib",
            Self::VertexFilletFilletEdges => {
                "a fillet vertex where three fillets meet lacks its two fillet/fillet edges"
            }
            Self::VertexGeoms => "a fillet vertex lacks the fillet geometries its kind joins",
            Self::VertexRail => "a fillet vertex at a fillet's end has no rail",
            Self::VertexUseFaceUse => "a vertex use of a face's loop is in no face use",
        }
    }
}
