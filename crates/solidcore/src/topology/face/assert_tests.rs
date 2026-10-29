//! The level-2 `SmFace::AssertValid` tests: coincident face vertices (test 32),
//! coincident face edges (test 33) and face loops meeting in UV (test 38).
//!
//! Upstream: `source/SMLib/src/SmFace.cpp`, `source/SMLib/inc/SmFace.h`.
//!
//! | Rust                            | Upstream                         |
//! |---------------------------------|----------------------------------|
//! | [`has_coincident_vertices`]     | `SmFace::HasCoincidentVertices`  |
//! | [`has_coincident_edges`]        | `SmFace::HasCoincidentEdges`     |
//! | `find_coincident_verts` (private) | `sm_FindCoincidentVerts`       |
//! | [`assert_test_face32`]          | `sm_AssertTestFace32`            |
//! | [`assert_test_face33`]          | `sm_AssertTestFace33`            |
//! | [`assert_test_face38`]          | `sm_AssertTestFace38`            |
//! | `do_loops_intersect_in_uv` (private) | `sm_DoLoopsIntersectInUV`   |
//!
//! Deviations:
//! - Out-params become return values: [`CoincidentVertices`] and [`CoincidentEdges`]
//!   hold the "to survive" / "to delete" lists. `HasCoincidentVertices` quits after the
//!   first pair only when the caller passes no survive list (upstream tests
//!   `pOptToSurviveVerts == NULL` twice); [`has_coincident_vertices`] takes that as
//!   `quit_early`. `HasCoincidentEdges` always passes its own lists to
//!   `HasCoincidentVertices`, and quits early when no edge survive list is passed
//!   (`quit_early` again). `sm_AssertTestFace32/33` pass none, so both quit early.
//! - `GlobalCoincidenceChecker`'s status is ignored by `HasCoincidentEdges`: a failed
//!   check leaves `rbAdditionalWorkNeeded` at the `FALSE` it was set to on entry (every
//!   `SER` return of the checker precedes its assignment) and the solutions empty, so
//!   the edge pair counts as coincident. A failing [`Curve::global_coincidence_checker`]
//!   is read the same way; [`SolidError::Unported`] propagates.
//! - Edge ends are `Option`s: upstream compares `NULL` vertex pointers like any other.
//! - `sm_AssertTestFace38` dereferences the face surface; a face without one is
//!   [`SolidError::InvalidInput`] (`SmFace::AssertValid` only calls it with a surface).
//!   The surface is copied before the UV trim curves are built (building them mutates
//!   the B-rep, not the surface).

use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::solution_array::{ObjectKind, SolutionObject, SolutionType};
use crate::geom::tol::xsect_tol_3d_from_zones;
use crate::kernel::Kernel;
use crate::math::EFF_ZERO_SQ;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p3 as eu3;
use crate::topology::{edge, loopuse, vertex};
use crate::topology::{Brep, EdgeId, FaceId, LoopId, VertexId};

const OP: &str = "topology::face::assert_tests";

fn null(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

/// The coincident vertex pairs [`has_coincident_vertices`] found
/// (`pOptToSurviveVerts`, `pOptToDeleteVerts`): `to_delete[i]` lies within
/// `SM_EFF_ZERO` of `to_survive[i]`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoincidentVertices {
    /// Whether any pair was found (the return value).
    pub found: bool,
    /// The vertices with coincident partners, once per partner.
    pub to_survive: Vec<VertexId>,
    /// Each pair's partner.
    pub to_delete: Vec<VertexId>,
}

/// The coincident edge pairs [`has_coincident_edges`] found (`pOptToSurviveEdges`,
/// `pOptToDeleteEdges`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoincidentEdges {
    /// Whether any pair was found (the return value).
    pub found: bool,
    /// The edges with coincident partners, once per partner.
    pub to_survive: Vec<EdgeId>,
    /// Each pair's partner.
    pub to_delete: Vec<EdgeId>,
}

