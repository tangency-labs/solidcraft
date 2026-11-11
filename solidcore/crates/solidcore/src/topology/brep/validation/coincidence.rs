//! Ordered B-rep coincidence checks with conservative spatial pruning.
//!
//! Upstream: `source/SMLib/src/SmBrep.cpp`, `source/SMLib/inc/SmBrep.h`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`contains_coincident_topology`] | `SmBrep::ContainsCoincidentTopology` |
//! | Face candidate search (inline) | `sm_FindCoinFace` |
//! | [`CoincidenceResult`] | Boolean and paired topology out-parameters |
//!
//! Deviations: local spatial indices and endpoint/outer-loop membership indices
//! prune impossible candidates; accepted pairs keep native order. No recursion.
//! The native exhaustive-search boolean is always true (`bCheck` starts false);
//! it is preserved, independently of whether any pairs were found.
//! The opt-in profiler counts supplied candidates and query invocations; its
//! counters are observational and compile out without the `profile` feature.
use super::spatial::SpatialIndex;
use super::{invalid, required, Reports};
use crate::error::{EntityRef, Result};
use crate::geom::assert_array::AssertArray;
use crate::geom::extent3d::Extent3d;
use crate::geom::global_solver::SolverOperation;
use crate::geom::solution_array::SolutionType;
use crate::geom::tools::{find_coincident_edge, CoincidentEdgeQuery};
use crate::intersect::point_class::{PointClassType, PointClassification};
use crate::kernel::Kernel;
use crate::math::UNDEF_DOUBLE;
use crate::nurbs::b_spline_curve::BoundingBoxRequest;
use crate::profile::{profile_count, profile_scope};
use crate::query::brep::{BrepFaceServices, FaceCaches};
use crate::query::face::{p1 as query_face, p2 as classify_face};
use crate::surface::surface::p5::DropPointOptions;
use crate::topology::brep::p1;
use crate::topology::brep_cache::CacheObject;
use crate::topology::face::{p1 as face1, p2 as face2, p4 as face4};
use crate::topology::{edge, loopuse, Brep, EdgeId, FaceId};
use std::collections::{BTreeMap, BTreeSet};

/// Coincidence flag and native-order topology pairs.
#[derive(Debug, Clone)]
pub struct CoincidenceResult {
    /// Native boolean (also true after an exhaustive search with no pairs).
    pub found: bool,
    /// Pairs in vertex/vertex, edge/edge, face/face, vertex/edge,
    /// vertex/face and edge/face order.
    pub pairs: Vec<(EntityRef, EntityRef)>,
}

fn ignored<T>(value: Result<T>) -> Result<Option<T>> {
    match value {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.is_unported_or_cancelled() => Err(error),
        Err(_) => Ok(None),
    }
}

