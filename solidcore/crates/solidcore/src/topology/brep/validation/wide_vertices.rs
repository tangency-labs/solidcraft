//! Vertices a boolean leaves with a tolerance `Test #82` refuses.
//!
//! A cut whose outline grazes a slanted face leaves a vertex at one edge's end with the other
//! edges' ends up to the whole gap away; the tolerance update then grows the vertex's tolerance
//! to twice that gap, past [`LARGE_SMALL_SIZE_RATIO`] times the B-rep's, and the check refuses
//! the result. [`update_tolerances_after_boolean`] first centres such a vertex on its edges'
//! ends where that fits, then runs [`validate_and_update_tolerances`] as before.

use super::assert_valid::{validate_and_update_tolerances, CheckLevel, ToleranceGaps};
use crate::error::{Result, SolidError};
use crate::geom::tol::LARGE_SMALL_SIZE_RATIO;
use crate::kernel::Kernel;
use crate::topology::brep::p1;
use crate::topology::{edge, Brep, EdgeId, VertexId};
use crate::Point3;

/// The tolerance update a boolean's result gets (`validate_and_update_tolerances` at level 2,
/// tolerances only grown), after [`centre_wide_vertices`].
///
/// # Errors
/// Those of [`centre_wide_vertices`] and [`validate_and_update_tolerances`].
pub fn update_tolerances_after_boolean(k: &mut Kernel, b: &mut Brep) -> Result<ToleranceGaps> {
    centre_wide_vertices(k, b)?;
    validate_and_update_tolerances(k, b, false, None, CheckLevel::Level2 as u8)
}

/// Moves each vertex of `b` whose tolerance SMLib's check refuses (not under
/// [`LARGE_SMALL_SIZE_RATIO`] times the B-rep's, `Test #82`) to the centre of the box round its
/// edges' ends, where that brings the tolerances it needs inside the limit, and sets its
/// tolerance to what they need.
///
/// The boolean grows a vertex's tolerance to twice its largest gap. A cut whose outline meets a
/// face at a shallow angle places the vertex at one edge's end with the other edges' ends up to
/// the whole gap away, so twice that gap can pass the limit although the edges end well inside
/// it. The box's centre is within half its diagonal (the ends' spread) of every end; the edges'
/// tolerances are grown to the spread, and the vertex's to twice its new largest gap
/// (vertex/face gaps included), the spread and its edges' tolerances, whichever is largest.
///
/// A move is kept only when that is inside the limit and every edge's 3D curve end and every
/// edge use's UV trim end, on its face's surface, lies within the vertex's and edge's
/// tolerances, as the validator requires of the curve ends. Otherwise, or when a gap cannot be
/// computed, the vertex and the tolerances are put back as they were, and the tolerance update
/// that follows decides as it did without this pass. An error the kernel never swallows
/// (cancelled, unported, face trims unavailable) is put back the same way and, once that put-back
/// succeeds, returned; otherwise the pass fails only on a vertex or edge missing from `b`, a broken
/// result that the check would refuse anyway.
pub(crate) fn centre_wide_vertices(k: &mut Kernel, b: &mut Brep) -> Result<()> {
    let limit = LARGE_SMALL_SIZE_RATIO * b.tolerance();
    for v in p1::vertices(b, None)? {
        // the check wants a vertex's tolerance strictly under the limit
        if b.vertex(v)?.tolerance < limit {
            continue;
        }
        let before = Tolerances::of(b, v)?;
        match centre_vertex(k, b, v, limit) {
            Ok(true) => {}
            Err(
                e @ (SolidError::Cancelled { .. }
                | SolidError::Unported { .. }
                | SolidError::FaceTrimsUnavailable { .. }),
            ) => {
                before.restore(b)?;
                return Err(e);
            }
            Ok(false) | Err(_) => before.restore(b)?,
        }
    }
    Ok(())
}

