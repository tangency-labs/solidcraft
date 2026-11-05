//! How far past its face an offset face's surface must reach for the offsets of its
//! neighbours to meet its own: the margin the offset executive extends each surface by before
//! it intersects the offsets (not upstream: upstream extends every surface by five offset
//! distances).
//!
//! At a point `p` of an edge between a face `F` (unit normal `nF`, binormal `bF` into `F`,
//! offset `dF`) and its neighbour `G` (unit normal `nG`, offset `dG`), `c = nF·nG`, the
//! offsets' tangent planes `(x − p)·nF = dF` and `(x − p)·nG = dG` meet on the line through
//! `p + a nF + β nG`, `a + β c = dF`, `a c + β = dG`. Its foot on `F`'s tangent plane lies
//! past the edge (along the outward binormal `m = −bF`) by the edge's signed reach
//!
//! ```text
//! r = β (nG·m),   β = (dG − c dF) / (1 − c²)   ( = d / (1 + c) where dF = dG = d ),
//! ```
//!
//! `d·cot(α / 2)` for equal offsets, `α` the sector angle of the solid between the faces:
//! `d` across a box's edge, `0` across a tangent edge, negative where the offsets meet inside
//! `F`. At a vertex where `F`'s edge uses meet with reaches `r1`, `r2` and outward binormals
//! `m1`, `m2`, the offset corner's foot `x` on the tangent plane solves `x·m1 = r1`,
//! `x·m2 = r2`, at `|x|` from the vertex.
//!
//! Each foot is within `|r|` (`|x|` at a corner) of a point of the face, so within that of the
//! face's surface's box, however far the meeting runs along the face: an inward meeting that
//! leaves the face across another of its edges (a hole's offset growing past a side) is held
//! as well as an outward one. The reach of a face is the largest of these, sampled along each
//! edge (its sector angle varies) and at each corner. Faces whose offsets never meet, a knife
//! edge's (`c = −1` for equal offsets) or tangent faces offset apart (`c = 1`, `dF ≠ dG`),
//! add nothing: no extension makes them meet.

use crate::error::{Result, SolidError};
use crate::geom::vector3d::{sqrt, Vec3};
use crate::kernel::Kernel;
use crate::math::EFF_ZERO;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::topology::edgeuse::{p1 as eu, p3 as eu3};
use crate::topology::{edge, loopuse, Brep, EdgeuseId, FaceId, FaceuseId};

const OP: &str = "offset::offset_executive::reach::extension_reach";

/// The number of intervals each edge is sampled at for its sector angle.
const EDGE_SAMPLES: u32 = 8;

/// One end of an edge use, as the corner beside it reads it.
#[derive(Debug, Clone, Copy)]
struct End {
    /// The signed reach past the edge.
    reach: f64,
    /// The outward binormal (away from the face, perpendicular to the edge).
    outward: Vec3,
}

/// The signed reach past the edge of a face offset by `own` whose neighbour is offset by
/// `other`, from their unit normals and the face's outward binormal (module docs); zero where
/// the offsets do not meet: faces folded back on each other (equal offsets) or tangent to
/// each other (different offsets: their offsets run apart), each within `tangent_sine`, the
/// sine of the tangency tolerance.
fn signed_reach(
    n_own: Vec3,
    n_other: Vec3,
    outward: Vec3,
    own: f64,
    other: f64,
    tangent_sine: f64,
) -> f64 {
    let c = n_own.dot(n_other);
    let sine_squared = (tangent_sine * tangent_sine).max(EFF_ZERO);
    #[allow(clippy::float_cmp)] // the same offset, not a computed value
    let beta = if own == other {
        // 1 + c = 1 - cos(θ) ≈ θ² / 2 for normals θ off opposite
        let denominator = 1.0 + c;
        if denominator <= 0.5 * sine_squared {
            return 0.0;
        }
        own / denominator
    } else {
        // 1 - c² = sin²(θ) for normals θ apart
        let denominator = 1.0 - c * c;
        if denominator <= sine_squared {
            return 0.0;
        }
        (other - c * own) / denominator
    };
    beta * n_other.dot(outward)
}

