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
//! `[breaks[i], breaks[i + 1]]` of the joined curve. Before it interpolates the sections'
//! control points, the skin makes them compatible (one degree and one knot vector:
//! `make_compatible_knot_tol`), which can rescale a section's knots to `[0, 1]`, merge
//! knots closer than its tolerance and insert the others' knots. That step says where
//! each of a section's knots went ([`KnotMap`], read off its own knot vectors), so each
//! segment's span maps onto the skinned surface's knots across the sections (V). The
//! surface is then split at those knots into faces, and a face holds a curve when the
//! face's V interval and the curve's mapped span overlap. Every comparison is between knot
//! values the construction itself carried, compared exactly: no tolerance and no geometry.
//!
//! Where a condition for that does not hold, the trace reports [`NotTraced`] and why,
//! never an estimate; the skin is built the same either way.
//!
//! Deviations: not upstream (the whole module).

use crate::nurbs::crv_tool::p2::KnotMap;
use crate::topology::{EdgeId, FaceId};

/// Why a skin's side faces were not traced to their profile curves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotTraced {
    /// The profiles are open curves, lofted as one composite each, and the composite
    /// builder's account of each segment's curve was not recorded.
    OpenProfiles,
    /// A section's end of a curve did not land on a knot of the skinned surface across the
    /// sections (a step after the compatibility moved the knots).
    SkinMovedKnots,
    /// Making the sections compatible merged both ends of a curve's span into one knot (a
    /// curve shorter than the merge tolerance), so no face holds it.
    SegmentMerged,
    /// A step of making the sections compatible did not say where a section's knots went
    /// (a knot sent to two places, a degree elevation that changed the distinct knots, or
    /// the skin's spine path, which is not traced).
    KnotMapBroken,
    /// A side face's bounds across the sections are not knots of its surface.
    FaceOffKnots,
    /// A profile edge has no recorded curve, or a loop has not one edge per segment.
    EdgeUntraced,
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
    /// profile by profile, and within a profile in the caller's order. A face holds at
    /// least one curve of each section: the faces split where the skin is not smooth, so a
    /// curve with a kink of its own appears on several faces, and two curves of a section
    /// meeting straight on (collinear lines) can share one.
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
    /// Each open profile's loop (its composite's breaks and each segment's curve), in
    /// profile order, as the skin built them.
    pub(crate) open_loops: Vec<LoopSpans>,
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

/// Where one segment of a section's loop came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Segment {
    /// A closed profile's edge, its curve recorded with it ([`SkinTrace`]'s `edge_curves`).
    Edge(EdgeId),
    /// An open profile's curve, by its index among all the skin's curves.
    Curve(usize),
    /// A line the composite builder put across a gap: from no curve.
    GapLine,
}

/// One loop of one section: the join's breaks (knots of its joined curve) and where each
/// segment came from.
#[derive(Debug)]
pub(crate) struct LoopSpans {
    pub(crate) breaks: Vec<f64>,
    pub(crate) segments: Vec<Segment>,
}

/// Whether `v` is one of `knots`, bit for bit.
fn is_knot(knots: &[f64], v: f64) -> bool {
    knots.iter().any(|k| k.to_bits() == v.to_bits())
}