/// One vertex of [`centre_wide_vertices`]: `true` when it was moved and its tolerances set,
/// `false` (or an error) when the move does not fit and the caller puts it back.
fn centre_vertex(k: &mut Kernel, b: &mut Brep, v: VertexId, limit: f64) -> Result<bool> {
    use crate::geom::tol::zone_tol3d;
    use crate::heal::gap::Gap;
    use crate::topology::{vertex, vertexuse};
    use crate::Vec3;

    // a cancelled job stops at each vertex, before it is touched and before its ends are checked
    k.check_cancelled("centre_wide_vertices")?;
    let mut ends = Vec::new();
    for vu in vertex::vertexuses(b, v)? {
        if let Some(g) = vertexuse::vertex_edge_gap(k, b, vu, true)? {
            // the gap's second end point is on the edge's curve
            ends.push(Gap::VertexEdge(g).gap_end_points(k, b)?.1);
        }
    }
    // one end gives no spread to centre on
    let Some((&first, rest)) = ends.split_first() else {
        return Ok(false);
    };
    if rest.is_empty() {
        return Ok(false);
    }
    let (mut lo, mut hi) = (first, first);
    for p in rest {
        lo = Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
        hi = Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
    }
    // the edges' ends stay where they are: their spread is the gap between the edges, which
    // the edges' tolerances must cover as well as the vertex's
    let spread = zone_tol3d(k, lo.distance(hi));
    b.vertex_mut(v)?.point = (lo + hi) / 2.0;
    let gap = largest_gap(k, b, v)?;
    let need = zone_tol3d(k, 2.0 * gap).max(spread);
    let edges = vertex::edges(b, v, None)?;
    let mut widest = b.tolerance();
    for &e in &edges {
        widest = widest.max(b.edge(e)?.tolerance);
    }
    let tolerance = need.max(widest);
    if tolerance >= limit {
        return Ok(false);
    }
    for &e in &edges {
        edge::set_tolerance(b, e, spread, true, true)?;
    }
    b.vertex_mut(v)?.tolerance = tolerance;
    k.check_cancelled("centre_wide_vertices")?;
    ends_within(k, b, v)
}

/// The largest vertex/edge or vertex/face gap of `v`'s uses, recomputed (their caches refreshed
/// for where the vertex is now); 0 without gaps.
fn largest_gap(k: &mut Kernel, b: &mut Brep, v: VertexId) -> Result<f64> {
    use crate::topology::{vertex, vertexuse};

    let mut most = 0.0_f64;
    for vu in vertex::vertexuses(b, v)? {
        if let Some(g) = vertexuse::vertex_edge_gap(k, b, vu, true)? {
            most = most.max(g.gap_3d);
        }
        if let Some(g) = vertexuse::vertex_face_gap(k, b, vu, true)? {
            most = most.max(g.gap_3d);
        }
    }
    Ok(most)
}

