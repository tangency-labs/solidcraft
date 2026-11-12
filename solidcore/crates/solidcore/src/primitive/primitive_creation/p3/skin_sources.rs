//! Which profile curves each side face of a skin was swept from, traced through the
//! skin's construction (not upstream: tangency-labs/solidcore#3139).
//!
//! Upstream: none. The trace follows `CreateSkinPrimitive` and `CreateSkinFromFaces`
//! (`source/SMLib/src/SmPrimitiveCreation.cpp`) without changing what they build.
//!
//! | Rust              | Upstream |
//! |-------------------|----------|
//! | [`SkinSources`]   | (none)   |
//! | [`NotTraced`]     | (none)   |
//! | [`SkinTrace`]     | (none)   |
//!
//! How the trace runs. Each closed profile becomes a planar face: the edge made from
//! each profile curve is recorded with the curve's index. Each loop of a profile face
//! becomes one composite curve whose segments are the loop's edges, joined into one
//! B-spline; the join records the knot at which each segment ends, so segment `i` spans
//! `[breaks[i], breaks[i + 1]]` of the joined curve. The skin interpolates the sections'
//! control points, so where every section's joined curve has the same degree and the
//! same knots (bit for bit), its compatibility step changes nothing and the skinned
//! surface's knots across the sections (V) are the sections' knots. The surface is then
//! split at those knots into faces, and a face holds a curve when the face's V interval
//! and the curve's segment span overlap. Every comparison is between knot values the
//! construction itself carried, compared exactly: no tolerance and no geometry.
//!
//! Where a condition for that does not hold, the trace reports [`NotTraced`] and why,
//! never an estimate; the skin is built the same either way.
//!
//! Deviations: not upstream (the whole module).

use crate::error::Result;
use crate::nurbs::b_spline_curve::p1::BSplineCurve;
use crate::topology::{EdgeId, FaceId};

/// Why a skin's side faces were not traced to their profile curves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotTraced {
    /// The profiles are open curves, lofted as one composite each; the composite
    /// builder does not report which curve each of its segments is.
    OpenProfiles,
    /// The sections' joined curves differ in degree or knots, so making them compatible
    /// can move a curve's ends across the skin (knots within a thousandth of the range
    /// are merged).
    SectionsNotKnotIdentical,
    /// The skinned surface's knots across the sections are not the sections' knots.
    SkinMovedKnots,
    /// A side face's bounds across the sections are not knots of its surface.
    FaceOffKnots,
    /// A profile edge has no recorded curve, or a loop has not one edge per segment.
    EdgeUntraced,
    /// A section loop's joined curve's degree or knots could not be read.
    JoinedCurveUnreadable,
    /// The skinned surface's knots across the sections could not be read.
    SurfaceKnotsUnreadable,
    /// A side face's surface or its domain across the sections could not be read.
    FaceDomainUnreadable,
    /// The construction stopped before it made the side faces, so there was nothing to
    /// trace.
    SideFacesNotReached,
}

/// Which profile curves each side face of a skin was swept from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkinSources {
    /// Each side face, in the order of the skin's side faces, with the indices (into
    /// the skin's curves, every profile's) of the curves it was swept from, ascending:
    /// profile by profile, and within a profile in the caller's order. The faces split
    /// at the skin's knots of continuity G1 or less, judged on the knots: each join of two
    /// curves is one, tangent or not, so a face holds one curve of each section, and a
    /// curve with a kink of its own appears on several faces.
    Traced(Vec<(FaceId, Vec<usize>)>),
    /// Not traced, and why.
    NotTraced {
        /// Why.
        why: NotTraced,
    },
}

/// What the skin's construction records for [`SkinSources`] when asked.
#[derive(Debug, Default)]
pub struct SkinTrace {
    /// The edge made from each profile curve, with the curve's index.
    pub(crate) edge_curves: Vec<(EdgeId, usize)>,
    /// The profiles are lofted as open composites.
    pub(crate) open_profiles: bool,
    /// The outcome, once the side faces are made.
    pub(crate) sources: Option<SkinSources>,
}

impl SkinTrace {
    /// The outcome: [`SkinSources::NotTraced`] with [`NotTraced::SideFacesNotReached`]
    /// when the side faces were never reached.
    #[must_use]
    pub fn into_sources(self) -> SkinSources {
        self.sources.unwrap_or(SkinSources::NotTraced {
            why: NotTraced::SideFacesNotReached,
        })
    }
}

/// One loop of one section: its joined curve's knots and its segments.
#[derive(Debug)]
pub(crate) struct LoopSpans {
    degree: usize,
    knots: Vec<f64>,
    multiplicities: Vec<usize>,
    breaks: Vec<f64>,
    edges: Vec<EdgeId>,
}

impl LoopSpans {
    /// The loop of `joined`, with the join's `breaks` and the edge of each segment.
    pub(crate) fn of(joined: &BSplineCurve, breaks: Vec<f64>, edges: Vec<EdgeId>) -> Result<Self> {
        let (knots, multiplicities) = joined.knots(None)?;
        Ok(Self {
            degree: joined.degree()?,
            knots,
            multiplicities,
            breaks,
            edges,
        })
    }
}

/// Bit-for-bit equality of two knot lists.
fn same_values(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
}

/// Whether `v` is one of `knots`, bit for bit.
fn is_knot(knots: &[f64], v: f64) -> bool {
    knots.iter().any(|k| k.to_bits() == v.to_bits())
}