/// The sources of one layer's faces: `sections` the layer's loop in each section, `maps`
/// where each section's knots went in the skin (one per section, in order),
/// `surface_knots` the skinned surface's distinct knots across the sections, `faces` each
/// face made from it with its V interval.
pub(crate) fn layer_sources(
    edge_curves: &[(EdgeId, usize)],
    sections: &[&LoopSpans],
    maps: &[Option<KnotMap>],
    surface_knots: &[f64],
    faces: &[(FaceId, f64, f64)],
) -> Result<Vec<(FaceId, Vec<usize>)>, NotTraced> {
    if sections.is_empty() {
        return Err(NotTraced::EdgeUntraced);
    }
    if maps.len() != sections.len() {
        return Err(NotTraced::KnotMapBroken);
    }
    // each section's segments, their ends mapped onto the surface's knots: (span start,
    // span end, curve)
    let mut spans: Vec<(f64, f64, usize)> = Vec::new();
    for (s, map) in sections.iter().copied().zip(maps) {
        let map = map.as_ref().ok_or(NotTraced::KnotMapBroken)?;
        if s.breaks.len() != s.segments.len() + 1 {
            return Err(NotTraced::EdgeUntraced);
        }
        let mut mapped = Vec::new();
        for &b in &s.breaks {
            let v = map.get(b).ok_or(NotTraced::KnotMapBroken)?;
            if !is_knot(surface_knots, v) {
                return Err(NotTraced::SkinMovedKnots);
            }
            mapped.push(v);
        }
        for (i, seg) in s.segments.iter().enumerate() {
            let curve = match *seg {
                Segment::Edge(e) => edge_curves
                    .iter()
                    .find(|(edge, _)| *edge == e)
                    .map(|&(_, c)| c)
                    .ok_or(NotTraced::EdgeUntraced)?,
                Segment::Curve(c) => c,
                // a gap line's span belongs to no curve
                Segment::GapLine => continue,
            };
            let (Some(&a), Some(&b)) = (mapped.get(i), mapped.get(i + 1)) else {
                return Err(NotTraced::EdgeUntraced);
            };
            if a >= b {
                return Err(NotTraced::SegmentMerged);
            }
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

    fn spans(breaks: &[f64], edges: &[u32]) -> LoopSpans {
        LoopSpans {
            breaks: breaks.to_vec(),
            segments: edges
                .iter()
                .map(|&e| Segment::Edge(EdgeId::from_index(e)))
                .collect(),
        }
    }

    /// The map sending each of `from` to the matching `to`.
    fn map(from: &[f64], to: &[f64]) -> KnotMap {
        KnotMap(from.iter().copied().zip(to.iter().copied()).collect())
    }

    /// The identity map on `knots`.
    fn id(knots: &[f64]) -> KnotMap {
        KnotMap::identity(knots)
    }

    #[test]
    fn faces_take_the_curves_whose_spans_they_cover() {
        let knots = [0.0, 1.0, 2.0, 3.0];
        let edge_curves = vec![
            (EdgeId::from_index(7), 2),
            (EdgeId::from_index(8), 0),
            (EdgeId::from_index(9), 1),
        ];
        let s = spans(&knots, &[7, 8, 9]);
        let f = |i: u32| FaceId::from_index(i);
        // one face over the first two segments, one over the third
        let got = layer_sources(
            &edge_curves,
            &[&s],
            &[Some(id(&knots))],
            &knots,
            &[(f(0), 0.0, 2.0), (f(1), 2.0, 3.0)],
        )
        .unwrap();
        assert_eq!(got, vec![(f(0), vec![0, 2]), (f(1), vec![1])]);
    }

    /// Sections whose knots differ: each one's breaks go through its own map (here the
    /// second rescaled from [0, 3] to [0, 1] and given the first's middle knot), and each
    /// face takes the curves whose mapped spans it covers.
    #[test]
    fn sections_whose_knots_differ_go_through_their_maps() {
        let a = spans(&[0.0, 0.5, 1.0], &[1, 2]);
        let b = spans(&[0.0, 3.0], &[3]);
        let edge_curves = vec![
            (EdgeId::from_index(1), 0),
            (EdgeId::from_index(2), 1),
            (EdgeId::from_index(3), 2),
        ];
        let surface = [0.0, 0.5, 1.0];
        let f = |i: u32| FaceId::from_index(i);
        let got = layer_sources(
            &edge_curves,
            &[&a, &b],
            &[Some(id(&surface)), Some(map(&[0.0, 3.0], &[0.0, 1.0]))],
            &surface,
            &[(f(0), 0.0, 0.5), (f(1), 0.5, 1.0)],
        )
        .unwrap();
        assert_eq!(got, vec![(f(0), vec![0, 2]), (f(1), vec![1, 2])]);
    }

    /// An open profile's segments name their curves directly, and a gap line's span
    /// belongs to no curve: the face over it holds only the other section's curve.
    #[test]
    fn open_segments_name_their_curves_and_a_gap_line_none() {
        let knots = [0.0, 1.0, 2.0, 3.0];
        let open = LoopSpans {
            breaks: knots.to_vec(),
            segments: vec![Segment::Curve(1), Segment::GapLine, Segment::Curve(0)],
        };
        let other = LoopSpans {
            breaks: knots.to_vec(),
            segments: vec![Segment::Curve(2), Segment::Curve(3), Segment::Curve(4)],
        };
        let f = |i: u32| FaceId::from_index(i);
        let got = layer_sources(
            &[],
            &[&open, &other],
            &[Some(id(&knots)), Some(id(&knots))],
            &knots,
            &[(f(0), 0.0, 1.0), (f(1), 1.0, 2.0), (f(2), 2.0, 3.0)],
        )
        .unwrap();
        assert_eq!(
            got,
            vec![(f(0), vec![1, 2]), (f(1), vec![3]), (f(2), vec![0, 4])]
        );
    }

    #[test]
    fn a_curve_merged_to_one_knot_is_not_traced() {
        let s = spans(&[0.0, 0.5, 0.5004, 1.0], &[1, 2, 3]);
        let edge_curves = vec![
            (EdgeId::from_index(1), 0),
            (EdgeId::from_index(2), 1),
            (EdgeId::from_index(3), 2),
        ];
        let surface = [0.0, 0.5, 1.0];
        let got = layer_sources(
            &edge_curves,
            &[&s],
            &[Some(map(&[0.0, 0.5, 0.5004, 1.0], &[0.0, 0.5, 0.5, 1.0]))],
            &surface,
            &[],
        );
        assert_eq!(got, Err(NotTraced::SegmentMerged));
    }

    #[test]
    fn a_missing_or_broken_map_is_not_traced() {
        let knots = [0.0, 1.0, 2.0];
        let s = spans(&knots, &[1, 2]);
        let edge_curves = vec![(EdgeId::from_index(1), 0), (EdgeId::from_index(2), 1)];
        assert_eq!(
            layer_sources(&edge_curves, &[&s], &[None], &knots, &[]),
            Err(NotTraced::KnotMapBroken)
        );
        assert_eq!(
            layer_sources(&edge_curves, &[&s], &[], &knots, &[]),
            Err(NotTraced::KnotMapBroken)
        );
        // a break the map does not hold
        assert_eq!(
            layer_sources(
                &edge_curves,
                &[&s],
                &[Some(map(&[0.0, 2.0], &[0.0, 2.0]))],
                &knots,
                &[]
            ),
            Err(NotTraced::KnotMapBroken)
        );
    }

    #[test]
    fn a_face_bound_off_the_knots_is_not_traced() {
        let knots = [0.0, 1.0, 2.0];
        let s = spans(&knots, &[1, 2]);
        let edge_curves = vec![(EdgeId::from_index(1), 0), (EdgeId::from_index(2), 1)];
        let got = layer_sources(
            &edge_curves,
            &[&s],
            &[Some(id(&knots))],
            &knots,
            &[(FaceId::from_index(0), 0.0, 1.5)],
        );
        assert_eq!(got, Err(NotTraced::FaceOffKnots));
    }

    #[test]
    fn a_break_off_the_surface_s_knots_is_not_traced() {
        let knots = [0.0, 1.0, 2.0];
        let s = spans(&knots, &[1, 2]);
        let edge_curves = vec![(EdgeId::from_index(1), 0), (EdgeId::from_index(2), 1)];
        // the surface's knots lack the sections' middle knot
        let got = layer_sources(&edge_curves, &[&s], &[Some(id(&knots))], &[0.0, 2.0], &[]);
        assert_eq!(got, Err(NotTraced::SkinMovedKnots));
    }

    #[test]
    fn an_edge_with_no_recorded_curve_is_not_traced() {
        let knots = [0.0, 1.0, 2.0];
        let s = spans(&knots, &[1, 2]);
        // edge 2 has no curve recorded
        let edge_curves = vec![(EdgeId::from_index(1), 0)];
        let got = layer_sources(&edge_curves, &[&s], &[Some(id(&knots))], &knots, &[]);
        assert_eq!(got, Err(NotTraced::EdgeUntraced));
        // and a loop with a break too few for its edges
        let short = spans(&[0.0, 2.0], &[1, 2]);
        let got = layer_sources(
            &[(EdgeId::from_index(1), 0), (EdgeId::from_index(2), 1)],
            &[&short],
            &[Some(id(&knots))],
            &knots,
            &[],
        );
        assert_eq!(got, Err(NotTraced::EdgeUntraced));
    }
}