/// `SmFace::HasCoincidentVertices`: the pairs of face vertices (`GetVertices` order)
/// closer than `SM_EFF_ZERO`. A vertex already listed as a partner is not paired
/// again. With `quit_early` the search stops before the next outer vertex once a pair
/// was found (upstream: no survive list passed).
///
/// # Errors
///
/// Those of [`crate::topology::face::p4::vertices`] and [`vertex::point`].
pub fn has_coincident_vertices(
    b: &Brep,
    f: FaceId,
    quit_early: bool,
) -> Result<CoincidentVertices> {
    let vertices = crate::topology::face::p4::vertices(b, f, None, false, None)?;
    let mut out = CoincidentVertices::default();
    let mut done = false;

    // for every Vertex
    for (ii, &to_survive) in vertices.iter().enumerate() {
        if done {
            break;
        }

        // skip Vertices already on the pToDeleteVerts list
        if out.to_delete.contains(&to_survive) {
            continue;
        }
        let survive_point = vertex::point(b, to_survive)?;

        // for every other vertex - look for coincident partners
        for &to_delete in vertices.iter().skip(ii + 1) {
            let dist_sq = survive_point.distance_squared(vertex::point(b, to_delete)?);
            if dist_sq < EFF_ZERO_SQ {
                out.found = true;
                done = quit_early;
                out.to_survive.push(to_survive);
                out.to_delete.push(to_delete);
            }
        }
    }
    Ok(out)
}

/// `sm_FindCoincidentVerts`: `vertex` and every vertex the coincident pairs make it
/// coincident with (its surviving partner when it is a partner, then that vertex's
/// partners).
fn find_coincident_verts(
    vertex: Option<VertexId>,
    pairs: &CoincidentVertices,
) -> Vec<Option<VertexId>> {
    // init output
    let mut out = vec![vertex];
    let mut tgt = vertex;

    // See if pVertex is in the pToDeleteVerts
    if let Some(indx) = pairs.to_delete.iter().position(|&v| Some(v) == tgt) {
        // Make the surviving partner the TgtVertex
        tgt = pairs.to_survive.get(indx).copied();
        out.push(tgt);
    }

    // for every pToSurviveVerts vertex
    for (ii, &this_vertex) in pairs.to_survive.iter().enumerate() {
        if Some(this_vertex) == tgt {
            out.push(pairs.to_delete.get(ii).copied());
        }
    }
    out
}

/// The curve objects of the coincidence solutions (upstream names the edge curves):
/// their natural intervals.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CurveHandle(Extent1d);

impl SolutionObject for CurveHandle {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Curve
    }
    fn natural_interval(&self) -> Option<Extent1d> {
        Some(self.0)
    }
}

/// The edge curve (`GetCurve()`), dereferenced only by the coincidence check.
fn edge_curve(b: &Brep, e: EdgeId) -> Result<Option<&Curve>> {
    Ok(b.edge(e)?.curve.as_ref())
}

fn deref(c: Option<&Curve>) -> Result<&Curve> {
    c.ok_or_else(|| null("HasCoincidentEdges: edge without curve"))
}