/// The sources of one layer's faces: `sections` the layer's loop in each section,
/// `surface_knots` the skinned surface's distinct knots across the sections, `faces`
/// each face made from it with its V interval.
pub(crate) fn layer_sources(
    edge_curves: &[(EdgeId, usize)],
    sections: &[&LoopSpans],
    surface_knots: &[f64],
    faces: &[(FaceId, f64, f64)],
) -> std::result::Result<Vec<(FaceId, Vec<usize>)>, NotTraced> {
    let first = *sections.first().ok_or(NotTraced::EdgeUntraced)?;
    for s in sections.iter().copied() {
        if s.degree != first.degree
            || !same_values(&s.knots, &first.knots)
            || s.multiplicities != first.multiplicities
        {
            return Err(NotTraced::SectionsNotKnotIdentical);
        }
    }
    if !same_values(surface_knots, &first.knots) {
        return Err(NotTraced::SkinMovedKnots);
    }
    // each section's segments: (span start, span end, curve)
    let mut spans: Vec<(f64, f64, usize)> = Vec::new();
    for s in sections.iter().copied() {
        if s.breaks.len() != s.edges.len() + 1 {
            return Err(NotTraced::EdgeUntraced);
        }
        for (i, e) in s.edges.iter().enumerate() {
            let curve = edge_curves
                .iter()
                .find(|(edge, _)| edge == e)
                .map(|&(_, c)| c)
                .ok_or(NotTraced::EdgeUntraced)?;
            let (Some(&a), Some(&b)) = (s.breaks.get(i), s.breaks.get(i + 1)) else {
                return Err(NotTraced::EdgeUntraced);
            };
            spans.push((a, b, curve));
        }
    }
    let mut out = Vec::new();
    for &(face, v0, v1) in faces {
        if !is_knot(surface_knots, v0) || !is_knot(surface_knots, v1) {
            return Err(NotTraced::FaceOffKnots);
        }
        let mut curves: Vec<usize> = spans
            .iter()
            .filter(|&&(a, b, _)| v0.max(a) < v1.min(b))
            .map(|&(_, _, c)| c)
            .collect();
        curves.sort_unstable();
        curves.dedup();
        out.push((face, curves));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(knots: &[f64], breaks: &[f64], edges: &[u32]) -> LoopSpans {
        LoopSpans {
            degree: 1,
            knots: knots.to_vec(),
            multiplicities: vec![1; knots.len()],
            breaks: breaks.to_vec(),
            edges: edges.iter().map(|&e| EdgeId::from_index(e)).collect(),
        }
    }

    #[test]
    fn faces_take_the_curves_whose_spans_they_cover() {
        let knots = [0.0, 1.0, 2.0, 3.0];
        let edge_curves = vec![
            (EdgeId::from_index(7), 2),
            (EdgeId::from_index(8), 0),
            (EdgeId::from_index(9), 1),
        ];
        let s = spans(&knots, &knots, &[7, 8, 9]);
        let f = |i: u32| FaceId::from_index(i);
        // one face over the first two segments, one over the third
        let got = layer_sources(
            &edge_curves,
            &[&s],
            &knots,
            &[(f(0), 0.0, 2.0), (f(1), 2.0, 3.0)],
        )
        .unwrap();
        assert_eq!(got, vec![(f(0), vec![0, 2]), (f(1), vec![1])]);
    }

    #[test]
    fn different_section_knots_are_not_traced() {
        let a = spans(&[0.0, 1.0, 2.0], &[0.0, 1.0, 2.0], &[1, 2]);
        let b = spans(&[0.0, 1.5, 2.0], &[0.0, 1.5, 2.0], &[3, 4]);
        let got = layer_sources(&[], &[&a, &b], &[0.0, 1.0, 2.0], &[]);
        assert_eq!(got, Err(NotTraced::SectionsNotKnotIdentical));
    }

    #[test]
    fn a_face_bound_off_the_knots_is_not_traced() {
        let knots = [0.0, 1.0, 2.0];
        let s = spans(&knots, &knots, &[1, 2]);
        let edge_curves = vec![(EdgeId::from_index(1), 0), (EdgeId::from_index(2), 1)];
        let got = layer_sources(
            &edge_curves,
            &[&s],
            &knots,
            &[(FaceId::from_index(0), 0.0, 1.5)],
        );
        assert_eq!(got, Err(NotTraced::FaceOffKnots));
    }

    #[test]
    fn surface_knots_other_than_the_sections_are_not_traced() {
        let knots = [0.0, 1.0, 2.0];
        let s = spans(&knots, &knots, &[1, 2]);
        let edge_curves = vec![(EdgeId::from_index(1), 0), (EdgeId::from_index(2), 1)];
        // the surface's V knots with a knot the sections do not have
        let got = layer_sources(&edge_curves, &[&s], &[0.0, 0.5, 1.0, 2.0], &[]);
        assert_eq!(got, Err(NotTraced::SkinMovedKnots));
    }

    #[test]
    fn an_edge_with_no_recorded_curve_is_not_traced() {
        let knots = [0.0, 1.0, 2.0];
        let s = spans(&knots, &knots, &[1, 2]);
        // edge 2 has no curve recorded
        let edge_curves = vec![(EdgeId::from_index(1), 0)];
        let got = layer_sources(&edge_curves, &[&s], &knots, &[]);
        assert_eq!(got, Err(NotTraced::EdgeUntraced));
        // and a loop with a break too few for its edges
        let short = spans(&knots, &[0.0, 2.0], &[1, 2]);
        let got = layer_sources(
            &[(EdgeId::from_index(1), 0), (EdgeId::from_index(2), 1)],
            &[&short],
            &knots,
            &[],
        );
        assert_eq!(got, Err(NotTraced::EdgeUntraced));
    }
}