/// The signed reach past `eu`'s edge at `parameter` (module docs), with `eu`'s outward
/// binormal there, and whether the edge has a neighbour (zero reach without).
fn edge_reach(
    k: &Kernel,
    b: &mut Brep,
    eu: EdgeuseId,
    parameter: f64,
    offset: f64,
    unmoved: &[FaceId],
    tangent_sine: f64,
) -> Result<(End, bool)> {
    let options = eu3::BinormalOptions::default();
    let own = eu3::evaluate_binormal(k, b, eu, parameter, false, &options)?;
    let outward = -own.binormal;
    // A seam's radial is on the face itself (its offset continues across), a lamina
    // edge's is its mate on the face's other side (its offset is closed by a side wall).
    let own_face = eu::face(b, eu)?;
    let neighbour = match eu::radial(b, eu)? {
        Some(r) if r != eu && eu::face(b, r)? != own_face => Some(r),
        _ => None,
    };
    let Some(radial) = neighbour else {
        let end = End {
            reach: 0.0,
            outward,
        };
        return Ok((end, false));
    };
    let other_offset = match eu::face(b, radial)? {
        Some(f) if unmoved.contains(&f) => 0.0,
        _ => offset,
    };
    let other = eu3::evaluate_binormal(k, b, radial, parameter, false, &options)?;
    let n_own = own.face_normal.unitize()?;
    let n_other = other.face_normal.unitize()?;
    let end = End {
        reach: signed_reach(n_own, n_other, outward, offset, other_offset, tangent_sine),
        outward,
    };
    Ok((end, true))
}

/// The distance from a vertex of the foot of its offset corner on the face's tangent plane
/// (module docs), from the reaches and outward binormals of the edge uses meeting there.
/// Where the edges run on tangent to each other (within `tangent_sine`, the sine of the
/// tangency tolerance) the corner is no corner: the offset edges run on into each other, and
/// the larger reach stands (the tangent lines' crossing, far off for nearly parallel lines,
/// is not where curved edges' offsets meet).
fn corner_reach(into: End, out: End, tangent_sine: f64) -> f64 {
    let g = into.outward.dot(out.outward);
    let det = 1.0 - g * g;
    if det <= (tangent_sine * tangent_sine).max(EFF_ZERO) {
        return into.reach.abs().max(out.reach.abs());
    }
    let a = (into.reach - g * out.reach) / det;
    let c = (out.reach - g * into.reach) / det;
    sqrt((a * a + c * c + 2.0 * a * c * g).max(0.0))
}