/// `SmFace::HasCoincidentEdges`: the pairs of face edges (`GetEdges` order) whose end
/// vertices are the same or coincident ([`has_coincident_vertices`]) in either
/// orientation and whose curves `GlobalCoincidenceChecker` (snapping to the ends)
/// finds coincident within the edges' cross-section tolerance: no more work needed,
/// or one or two range solutions. An edge already listed as a partner is not paired
/// again. With `quit_early` the search stops before the next outer edge once a pair
/// was found (upstream: no survive list passed).
///
/// # Errors
///
/// Those of the face, edge and vertex accessors; [`SolidError::InvalidInput`] for an
/// edge without curve; [`SolidError::Unported`] from the coincidence check.
pub fn has_coincident_edges(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    quit_early: bool,
) -> Result<CoincidentEdges> {
    let edges = crate::topology::face::p4::edges(b, f, None, false, None)?;
    let mut out = CoincidentEdges::default();

    // look for coincident vertices
    let verts = has_coincident_vertices(b, f, false)?;

    let mut done = false;

    // for every Edge
    for (ii, &to_survive) in edges.iter().enumerate() {
        if done {
            break;
        }
        let survive_curve = edge_curve(b, to_survive)?;
        let survive_ivl = b.edge(to_survive)?.interval;
        let survive_zone_tol = edge::tolerance(b, to_survive)?;

        // skip edges already on the pToDeleteEdges list
        if out.to_delete.contains(&to_survive) {
            continue;
        }

        // Get list of all vertices coincident with pToSurviveEdge Start and End Vertices
        let start_verts = find_coincident_verts(edge::start_vertex(b, to_survive)?, &verts);
        let end_verts = find_coincident_verts(edge::end_vertex(b, to_survive)?, &verts);

        // for every other Edge - look for coincident partners
        for &to_delete in edges.iter().skip(ii + 1) {
            let delete_curve = edge_curve(b, to_delete)?;
            let delete_ivl = b.edge(to_delete)?.interval;
            let start_vertex = edge::start_vertex(b, to_delete)?;
            let end_vertex = edge::end_vertex(b, to_delete)?;
            let delete_zone_tol = edge::tolerance(b, to_delete)?;

            // only check edges which share common endPoint vertices
            if (start_verts.contains(&start_vertex) && end_verts.contains(&end_vertex))
                || (end_verts.contains(&start_vertex) && start_verts.contains(&end_vertex))
            {
                let xsect_tol = xsect_tol_3d_from_zones(k, survive_zone_tol, delete_zone_tol);
                let survive_curve = deref(survive_curve)?;
                let delete_curve = deref(delete_curve)?;
                let found = survive_curve.global_coincidence_checker(
                    k,
                    &survive_ivl,
                    delete_curve,
                    &delete_ivl,
                    xsect_tol,
                    true,
                    [
                        Some(CurveHandle(survive_curve.natural_interval())),
                        Some(CurveHandle(delete_curve.natural_interval())),
                    ],
                );
                // status ignored: a failed check leaves bAdditionalWorkNeeded FALSE
                let (additional_work_needed, types) = match found {
                    Ok(c) => (
                        c.additional_work_needed,
                        c.solutions
                            .iter()
                            .map(|s| s.solution_type)
                            .collect::<Vec<_>>(),
                    ),
                    Err(e) if e.is_unported() => return Err(e),
                    Err(_) => (false, Vec::new()),
                };

                // check for coincidence
                let range = |i: usize| types.get(i) == Some(&SolutionType::RangeOfValues);
                if !additional_work_needed
                    || (types.len() == 1 && range(0))
                    || (types.len() == 2 && range(0) && range(1))
                {
                    out.found = true;
                    done = quit_early;
                    out.to_survive.push(to_survive);
                    out.to_delete.push(to_delete);
                }
            }
        }
    }
    Ok(out)
}

/// `sm_AssertTestFace32` (no output lists): `true` when the face has no coincident
/// vertices.
///
/// # Errors
///
/// Those of [`has_coincident_vertices`].
pub fn assert_test_face32(b: &Brep, f: FaceId) -> Result<bool> {
    Ok(!has_coincident_vertices(b, f, true)?.found)
}

/// `sm_AssertTestFace33` (no output lists): `true` when the face has no coincident
/// edges.
///
/// # Errors
///
/// Those of [`has_coincident_edges`].
pub fn assert_test_face33(k: &Kernel, b: &Brep, f: FaceId) -> Result<bool> {
    Ok(!has_coincident_edges(k, b, f, true)?.found)
}

/// The UV trim curve `GetOrCreateUVTrimCurve(pUVCurve)` gives (built when missing);
/// `None` when the call fails or gives none (inconclusive).
fn uv_curve(k: &Kernel, b: &mut Brep, eu: crate::topology::EdgeuseId) -> Result<Option<Curve>> {
    let fetch = eu3::get_or_create_uv_trim_curve(k, b, eu, false, false, true)?;
    if fetch.status.is_err() {
        return Ok(None);
    }
    Ok(eu1::uv_trim_curve(b, eu)?.0.cloned())
}