/// Checks coincident topology, appending native-order pairs and failed reports.
/// Negative or zero tolerance selects the B-rep tolerance. `find_all=false`
/// stops at the first pair; otherwise every pair is returned. `level` selects the
/// native report level (0 through 4), without suppressing the geometric search.
/// The native all-pairs boolean quirk is preserved: inspect `pairs` for emptiness.
pub fn contains_coincident_topology(
    k: &Kernel,
    b: &mut Brep,
    tolerance: f64,
    find_all: bool,
    reports: Option<&mut AssertArray>,
    level: u8,
    keys: &dyn Fn(EntityRef) -> crate::geom::object::ObjectRef,
) -> Result<CoincidenceResult> {
    // (not upstream) cooperative cancellation once per vertex, edge and face of each search
    // (solidcore#3247)
    const OP: &str = "topology::brep::validation::coincidence::contains_coincident_topology";
    profile_scope!(k, "validation::coincidence");
    if level > 4 {
        return Err(invalid("invalid assert test level"));
    }
    let tol = if tolerance <= 0.0 {
        b.tolerance()
    } else {
        tolerance
    };
    let mut r = Reports {
        array: reports,
        level,
    };
    let mut result = CoincidenceResult {
        found: true,
        pairs: Vec::new(),
    };
    let vertices = p1::vertices(b, None)?;
    let points = vertices
        .iter()
        .map(|&v| Ok(b.vertex(v)?.point))
        .collect::<Result<Vec<_>>>()?;
    let point_index = SpatialIndex::new(points.iter().copied().map(Extent3d::from_point).map(Some));
    {
        profile_scope!(k, "validation::coincidence::vertex_vertex");
        for (i, &v) in vertices.iter().enumerate() {
            k.check_cancelled(OP)?;
            let point = b.vertex(v)?.point;
            let mut previous = i;
            for j in point_index
                .near_point(point, tol)
                .into_iter()
                .filter(|&j| j > i)
            {
                profile_count!(k, CoincidenceCandidates, 1);
                if j > previous + 1 {
                    if let Some(a) = r.array.as_deref_mut() {
                        a.set_last_add(false);
                    }
                }
                let &other = vertices.get(j).ok_or_else(|| invalid("vertex candidate"))?;
                let p = b.vertex(other)?.point;
                profile_count!(k, CoincidenceQueries, 1);
                let coincident = point.closer_than(tol, p);
                r.pair(
                    b,
                    0,
                    !coincident,
                    EntityRef::Vertex(v),
                    EntityRef::Vertex(other),
                    tol,
                    point.distance(p),
                )?;
                if coincident {
                    result
                        .pairs
                        .push((EntityRef::Vertex(v), EntityRef::Vertex(other)));
                    if !find_all {
                        return Ok(result);
                    }
                }
                previous = j;
            }
            if previous + 1 < vertices.len() {
                if let Some(a) = r.array.as_deref_mut() {
                    a.set_last_add(false);
                }
            }
        }
    }
    let edges = p1::edges(b, None)?;
    let owners: BTreeMap<_, _> = edges
        .iter()
        .map(|&e| (keys(EntityRef::Edge(e)), e))
        .collect();
    if owners.len() != edges.len() {
        return Err(invalid("topology identities alias distinct edges"));
    }
    let mut endpoint_groups: BTreeMap<(_, _), Vec<(usize, EdgeId)>> = BTreeMap::new();
    for (i, &e) in edges.iter().enumerate() {
        let v1 = required(edge::vertex(b, e)?)?;
        let v2 = required(edge::other_vertex(b, e, Some(v1))?)?;
        endpoint_groups
            .entry((v1.min(v2), v1.max(v2)))
            .or_default()
            .push((i, e));
    }
    {
        profile_scope!(k, "validation::coincidence::edge_edge");
        for (i, &e) in edges.iter().enumerate() {
            k.check_cancelled(OP)?;
            let v1 = required(edge::vertex(b, e)?)?;
            let v2 = required(edge::other_vertex(b, e, Some(v1))?)?;
            let candidates: Vec<_> = endpoint_groups
                .get(&(v1.min(v2), v1.max(v2)))
                .into_iter()
                .flatten()
                .filter(|(j, _)| *j > i)
                .map(|(_, e)| *e)
                .collect();
            let curve = required(b.edge(e)?.curve.as_ref())?;
            let query = CoincidentEdgeQuery {
                start_vertex: v1,
                end_vertex: v2,
                curve,
                curve_owner: curve.owner().and_then(|o| owners.get(&o).copied()),
                interval: b.edge(e)?.interval,
                tolerance: tol,
            };
            profile_count!(k, CoincidenceCandidates, candidates.len() as u64);
            profile_count!(k, CoincidenceQueries, 1);
            let found = ignored(find_coincident_edge(k, b, &query, &candidates, 0))?.flatten();
            if let Some(found) = found.filter(|f| f.max_deviation < tol) {
                let other = *candidates
                    .get(found.index)
                    .ok_or_else(|| invalid("edge candidate"))?;
                r.pair(
                    b,
                    1,
                    false,
                    EntityRef::Edge(e),
                    EntityRef::Edge(other),
                    tol,
                    found.max_deviation,
                )?;
                result
                    .pairs
                    .push((EntityRef::Edge(e), EntityRef::Edge(other)));
                if !find_all {
                    return Ok(result);
                }
            } else {
                r.pair(
                    b,
                    1,
                    true,
                    EntityRef::Edge(e),
                    EntityRef::Edge(e),
                    tol,
                    UNDEF_DOUBLE,
                )?;
            }
        }
    }
    let faces = p1::faces(b, None)?;
    let mut caches = FaceCaches::new();
    let mut face_boxes = Vec::new();
    for &f in &faces {
        k.check_cancelled(OP)?;
        face_boxes.push(ignored(query_face::calculate_bounding_box(
            k, &mut *b, f, false,
        ))?);
    }
    let mut outer_groups: BTreeMap<Vec<Option<EdgeId>>, Vec<usize>> = BTreeMap::new();
    let mut outer_keys = Vec::new();
    for (i, &f) in faces.iter().enumerate() {
        let mut key = match face2::loops(b, f, false, None)?.first().copied() {
            Some(l) => loopuse::loop_edges(b, l)?,
            None => Vec::new(),
        };
        key.sort_unstable();
        outer_groups.entry(key.clone()).or_default().push(i);
        outer_keys.push(key);
    }
    {
        profile_scope!(k, "validation::coincidence::face_face");
        for (i, &f) in faces.iter().enumerate() {
            k.check_cancelled(OP)?;
            let key = outer_keys.get(i).ok_or_else(|| invalid("face candidate"))?;
            let mut found = None;
            for &j in outer_groups
                .get(key)
                .into_iter()
                .flatten()
                .filter(|&&j| j > i)
            {
                profile_count!(k, CoincidenceCandidates, 1);
                let other = *faces.get(j).ok_or_else(|| invalid("face candidate"))?;
                let coin_tol = tol + b.face(other)?.tolerance;
                if let (Some(a), Some(c)) = (
                    face_boxes.get(i).copied().flatten(),
                    face_boxes.get(j).copied().flatten(),
                ) {
                    if a.are_disjoint(&c, coin_tol) {
                        continue;
                    }
                }
                // A face without outer loop makes FindCoinFace return SM_ERR.
                if face2::loops(b, f, false, None)?.is_empty() {
                    break;
                }
                if face2::loops(b, other, false, None)?.is_empty() {
                    continue;
                }
                let domain = trim_domain(k, b, other)?;
                let uv = domain.evaluate(0.5, 0.5);
                let point = required(b.face(other)?.surface.as_ref())?.evaluate_point(k, uv)?;
                let domain = trim_domain(k, b, f)?;
                let opts = DropPointOptions {
                    guess_uv: Some(domain.evaluate(0.5, 0.5)),
                    ..DropPointOptions::default()
                };
                profile_count!(k, CoincidenceQueries, 1);
                let drop = ignored(drop_face(k, b, f, point, &domain, &opts, &mut caches))?;
                if let Some(drop) = drop {
                    if drop.success
                        && drop.gap < coin_tol
                        && domain.contains_point(drop.result_uv, 0.0)
                    {
                        found = Some((other, drop.gap));
                        break;
                    }
                }
            }
            if let Some((other, gap)) = found.filter(|(_, gap)| *gap < tol) {
                r.pair(
                    b,
                    2,
                    false,
                    EntityRef::Face(f),
                    EntityRef::Face(other),
                    tol,
                    gap,
                )?;
                result
                    .pairs
                    .push((EntityRef::Face(f), EntityRef::Face(other)));
                if !find_all {
                    return Ok(result);
                }
            } else {
                r.pair(
                    b,
                    2,
                    true,
                    EntityRef::Face(f),
                    EntityRef::Face(f),
                    tol,
                    UNDEF_DOUBLE,
                )?;
            }
        }
    }
    let mut edge_boxes = Vec::new();
    let mut previous_box = Extent3d::new();
    {
        profile_scope!(k, "validation::coincidence::vertex_edge");
        for &e in &edges {
            k.check_cancelled(OP)?;
            let bbox = ignored(crate::query::edge::calculate_bounding_box(
                k,
                b,
                e,
                BoundingBoxRequest {
                    normal: true,
                    pseudo: false,
                    polar: false,
                },
                false,
            ))?;
            if let Some(bbox) = bbox.and_then(|b| b.normal) {
                previous_box = bbox;
            }
            edge_boxes.push(previous_box);
            let data = b.edge(e)?;
            let Some(curve) = data.curve.as_ref() else {
                continue;
            };
            let v1 = edge::vertex(b, e)?;
            let v2 = match v1 {
                Some(v) => edge::other_vertex(b, e, Some(v))?,
                None => None,
            };
            for j in point_index.points_in_box(&previous_box, tol) {
                profile_count!(k, CoincidenceCandidates, 1);
                let v = *vertices.get(j).ok_or_else(|| invalid("vertex candidate"))?;
                if Some(v) == v1 || Some(v) == v2 {
                    continue;
                }
                let point = b.vertex(v)?.point;
                if !previous_box.contains_point(point, tol) {
                    continue;
                }
                profile_count!(k, CoincidenceQueries, 1);
                let drop = ignored(curve.drop_point(
                    k,
                    &data.interval,
                    point,
                    None,
                    tol,
                    None,
                    SolverOperation::Intersect,
                ))?;
                if let Some(drop) = drop {
                    if !drop.success || drop.distance > tol {
                        continue;
                    }
                    r.pair(
                        b,
                        3,
                        false,
                        EntityRef::Vertex(v),
                        EntityRef::Edge(e),
                        tol,
                        drop.distance,
                    )?;
                    result
                        .pairs
                        .push((EntityRef::Vertex(v), EntityRef::Edge(e)));
                    if !find_all {
                        return Ok(result);
                    }
                }
            }
        }
    }
    let edge_index = SpatialIndex::new(edge_boxes.iter().copied().map(Some));
    let mut previous_face_box = Extent3d::new();
    for &f in &faces {
        let face_tol = b.face(f)?.tolerance;
        let mut pc = PointClassification::<EntityRef>::new(face_tol);
        if let Some(bbox) = ignored(query_face::calculate_bounding_box(k, &mut *b, f, false))? {
            previous_face_box = bbox;
        }
        let domain = b.face(f)?.uv_domain;
        let face_vertices: BTreeSet<_> = face4::vertices(b, f, None, false, None)?
            .into_iter()
            .collect();
        let face_edges: BTreeSet<_> = face4::edges(b, f, None, false, None)?.into_iter().collect();
        {
            profile_scope!(k, "validation::coincidence::vertex_face");
            for j in point_index.points_in_box(&previous_face_box, tol) {
                profile_count!(k, CoincidenceCandidates, 1);
                let v = *vertices.get(j).ok_or_else(|| invalid("vertex candidate"))?;
                if face_vertices.contains(&v) {
                    continue;
                }
                let point = b.vertex(v)?.point;
                if !previous_face_box.contains_point(point, tol) {
                    continue;
                }
                profile_count!(k, CoincidenceQueries, 1);
                let drop = ignored(drop_face(
                    k,
                    b,
                    f,
                    point,
                    &domain,
                    &DropPointOptions::default(),
                    &mut caches,
                ))?;
                let Some(drop) = drop else {
                    continue;
                };
                if !drop.success || drop.gap > tol {
                    continue;
                }
                let opts = classify_face::PointClassifyOptions {
                    src_zone_tol_3d: b.vertex(v)?.tolerance,
                    use_uv_classification: true,
                    ..classify_face::PointClassifyOptions::default()
                };
                profile_count!(k, CoincidenceClassifications, 1);
                if ignored(classify_face::point_classify(
                    k,
                    b,
                    f,
                    drop.result_uv,
                    &mut pc,
                    opts,
                ))?
                .is_none()
                {
                    continue;
                }
                if !matches!(
                    pc.point_class(),
                    PointClassType::Face | PointClassType::Edge | PointClassType::Vertex
                ) {
                    continue;
                }
                r.pair(
                    b,
                    4,
                    false,
                    EntityRef::Vertex(v),
                    EntityRef::Face(f),
                    tol,
                    drop.gap,
                )?;
                result
                    .pairs
                    .push((EntityRef::Vertex(v), EntityRef::Face(f)));
                if !find_all {
                    return Ok(result);
                }
            }
        }
        {
            profile_scope!(k, "validation::coincidence::edge_face");
            for j in edge_index.boxes_in_box(&previous_face_box, tol) {
                profile_count!(k, CoincidenceCandidates, 1);
                let e = *edges.get(j).ok_or_else(|| invalid("edge candidate"))?;
                if face_edges.contains(&e) {
                    continue;
                }
                let bbox = edge_boxes.get(j).ok_or_else(|| invalid("edge box"))?;
                if previous_face_box.are_disjoint(bbox, tol) {
                    continue;
                }
                let interval = b.edge(e)?.interval;
                let curve = required(b.edge(e)?.curve.as_ref())?;
                let surface = required(b.face(f)?.surface.as_ref())?.clone_for_evaluation()?;
                let curve = curve.clone_for_evaluation()?;
                let mut services = BrepFaceServices::with_caches(b, std::mem::take(&mut caches));
                profile_count!(k, CoincidenceQueries, 1);
                let solution = surface.simple_coincidence_checker_with_context(
                    k,
                    &domain,
                    &curve,
                    &interval,
                    tol,
                    true,
                    [Some(CacheObject::Curve(e)), Some(CacheObject::Surface(f))],
                    None,
                    &mut services,
                );
                caches = services.into_caches();
                let solution = ignored(solution)?.flatten();
                let Some(solution) = solution else {
                    continue;
                };
                if solution.solution_type != SolutionType::RangeOfValues
                    || solution.start.solution_value > tol
                    || solution.end.solution_value > tol
                {
                    continue;
                }
                profile_count!(k, CoincidenceClassifications, 1);
                for class in face2::classify_interval_on_face(k, b, f, &curve, interval, tol)? {
                    if !matches!(class, PointClassType::Face | PointClassType::Edge) {
                        continue;
                    }
                    r.pair(
                        b,
                        5,
                        false,
                        EntityRef::Edge(e),
                        EntityRef::Face(f),
                        tol,
                        UNDEF_DOUBLE,
                    )?;
                    result.pairs.push((EntityRef::Edge(e), EntityRef::Face(f)));
                    if !find_all {
                        return Ok(result);
                    }
                }
            }
        }
    }
    Ok(result)
}