/// Whether every end at `v` of its edges' 3D curves, and of their uses' UV trim curves on their
/// faces' surfaces, is within the vertex's tolerance plus the edge's. A trim runs with its edge
/// (its domain's start at the edge's start), as the validator compares them.
pub(crate) fn ends_within(k: &Kernel, b: &Brep, v: VertexId) -> Result<bool> {
    use crate::topology::{edgeuse, vertex};
    use crate::Point2;

    let at = b.vertex(v)?.point;
    let reach = b.vertex(v)?.tolerance;
    for e in vertex::edges(b, v, None)? {
        let (start, end) = edge::start_end_vertices(b, e)?;
        let ivl = edge::interval(b, e)?;
        let reach = reach + b.edge(e)?.tolerance;
        let Some(curve) = edge::curve(b, e)? else {
            continue;
        };
        for (here, t) in [(start, ivl.min()), (end, ivl.max())] {
            if here == Some(v) && curve.evaluate_point(k, t)?.distance(at) > reach {
                return Ok(false);
            }
        }
        for &eu in &b.edge(e)?.edgeuses {
            let Some(trim) = b.edgeuse(eu)?.uv_trim_curve.as_ref() else {
                continue;
            };
            let Some(f) = edgeuse::p1::face(b, eu)? else {
                continue;
            };
            let Some(surface) = b.face(f)?.surface.as_ref() else {
                continue;
            };
            let domain = trim.natural_interval();
            for (here, t) in [(start, domain.min()), (end, domain.max())] {
                if here != Some(v) {
                    continue;
                }
                let uv = trim.evaluate_point(k, t)?;
                if surface
                    .evaluate_point(k, Point2::new(uv.x, uv.y))?
                    .distance(at)
                    > reach
                {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

/// A vertex's point and tolerance, its edges' tolerances and their other vertices' (which
/// growing an edge's raises), to put back when [`centre_vertex`] does not keep its move. The
/// vertex's cached gaps are cleared on the way back, so they are computed again where it is.
struct Tolerances {
    vertex: VertexId,
    point: Point3,
    vertices: Vec<(VertexId, f64)>,
    edges: Vec<(EdgeId, f64)>,
}

impl Tolerances {
    fn of(b: &Brep, v: VertexId) -> Result<Self> {
        use crate::topology::vertex;

        let mut vertices = vec![(v, b.vertex(v)?.tolerance)];
        let mut edges = Vec::new();
        for e in vertex::edges(b, v, None)? {
            edges.push((e, b.edge(e)?.tolerance));
            if let Some(o) = edge::other_vertex(b, e, Some(v))? {
                vertices.push((o, b.vertex(o)?.tolerance));
            }
        }
        Ok(Self {
            vertex: v,
            point: b.vertex(v)?.point,
            vertices,
            edges,
        })
    }

    fn restore(&self, b: &mut Brep) -> Result<()> {
        use crate::topology::vertex;

        b.vertex_mut(self.vertex)?.point = self.point;
        for &(e, t) in &self.edges {
            b.edge_mut(e)?.tolerance = t;
        }
        for &(v, t) in &self.vertices {
            b.vertex_mut(v)?.tolerance = t;
        }
        for vu in vertex::vertexuses(b, self.vertex)? {
            let u = b.vertexuse_mut(vu)?;
            u.vertex_edge_gap.reset();
            u.vertex_face_gap.reset();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{centre_wide_vertices, ends_within};
    use crate::error::Result;
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::geom::vector3d::Vec3;
    use crate::kernel::Kernel;
    use crate::primitive::api_primitives::create_box;
    use crate::topology::Brep;
    use crate::Point3;

    /// `f` on a 10 mm box at the origin, in a kernel of its own.
    fn with_box(f: impl FnOnce(&mut Kernel, &mut Brep) -> Result<()>) -> Result<()> {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0)?;
        f(&mut k, &mut b)
    }

    /// Every vertex's point and tolerance and every edge's tolerance, to compare bit for bit.
    fn tolerance_state(b: &Brep) -> Vec<u64> {
        let mut s = Vec::new();
        for &v in b.vertex_list() {
            let r = b.vertex(v).unwrap();
            s.extend([r.point.x, r.point.y, r.point.z, r.tolerance].map(f64::to_bits));
        }
        for &e in b.edge_list() {
            s.push(b.edge(e).unwrap().tolerance.to_bits());
        }
        s
    }

    /// A box corner pushed 4 µm off its edges' ends with a tolerance past `Test #82`'s limit, as a
    /// grazing cut leaves one: `centre_wide_vertices` puts it back on the ends and sets a tolerance
    /// inside the limit that still covers every edge end there, 3D curve and UV trim.
    #[test]
    fn a_wide_vertex_is_centred_on_its_edges_ends() {
        with_box(|k, b| {
            let limit = crate::geom::tol::LARGE_SMALL_SIZE_RATIO * b.tolerance();
            let v = b.vertex_list()[0];
            let corner = b.vertex(v)?.point;
            b.vertex_mut(v)?.point = corner + Vec3::new(0.004, 0.0, 0.0);
            b.vertex_mut(v)?.tolerance = 5.0 * limit;
            centre_wide_vertices(k, b)?;
            let r = b.vertex(v)?;
            assert!(
                r.point.distance(corner) < 1e-9,
                "{:?} for {corner:?}",
                r.point
            );
            assert!(r.tolerance <= limit, "{} > {limit}", r.tolerance);
            for &w in b.vertex_list() {
                assert!(b.vertex(w)?.tolerance <= limit);
                assert!(ends_within(k, b, w)?, "an edge end outside vertex {w:?}");
            }
            Ok(())
        })
        .unwrap();
    }

    /// The same corner with an edge already wider than the limit: no tolerance it could get would
    /// pass, so the vertex and every tolerance stay exactly as they were.
    #[test]
    fn a_vertex_whose_move_does_not_fit_is_left_as_it_was() {
        with_box(|k, b| {
            let limit = crate::geom::tol::LARGE_SMALL_SIZE_RATIO * b.tolerance();
            let v = b.vertex_list()[0];
            let corner = b.vertex(v)?.point;
            b.vertex_mut(v)?.point = corner + Vec3::new(0.004, 0.0, 0.0);
            b.vertex_mut(v)?.tolerance = 5.0 * limit;
            let e = crate::topology::vertex::edges(b, v, None)?[0];
            b.edge_mut(e)?.tolerance = 2.0 * limit;
            let before = tolerance_state(b);
            centre_wide_vertices(k, b)?;
            assert_eq!(tolerance_state(b), before);
            Ok(())
        })
        .unwrap();
    }

    /// A body whose vertices are all inside the limit is not touched.
    #[test]
    fn a_body_inside_the_limit_is_unchanged() {
        with_box(|k, b| {
            let before = tolerance_state(b);
            centre_wide_vertices(k, b)?;
            assert_eq!(tolerance_state(b), before);
            Ok(())
        })
        .unwrap();
    }

    /// Corner 0 of a 10 mm box, the box's `Test #82` limit, and the vertex.
    fn corner_and_limit(b: &Brep) -> (crate::topology::VertexId, Point3, f64) {
        let v = b.vertex_list()[0];
        let limit = crate::geom::tol::LARGE_SMALL_SIZE_RATIO * b.tolerance();
        (v, b.vertex(v).unwrap().point, limit)
    }

    /// Moves the UV trim of one use of an edge at `v` by `du` along u, every 3D curve left as it
    /// is; how far its points moved on its face's (planar) surface. Panics without a trim there.
    fn shift_a_trim(
        k: &Kernel,
        b: &mut Brep,
        v: crate::topology::VertexId,
        du: f64,
    ) -> std::result::Result<f64, crate::SolidError> {
        use crate::topology::{edgeuse, vertex};
        use crate::{Point2, Point3};

        for e in vertex::edges(b, v, None)? {
            for eu in b.edge(e)?.edgeuses.clone() {
                let Some(f) = edgeuse::p1::face(b, eu)? else {
                    continue;
                };
                let (Some(trim), Some(surface)) = (
                    b.edgeuse(eu)?.uv_trim_curve.as_ref(),
                    b.face(f)?.surface.as_ref(),
                ) else {
                    continue;
                };
                let t = trim.natural_interval().min();
                let was = trim.evaluate_point(k, t)?;
                let was = surface.evaluate_point(k, Point2::new(was.x, was.y))?;
                let along_u = Axis2Placement::new(
                    Point3::new(du, 0.0, 0.0),
                    Vec3::new(1.0, 0.0, 0.0),
                    Vec3::new(0.0, 1.0, 0.0),
                );
                let trim = b
                    .edgeuse_mut(eu)?
                    .uv_trim_curve
                    .as_mut()
                    .expect("the trim read above");
                trim.transform(k, &along_u, None)?;
                let now = trim.evaluate_point(k, t)?;
                let surface = b.face(f)?.surface.as_ref().expect("the surface read above");
                let moved = surface
                    .evaluate_point(k, Point2::new(now.x, now.y))?
                    .distance(was);
                if moved > 0.5 * du.abs() {
                    return Ok(moved);
                }
                // the trim ends on the surface's edge, where it is clamped: shift it inward instead
                let back = Axis2Placement::new(
                    Point3::new(-2.0 * du, 0.0, 0.0),
                    Vec3::new(1.0, 0.0, 0.0),
                    Vec3::new(0.0, 1.0, 0.0),
                );
                let trim = b
                    .edgeuse_mut(eu)?
                    .uv_trim_curve
                    .as_mut()
                    .expect("the trim read above");
                trim.transform(k, &back, None)?;
                let now = trim.evaluate_point(k, t)?;
                let surface = b.face(f)?.surface.as_ref().expect("the surface read above");
                return Ok(surface
                    .evaluate_point(k, Point2::new(now.x, now.y))?
                    .distance(was));
            }
        }
        panic!("no edge at the corner has a UV trim");
    }

    /// `ends_within` against an independent distance: with every edge end at a box corner, the
    /// vertex moved off by less than its tolerance plus its tightest edge's has them within, and by
    /// more than its tolerance plus its widest edge's has them outside.
    #[test]
    fn ends_within_measures_each_end_against_both_tolerances() {
        with_box(|k, b| {
            let (v, corner, _) = corner_and_limit(b);
            let edge_tols: Vec<f64> = crate::topology::vertex::edges(b, v, None)?
                .iter()
                .map(|&e| b.edge(e).unwrap().tolerance)
                .collect();
            let vt = b.vertex(v)?.tolerance;
            let tightest = vt + edge_tols.iter().copied().fold(f64::MAX, f64::min);
            let widest = vt + edge_tols.iter().copied().fold(0.0, f64::max);
            b.vertex_mut(v)?.point = corner + Vec3::new(0.99 * tightest, 0.0, 0.0);
            assert!(
                ends_within(k, b, v)?,
                "{} inside {tightest}",
                0.99 * tightest
            );
            b.vertex_mut(v)?.point = corner + Vec3::new(1.01 * widest, 0.0, 0.0);
            assert!(!ends_within(k, b, v)?, "{} outside {widest}", 1.01 * widest);
            Ok(())
        })
        .unwrap();
    }

    /// A UV trim whose end is off the vertex is caught although every 3D curve end is on it.
    #[test]
    fn ends_within_sees_a_trim_end_off_its_vertex() {
        with_box(|k, b| {
            let (v, _, limit) = corner_and_limit(b);
            assert!(ends_within(k, b, v)?);
            let moved = shift_a_trim(k, b, v, 0.05)?;
            assert!(moved > 2.0 * limit, "the trim moved {moved}");
            assert!(!ends_within(k, b, v)?);
            Ok(())
        })
        .unwrap();
    }

    /// Corner 0 pushed 4 µm off with a tolerance five times the limit, and one of its edges ending
    /// 3 µm short of it, so a kept move spreads that edge's tolerance and its far vertex's; with
    /// `trim_off` a UV trim at the corner is also moved 50 µm. The corner vertex, the far vertex and
    /// the far vertex's tolerance before.
    fn spread_corner(
        k: &Kernel,
        b: &mut Brep,
        trim_off: bool,
    ) -> std::result::Result<
        (crate::topology::VertexId, crate::topology::VertexId, f64),
        crate::SolidError,
    > {
        use crate::topology::{edge, vertex};

        let (v, corner, limit) = corner_and_limit(b);
        let e = vertex::edges(b, v, None)?[0];
        let ivl = edge::interval(b, e)?;
        let length = {
            let c = edge::curve(b, e)?.expect("a box edge has a curve");
            c.evaluate_point(k, ivl.min())?
                .distance(c.evaluate_point(k, ivl.max())?)
        };
        let dt = 0.003 * (ivl.max() - ivl.min()) / length;
        let mut short = ivl;
        if edge::start_end_vertices(b, e)?.0 == Some(v) {
            short.set_min(ivl.min() + dt)?;
        } else {
            short.set_max(ivl.max() - dt)?;
        }
        edge::set_interval(b, e, &short)?;
        let c = edge::curve(b, e)?.expect("a box edge has a curve");
        let nearest = c
            .evaluate_point(k, short.min())?
            .distance(corner)
            .min(c.evaluate_point(k, short.max())?.distance(corner));
        assert!(
            (nearest - 0.003).abs() < 1e-9,
            "the edge ends {nearest} from the corner"
        );
        b.vertex_mut(v)?.point = corner + Vec3::new(0.004, 0.0, 0.0);
        b.vertex_mut(v)?.tolerance = 5.0 * limit;
        if trim_off {
            let moved = shift_a_trim(k, b, v, 0.05)?;
            assert!(moved > 2.0 * limit, "the trim moved {moved}");
        }
        let far = edge::other_vertex(b, e, Some(v))?.expect("a box edge has two vertices");
        Ok((v, far, b.vertex(far)?.tolerance))
    }

    /// The control for the next test: without the trim off, the move is kept and spreads the
    /// shortened edge's tolerance to its far vertex.
    #[test]
    fn a_kept_move_spreads_its_edges_tolerances() {
        with_box(|k, b| {
            let (v, corner, limit) = corner_and_limit(b);
            let (_, far, far_was) = spread_corner(k, b, false)?;
            centre_wide_vertices(k, b)?;
            assert!(
                b.vertex(v)?.point.distance(corner) < 0.002,
                "{:?}",
                b.vertex(v)?.point
            );
            assert!(
                b.vertex(far)?.tolerance > far_was,
                "far vertex {} not spread",
                b.vertex(far)?.tolerance
            );
            for &w in b.vertex_list() {
                assert!(b.vertex(w)?.tolerance < limit);
                assert!(ends_within(k, b, w)?, "an edge end outside vertex {w:?}");
            }
            Ok(())
        })
        .unwrap();
    }

    /// The same corner with a UV trim end 50 µm off: the move reaches the spread of the edges'
    /// tolerances and is then refused, so every point and tolerance (the far vertex's included) is
    /// put back bit for bit, and the corner's cached gaps are those of where it is again.
    #[test]
    fn a_move_refused_after_its_tolerances_spread_is_put_back() {
        use crate::topology::{vertex, vertexuse};

        with_box(|k, b| {
            let (v, far, far_was) = spread_corner(k, b, true)?;
            let before = tolerance_state(b);
            centre_wide_vertices(k, b)?;
            assert_eq!(tolerance_state(b), before);
            assert_eq!(b.vertex(far)?.tolerance.to_bits(), far_was.to_bits());
            for vu in vertex::vertexuses(b, v)? {
                let cached =
                    vertexuse::vertex_edge_gap(k, b, vu, false)?.map(|g| g.gap_3d.to_bits());
                let fresh = vertexuse::vertex_edge_gap(k, b, vu, true)?.map(|g| g.gap_3d.to_bits());
                assert_eq!(cached, fresh, "a stale vertex/edge gap");
                let cached =
                    vertexuse::vertex_face_gap(k, b, vu, false)?.map(|g| g.gap_3d.to_bits());
                let fresh = vertexuse::vertex_face_gap(k, b, vu, true)?.map(|g| g.gap_3d.to_bits());
                assert_eq!(cached, fresh, "a stale vertex/face gap");
            }
            Ok(())
        })
        .unwrap();
    }

    /// `Test #82` refuses a vertex tolerance equal to its limit, not only one over it: just under
    /// the limit the check reports nothing, at it test 82 fails for that vertex.
    #[test]
    fn the_check_refuses_a_vertex_tolerance_at_the_limit() {
        use crate::topology::brep::validation::{check_brep, CheckLevel};

        for (factor, refused) in [(0.999_999, false), (1.0, true)] {
            with_box(|k, b| {
                let (v, _, limit) = corner_and_limit(b);
                b.vertex_mut(v)?.tolerance = factor * limit;
                let c = check_brep(k, b, CheckLevel::Level2)?;
                let failed: Vec<u32> = c
                    .pointer_reports
                    .reports()
                    .iter()
                    .chain(c.reports.reports())
                    .filter(|r| !r.ok)
                    .map(|r| r.test_index as u32)
                    .collect();
                assert_eq!(
                    failed,
                    if refused { vec![82] } else { vec![] },
                    "at {factor} of the limit"
                );
                Ok(())
            })
            .unwrap();
        }
    }

    /// So the pass treats the limit itself as outside: a vertex at it is centred to a tolerance
    /// under it, and a move that would need exactly the limit is not kept.
    #[test]
    fn the_limit_itself_is_outside() {
        with_box(|k, b| {
            let (v, corner, limit) = corner_and_limit(b);
            b.vertex_mut(v)?.point = corner + Vec3::new(0.004, 0.0, 0.0);
            b.vertex_mut(v)?.tolerance = limit;
            centre_wide_vertices(k, b)?;
            assert!(b.vertex(v)?.point.distance(corner) < 1e-9);
            assert!(
                b.vertex(v)?.tolerance < limit,
                "{} at {limit}",
                b.vertex(v)?.tolerance
            );
            Ok(())
        })
        .unwrap();
        with_box(|k, b| {
            let (v, corner, limit) = corner_and_limit(b);
            b.vertex_mut(v)?.point = corner + Vec3::new(0.004, 0.0, 0.0);
            b.vertex_mut(v)?.tolerance = 5.0 * limit;
            let e = crate::topology::vertex::edges(b, v, None)?[0];
            b.edge_mut(e)?.tolerance = limit;
            let before = tolerance_state(b);
            centre_wide_vertices(k, b)?;
            assert_eq!(tolerance_state(b), before);
            Ok(())
        })
        .unwrap();
    }

    /// Cancelled at any poll of the pass, on a move that would be kept and spreads its edges'
    /// tolerances: the pass returns `Cancelled`, never a quiet fallback, and puts every point and
    /// tolerance back bit for bit, wherever the cancellation fell (before or after the spread).
    #[test]
    fn a_cancelled_pass_returns_cancelled_and_puts_everything_back() {
        use crate::kernel::CancelToken;

        // (polls made, and for a pass that failed: whether it was `Cancelled`, whether all is back)
        let polls = |n: u64| {
            let mut out = None;
            with_box(|k, b| {
                spread_corner(k, b, false)?;
                let before = tolerance_state(b);
                let token = CancelToken::after_polls(n);
                let was = k.cancel.replace(token.clone());
                let r = centre_wide_vertices(k, b);
                k.cancel = was;
                let made = token.polls().unwrap();
                out = Some(match r {
                    Ok(()) => (made, None),
                    Err(e) => (made, Some((e.is_cancelled(), tolerance_state(b) == before))),
                });
                Ok(())
            })
            .unwrap();
            out.unwrap()
        };
        let (all, finished) = polls(u64::MAX);
        assert!(finished.is_none(), "uncancelled, the pass runs through");
        assert!(all > 0, "the pass never polls its kernel");
        for n in 1..=all {
            let (_, r) = polls(n);
            assert_eq!(r, Some((true, true)), "cancelled at poll {n} of {all}");
        }
    }
}