/// `sm_DoLoopsIntersectInUV`: whether any UV trim curve of `loop1` meets one of `loop2`
/// (`GlobalCurveIntersect` at the surface zone tolerance mapped to UV at the domain
/// middle gives a solution). Missing trim curves are built; edge uses whose curve does
/// not build, and pairs whose intersection fails, are skipped.
fn do_loops_intersect_in_uv(
    k: &Kernel,
    b: &mut Brep,
    loop1: LoopId,
    loop2: LoopId,
    surface: &crate::surface::surface::Surface,
) -> Result<bool> {
    let edgeuses1 = loopuse::loop_edgeuses(b, loop1)?;
    let edgeuses2 = loopuse::loop_edgeuses(b, loop2)?;

    // UVTrimCurves are measured in UVSpace, so the 3d ZoneTol has to be mapped there
    let zone_tol_3d = crate::surface::surface::p1::zone_tol_3d(k, surface)?;
    let mid = surface.natural_uv_domain_with_nurb(k)?.mid();
    let uv_tol = crate::surface::surface::p1::map_to_2d(k, zone_tol_3d, mid, surface)?;

    // for every Loop1 UVTrimCurve
    for &eu1_id in &edgeuses1 {
        let Some(uv_curve1) = uv_curve(k, b, eu1_id)? else {
            continue; // inconclusive - nothing to intersect
        };
        let interval1 = uv_curve1.natural_interval();

        // for every Loop2 UVTrimCurve
        for &eu2_id in &edgeuses2 {
            let Some(uv_curve2) = uv_curve(k, b, eu2_id)? else {
                continue; // inconclusive - nothing to intersect
            };
            let interval2 = uv_curve2.natural_interval();
            let solutions = match uv_curve1.global_curve_intersect::<CurveHandle>(
                k,
                &interval1,
                &uv_curve2,
                &interval2,
                uv_tol,
                &crate::curve::curve::p8::CurveIntersectOptions::default(),
            ) {
                Ok(s) => s,
                Err(e) if e.is_unported() => return Err(e),
                Err(_) => continue, // inconclusive - the intersector gave up on this pair
            };

            // any solution at all places the two Loops on one another
            if !solutions.is_empty() {
                return Ok(true);
            }
        }
    }

    // arrive here when no intersection was found
    Ok(false)
}