fn trim_domain(k: &Kernel, b: &mut Brep, f: FaceId) -> Result<crate::geom::extent2d::Extent2d> {
    Ok(ignored(face1::calculate_uv_domain_from_uv_trim_curves(
        k, b, f, true, false, false,
    ))?
    .map_or(b.face(f)?.uv_domain, |d| d.domain))
}

fn drop_face(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    point: crate::math::Point3,
    domain: &crate::geom::extent2d::Extent2d,
    opts: &DropPointOptions,
    caches: &mut FaceCaches,
) -> Result<crate::surface::surface::p5::DropPoint<CacheObject>> {
    let surface = required(b.face(f)?.surface.as_ref())?.clone_for_evaluation()?;
    let mut services = BrepFaceServices::with_caches(b, std::mem::take(caches));
    let result = surface.drop_point_with_context(
        k,
        point,
        domain,
        opts,
        Some(CacheObject::Surface(f)),
        &mut services,
    );
    *caches = services.into_caches();
    result
}

#[cfg(all(test, feature = "profile"))]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod profile_tests {
    use super::*;
    use crate::primitive::api_primitives::create_box;
    use crate::Point3;

    /// Read a named counter from the single phase path expected by these fixtures.
    fn count(profile: &crate::profile::Profile, phase: &str, counter: &str) -> u64 {
        let scope = profile.scopes.iter().find(|s| s.name == phase).unwrap();
        crate::profile::Counter::ALL
            .iter()
            .zip(scope.counters)
            .find(|(c, _)| c.name() == counter)
            .unwrap()
            .1
    }

    /// Incident pairs contribute candidates but do not invoke geometric queries.
    #[test]
    fn cube_counts_distinguish_incidence_candidates_from_geometric_queries() {
        let mut k = Kernel::default();
        let mut body = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
        let identities = body.clone_standing_for_self().unwrap();
        let keys = |e| identities.owner_identity(e);
        let _ = k.take_profile();
        let result =
            contains_coincident_topology(&k, &mut body, -1.0, false, None, 0, &keys).unwrap();
        assert!(result.pairs.is_empty());
        let profile = k.take_profile();
        // Twelve edges have two endpoint vertices each; six faces have four
        // incident vertices each. Those are candidates, but need no projection.
        for phase in ["vertex_edge", "vertex_face"] {
            let name = format!("validation::coincidence::{phase}");
            assert_eq!(count(&profile, &name, "coincidence_candidates"), 24);
            assert_eq!(count(&profile, &name, "coincidence_queries"), 0);
        }
        // Each face meets four boundary edges and four transverse edges at an
        // endpoint. Only the latter four reach the geometric coincidence query.
        let phase = "validation::coincidence::edge_face";
        assert_eq!(count(&profile, phase, "coincidence_candidates"), 48);
        assert_eq!(count(&profile, phase, "coincidence_queries"), 24);
        assert_eq!(count(&profile, phase, "coincidence_classifications"), 0);
    }

    /// A first-match return closes both scopes and leaves later phases unvisited.
    #[test]
    fn first_vertex_pair_return_closes_the_profile_and_skips_later_phases() {
        let mut k = Kernel::default();
        let mut body = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
        let vertices = p1::vertices(&body, None).unwrap();
        body.vertex_mut(vertices[1]).unwrap().point = body.vertex(vertices[0]).unwrap().point;
        let identities = body.clone_standing_for_self().unwrap();
        let keys = |e| identities.owner_identity(e);
        let _ = k.take_profile();
        let result =
            contains_coincident_topology(&k, &mut body, -1.0, false, None, 0, &keys).unwrap();
        assert_eq!(
            result.pairs,
            vec![(
                EntityRef::Vertex(vertices[0]),
                EntityRef::Vertex(vertices[1])
            )]
        );
        let profile = k.take_profile();
        assert_eq!(profile.scopes.len(), 2);
        assert_eq!(
            count(
                &profile,
                "validation::coincidence",
                "coincidence_candidates"
            ),
            1
        );
        assert_eq!(
            count(&profile, "validation::coincidence", "coincidence_queries"),
            1
        );
        assert!(k.take_profile().scopes.is_empty());
    }
}