/// How far past its face the surface of `faceuse` must be extended for the offsets of its
/// neighbours to meet its own, the face offset by `offset` (signed, along the face use's
/// normal) and each neighbour by the same, except the `unmoved` faces, which stay where they
/// are (module docs): the largest distance from the face of the feet of the meetings, sampled
/// along each edge at its sector angle and at each corner (edges meeting within
/// `tangency_tol_radians` of tangent make no corner); `None` for a face with no
/// neighbour (every edge a lamina edge or a seam), whose offset meets no other.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for an offset that is not finite or a vanishing face normal;
/// those of [`eu3::evaluate_binormal`]; [`SolidError::NotFound`] for a dangling id.
pub fn extension_reach(
    k: &Kernel,
    b: &mut Brep,
    faceuse: FaceuseId,
    offset: f64,
    unmoved: &[FaceId],
    tangency_tol_radians: f64,
) -> Result<Option<f64>> {
    if !offset.is_finite() {
        return Err(SolidError::invalid_input(OP, "the offset is not finite"));
    }
    let tangent_sine = tangency_tol_radians.abs().min(crate::math::PI / 2.0).sin();
    let mut reach = 0.0_f64;
    let mut met = false;
    for lu in eu::faceuse_loopuses(b, faceuse, false, None)? {
        let mut ends: Vec<(End, End)> = Vec::new();
        for edgeuse in loopuse::edgeuses(b, lu, None, None)? {
            let Some(e) = eu::edge(b, edgeuse)? else {
                continue;
            };
            let interval = edge::interval(b, e)?;
            let (lo, hi) = (interval.min(), interval.max());
            let mut first = None;
            let mut last = None;
            for i in 0..=EDGE_SAMPLES {
                let parameter = lo + (hi - lo) * f64::from(i) / f64::from(EDGE_SAMPLES);
                let (end, neighbour) =
                    edge_reach(k, b, edgeuse, parameter, offset, unmoved, tangent_sine)?;
                met |= neighbour;
                reach = reach.max(end.reach.abs());
                if i == 0 {
                    first = Some(end);
                }
                last = Some(end);
            }
            let (Some(first), Some(last)) = (first, last) else {
                continue;
            };
            // The edge use runs from its start to its end vertex along the loop.
            ends.push(match eu::orientation(b, edgeuse)? {
                OrientType::Opposite => (last, first),
                _ => (first, last),
            });
        }
        // Each edge use's end meets the next one's start (the loop closes).
        let next = ends.iter().cycle().skip(1);
        for (&(_, into), &(out, _)) in ends.iter().zip(next) {
            reach = reach.max(corner_reach(into, out, tangent_sine));
        }
    }
    Ok(met.then_some(reach))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Point3;
    use crate::topology::brep::p1 as bp1;

    fn box_faceuses(k: &mut Kernel, inset: bool) -> (Brep, Vec<(FaceId, FaceuseId)>) {
        let brep = crate::primitive::api_primitives::create_box(
            k,
            Point3::new(0.0, 0.0, 0.0),
            1.0,
            2.0,
            3.0,
        )
        .unwrap();
        let faceuses = bp1::faces(&brep, None)
            .unwrap()
            .into_iter()
            .map(|f| {
                let fu = super::super::p2::select_offset_faceuse(&brep, f, false, inset, OP);
                (f, fu.unwrap())
            })
            .collect();
        (brep, faceuses)
    }

    /// Across a box's edges (sector angle a right angle) the offsets meet `d` past each
    /// edge, and at its corners `d` past both: `d·√2` from the vertex. Offset inward they meet
    /// as far inside: the same distance from the face.
    #[test]
    fn a_box_offset_either_way_reaches_its_corners_offset() {
        for inset in [false, true] {
            let mut k = Kernel::default();
            let (mut brep, faceuses) = box_faceuses(&mut k, inset);
            for (_, fu) in faceuses {
                let reach = extension_reach(&k, &mut brep, fu, 0.25, &[], 1e-3)
                    .unwrap()
                    .unwrap();
                assert!((reach - 0.25 * sqrt(2.0)).abs() < 1e-12, "{reach}");
            }
        }
    }

    /// A box shelled open at one face: that face stays where it is, and each wall's offset
    /// meets it at the wall's edge (a right angle, `β = 0`). A wall's corner on the open face
    /// lies `d` from its vertex, and its other corners `d·√2`.
    #[test]
    fn an_unmoved_face_is_met_where_it_stands() {
        let mut k = Kernel::default();
        let (mut brep, faceuses) = box_faceuses(&mut k, true);
        let (open, _) = faceuses[0];
        for &(_, fu) in faceuses.iter().filter(|(f, _)| *f != open) {
            let reach = extension_reach(&k, &mut brep, fu, 0.25, &[open], 1e-3)
                .unwrap()
                .unwrap();
            assert!((reach - 0.25 * sqrt(2.0)).abs() < 1e-12, "{reach}");
        }
    }

    /// The signed reach: `d·cot(α/2)` for equal offsets (a right angle `d`, a tangent edge
    /// `0`), `0` for a knife edge (no meeting); an unmoved neighbour at a right angle is met at
    /// the edge, at 60° between normals `dF·c·sin / (1 − c²)` inside.
    #[test]
    fn signed_reach_is_the_foot_of_the_offsets_meeting() {
        let up = Vec3::new(0.0, 0.0, 1.0);
        let out = Vec3::X;
        assert!((signed_reach(up, Vec3::X, out, 0.5, 0.5, 1e-3) - 0.5).abs() < 1e-15);
        assert!((signed_reach(up, -Vec3::X, out, 0.5, 0.5, 1e-3) + 0.5).abs() < 1e-15);
        assert_eq!(signed_reach(up, up, out, 0.5, 0.5, 1e-3), 0.0);
        assert_eq!(signed_reach(up, -up, out, 0.5, 0.5, 1e-3), 0.0);
        assert_eq!(signed_reach(up, up, out, 0.5, 0.0, 1e-3), 0.0);
        assert!(signed_reach(up, Vec3::X, out, 0.5, 0.0, 1e-3).abs() < 1e-15);
        let half = sqrt(3.0) / 2.0;
        let tilted = Vec3::new(half, 0.0, 0.5);
        let expected = -0.5 * 0.5 * half / (1.0 - 0.25);
        assert!((signed_reach(up, tilted, out, 0.5, 0.0, 1e-3) - expected).abs() < 1e-15);
        // within the tangency tolerance of tangent, offsets 0.5 apart never meet near
        let nearly = Vec3::new(1e-4, 0.0, (1.0 - 1e-8_f64).sqrt());
        assert_eq!(signed_reach(up, nearly, out, 0.5, 0.0, 1e-3), 0.0);
        assert!(signed_reach(up, nearly, out, 0.5, 0.0, 1e-5).abs() > 1e3);
    }

    /// The corner's foot: perpendicular edges each reached `1` and `2` past put the corner
    /// at `(1, 2)`; edges at 60° reached `d` past both put it on the bisector, `d / cos 30°`
    /// from the vertex, as reached `d` inside; parallel edges, or edges within the tangency
    /// tolerance of tangent, take the larger reach.
    #[test]
    fn corner_reach_is_the_foot_of_the_offset_corner() {
        let end = |reach: f64, outward: Vec3| End { reach, outward };
        let square = corner_reach(end(1.0, Vec3::X), end(2.0, Vec3::Y), 1e-3);
        assert!((square - sqrt(5.0)).abs() < 1e-12);
        let half = sqrt(3.0) / 2.0;
        let sixty = Vec3::new(0.5, half, 0.0);
        let acute = corner_reach(end(1.0, Vec3::X), end(1.0, sixty), 1e-3);
        assert!((acute - 1.0 / half).abs() < 1e-12, "{acute}");
        let inside = corner_reach(end(-1.0, Vec3::X), end(-1.0, sixty), 1e-3);
        assert!((inside - 1.0 / half).abs() < 1e-12, "{inside}");
        let parallel = corner_reach(end(1.0, Vec3::X), end(-3.0, Vec3::X), 1e-3);
        assert_eq!(parallel, 3.0);
        // edges 1e-4 off tangent, within a tolerance of 1e-3: no corner, not 1e4 off
        let nearly = Vec3::new((1.0 - 1e-8_f64).sqrt(), 1e-4, 0.0);
        let tangent = corner_reach(end(1.0, Vec3::X), end(2.0, nearly), 1e-3);
        assert_eq!(tangent, 2.0);
    }

    /// A moved input is refused: an offset that is not finite.
    #[test]
    fn a_non_finite_offset_is_refused() {
        let mut k = Kernel::default();
        let (mut brep, faceuses) = box_faceuses(&mut k, false);
        assert!(extension_reach(&k, &mut brep, faceuses[0].1, f64::NAN, &[], 1e-3).is_err());
    }
}