/// `sm_AssertTestFace38`: `true` when no two loops of the face meet in its UV space
/// (`sm_DoLoopsIntersectInUV` for every loop pair, `GetLoops` order). A face with
/// fewer than two loops passes.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a face without surface (upstream dereferences
/// it); those of the loop, trim curve, tolerance and intersection computations
/// ([`SolidError::Unported`] included).
pub fn assert_test_face38(k: &Kernel, b: &mut Brep, f: FaceId) -> Result<bool> {
    let loops = crate::topology::face::p2::loops(b, f, false, None)?;
    let surface = crate::topology::face::p2::surface(b, f)?
        .ok_or_else(|| null("sm_AssertTestFace38: face without surface"))?
        .clone();

    // no work - a Face with fewer than 2 Loops has no Loop pair to intersect
    if loops.len() < 2 {
        return Ok(true);
    }

    // for every pair of Face Loops
    for (ii, &l1) in loops.iter().enumerate() {
        for &l2 in loops.iter().skip(ii + 1) {
            if do_loops_intersect_in_uv(k, b, l1, l2, &surface)? {
                return Ok(false);
            }
        }
    }

    // arrive here when no two Face Loops intersect
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boolean::api_brep::boolean_difference;
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::math::{Point3, Vec3};
    use crate::primitive::api_primitives::{create_box, create_cylinder};
    use crate::topology::face::{p2, p4};

    fn all_pass(k: &Kernel, b: &mut Brep) {
        let faces = crate::topology::brep::p1::faces(b, None).unwrap();
        for f in faces {
            assert!(assert_test_face32(b, f).unwrap(), "{f:?} test 32");
            assert!(assert_test_face33(k, b, f).unwrap(), "{f:?} test 33");
            assert!(assert_test_face38(k, b, f).unwrap(), "{f:?} test 38");
        }
    }

    fn edge_between(b: &Brep, f: FaceId, v1: VertexId, v2: VertexId) -> EdgeId {
        p4::edges(b, f, None, false, None)
            .unwrap()
            .into_iter()
            .find(|&e| {
                let s = edge::start_vertex(b, e).unwrap();
                let t = edge::end_vertex(b, e).unwrap();
                (s, t) == (Some(v1), Some(v2)) || (s, t) == (Some(v2), Some(v1))
            })
            .unwrap()
    }

    /// A box passes tests 32, 33 and 38 on every face.
    #[test]
    fn box_faces_pass() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 2.0, 3.0, 4.0).unwrap();
        all_pass(&k, &mut b);
    }

    /// A box face whose third corner is moved onto its first and whose second edge is
    /// given the first edge's curve has one coincident vertex pair (test 32) and one
    /// coincident edge pair (test 33).
    #[test]
    fn coincident_vertices_and_edges_fail() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 2.0, 3.0, 4.0).unwrap();
        let f = b.faces().ids().next().unwrap();
        let vs = p4::vertices(&b, f, None, false, None).unwrap();
        let (v0, v1, v2) = (vs[0], vs[1], vs[2]);
        let e01 = edge_between(&b, f, v0, v1);
        let e12 = edge_between(&b, f, v1, v2);

        // test 32 only: the corner moved, the curves untouched
        let p0 = b.vertex(v0).unwrap().point;
        b.vertex_mut(v2).unwrap().point = p0;
        assert!(!assert_test_face32(&b, f).unwrap());
        let pairs = has_coincident_vertices(&b, f, false).unwrap();
        assert_eq!(
            pairs,
            CoincidentVertices {
                found: true,
                to_survive: vec![v0],
                to_delete: vec![v2],
            }
        );
        assert!(assert_test_face33(&k, &b, f).unwrap());

        // test 33: the edge from v1 to v2 (now at v0) on the curve from v0 to v1
        let (curve, interval) = {
            let e = b.edge(e01).unwrap();
            (e.curve.clone(), e.interval)
        };
        let e = b.edge_mut(e12).unwrap();
        e.curve = curve;
        e.interval = interval;
        assert!(!assert_test_face33(&k, &b, f).unwrap());
        let pairs = has_coincident_edges(&k, &b, f, false).unwrap();
        assert!(pairs.found);
        assert_eq!(pairs.to_survive.len(), 1);
        let pair = [pairs.to_survive[0], pairs.to_delete[0]];
        assert!(pair == [e01, e12] || pair == [e12, e01], "{pair:?}");
    }

    /// The top face of a box with a through hole has two disjoint loops (test 38
    /// passes); moving the hole across the face boundary makes them cross (fails).
    #[test]
    fn loops_crossing_in_uv_fail() {
        let mut k = Kernel::default();
        let a = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
        let c = create_cylinder(&mut k, Point3::new(5.0, 5.0, -1.0), 2.0, 12.0).unwrap();
        let mut b = boolean_difference(&mut k, a, c).unwrap();
        all_pass(&k, &mut b);

        let faces = crate::topology::brep::p1::faces(&b, None).unwrap();
        let mut two_loop_faces = 0;
        for f in faces {
            let loops = p2::loops(&b, f, false, None).unwrap();
            if loops.len() != 2 {
                continue;
            }
            two_loop_faces += 1;
            let mut b = b.clone();
            // move the inner loop's edge (and its vertex) 4 along x, dropping its UV
            // trims so they are rebuilt from the moved curve
            let inner = loopuse::loop_edges(&b, loops[1]).unwrap();
            let shift = Axis2Placement::new(
                Point3::new(4.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
                Vec3::new(1.0, 0.0, 0.0),
            );
            for e in inner.into_iter().flatten() {
                let mut curve = b.edge(e).unwrap().curve.clone().unwrap();
                curve.transform(&k, &shift, None).unwrap();
                b.edge_mut(e).unwrap().curve = Some(curve);
                for v in b.edge(e).unwrap().vertices {
                    let p = b.vertex(v).unwrap().point;
                    b.vertex_mut(v).unwrap().point = Point3::new(p.x + 4.0, p.y, p.z);
                }
                for eu in b.edge(e).unwrap().edgeuses.clone() {
                    b.edgeuse_mut(eu).unwrap().uv_trim_curve = None;
                }
            }
            assert!(!assert_test_face38(&k, &mut b, f).unwrap(), "{f:?}");
        }
        assert_eq!(two_loop_faces, 2);
    }
}
