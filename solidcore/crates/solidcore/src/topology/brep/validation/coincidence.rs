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
//! A vertex/face drop or an edge/face coincidence solve is skipped when the certified
//! boxes of a stored B-spline face (the control-point boxes of its knot spans, each
//! refined by splitting its Bezier patch `COVER_DEPTH` times) and of the edge's curve
//! (the same for a stored B-spline curve, else the curve's own box) are all farther
//! than `2 * tol` plus their rounding allowance apart (see [`cover_margin`]):
//! no pair can result, so the pairs, reports and their order are unchanged. Two side
//! effects of skipping: a query that would have returned an Unported or Cancelled
//! error (`ignored` propagates those) is no longer run, so such an `Err` can become
//! `Ok`; and a skipped query no longer fills the per-face caches (`FaceCaches`),
//! which only memoise results of later queries.
use super::spatial::SpatialIndex;
use super::{invalid, required, Reports};
use crate::curve::curve::{Curve, CurveKind};
use crate::error::{EntityRef, Result};
use crate::geom::assert_array::AssertArray;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent3d::Extent3d;
use crate::geom::global_solver::SolverOperation;
use crate::geom::solution_array::SolutionType;
use crate::geom::tools::{find_coincident_edge, CoincidentEdgeQuery};
use crate::intersect::point_class::{PointClassType, PointClassification};
use crate::kernel::Kernel;
use crate::math::{Point3, UNDEF_DOUBLE};
use crate::nurbs::b_spline_curve::BoundingBoxRequest;
use crate::nurbs::nurbs_crv::CurveCover;
use crate::nurbs::nurbs_srf::SurfaceCover;
use crate::profile::{profile_count, profile_scope};
use crate::query::brep::{BrepFaceServices, FaceCaches};
use crate::query::face::{p1 as query_face, p2 as classify_face};
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::SurfaceKind;
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

/// How many times a surviving span of a face or edge is split before its boxes are
/// tested (4 pieces per split of a surface patch, 2 of a curve segment).
const COVER_DEPTH: usize = 3;

/// How far a certified box may be from a point or another box and still hold a pair.
///
/// A pair needs a gap `<= tol` between exact geometry. The boxes bound the exact
/// geometry (convex hull of each span's, or piece's, control points), but the solvers
/// measure that gap in floating point, whose error is the rounding of the evaluated
/// coordinates: about `n * eps * |coordinate|` for a degree-`n` evaluation. That is
/// below `tol` for coordinates up to `tol / (n * eps)` (about 3e10 for the default 1e-5
/// and a cubic), so the allowance for it is taken as one more `tol`. The rounding of
/// splitting spans into pieces is a separate allowance, `slack` of the covers.
fn cover_margin(tol: f64) -> f64 {
    2.0 * tol
}

/// A face's certified cover: its surface's span boxes, refinable, and a lookup index of
/// them.
struct FaceCover {
    surface: SurfaceCover,
    index: SpatialIndex,
}

impl FaceCover {
    /// `None` means no certified cover (no surface, another kind, an unreadable net):
    /// nothing is pruned then, as before. A missing surface is reported by the query that
    /// needs it, not here.
    fn new(b: &Brep, f: FaceId) -> Result<Option<Self>> {
        let Some(surface) = b.face(f)?.surface.as_ref() else {
            return Ok(None);
        };
        let SurfaceKind::BSpline(spline) = surface.kind() else {
            return Ok(None);
        };
        Ok(spline
            .cover(COVER_DEPTH)
            .filter(|cover| !cover.coarse().is_empty())
            .map(|surface| Self {
                index: SpatialIndex::new(surface.coarse().iter().copied().map(Some)),
                surface,
            }))
    }

    /// Whether a surface point can be within `margin` of `point`: some span near it has
    /// a piece box near it.
    fn may_reach_point(&mut self, point: Point3, margin: f64) -> bool {
        let reach = margin + self.surface.slack();
        for i in self.index.near_point(point, margin) {
            if self
                .surface
                .pieces(i)
                .iter()
                .any(|piece| piece.contains_point(point, reach))
            {
                return true;
            }
        }
        false
    }

    /// Whether a point of `curve` can be within `margin` of a surface point: some curve
    /// span and some surface span within `margin` have pieces within `margin` plus both
    /// allowances.
    fn may_reach_curve(&mut self, curve: &mut EdgeCover, margin: f64) -> bool {
        let reach = margin + self.surface.slack() + curve.slack();
        for ci in 0..curve.coarse().len() {
            let Some(span) = curve.coarse().get(ci).copied() else {
                continue;
            };
            for si in self.index.boxes_in_box(&span, margin) {
                let curve_pieces = curve.pieces(ci);
                let surface_pieces = self.surface.pieces(si);
                if curve_pieces
                    .iter()
                    .any(|c| surface_pieces.iter().any(|s| !c.are_disjoint(s, reach)))
                {
                    return true;
                }
            }
        }
        false
    }
}
/// A cover made on first use: not yet made, made and not certified (nothing to prune
/// with), or made.
enum Lazy<T> {
    Unmade,
    Absent,
    Made(T),
}

impl<T> Lazy<T> {
    /// The cover, made by `make` on the first call; `None` when not certified.
    fn get_or_make(&mut self, make: impl FnOnce() -> Result<Option<T>>) -> Result<Option<&mut T>> {
        if matches!(self, Self::Unmade) {
            *self = make()?.map_or(Self::Absent, Self::Made);
        }
        Ok(match self {
            Self::Made(cover) => Some(cover),
            Self::Unmade | Self::Absent => None,
        })
    }
}

/// An edge's certified cover: the refinable span boxes of a stored B-spline curve, or
/// the curve's own box (the one the solver itself rejects with) for any other curve.
enum EdgeCover {
    Spline(CurveCover),
    Whole([Extent3d; 1]),
}

impl EdgeCover {
    /// `own_box` is the box computed for this edge itself (`None` where that failed).
    /// `None` means no certified cover.
    fn new(curve: &Curve, interval: &Extent1d, own_box: Option<Extent3d>) -> Option<Self> {
        match curve.kind() {
            CurveKind::BSpline(spline) => spline
                .cover(interval, COVER_DEPTH)
                .filter(|cover| !cover.coarse().is_empty())
                .map(Self::Spline),
            _ => own_box.map(|bx| Self::Whole([bx])),
        }
    }

    fn coarse(&self) -> &[Extent3d] {
        match self {
            Self::Spline(cover) => cover.coarse(),
            Self::Whole(bx) => bx,
        }
    }

    fn slack(&self) -> f64 {
        match self {
            Self::Spline(cover) => cover.slack(),
            Self::Whole(_) => 0.0,
        }
    }

    fn pieces(&mut self, i: usize) -> &[Extent3d] {
        match self {
            Self::Spline(cover) => cover.pieces(i),
            Self::Whole(bx) => bx,
        }
    }
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
    search(k, b, tolerance, find_all, reports, level, keys, true)
}

/// [`contains_coincident_topology`]; `prune = false` (only the tests pass it) runs every
/// query the boxes of the faces and edges leave, as the reference the certified span-box
/// prune is compared with.
fn search(
    k: &Kernel,
    b: &mut Brep,
    tolerance: f64,
    find_all: bool,
    reports: Option<&mut AssertArray>,
    level: u8,
    keys: &dyn Fn(EntityRef) -> crate::geom::object::ObjectRef,
    prune: bool,
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
    // The box each edge's own computation returned (`None` where it failed and
    // `edge_boxes` carries the previous edge's box instead).
    let mut own_edge_boxes = Vec::new();
    let margin = cover_margin(tol);
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
            let own_box = bbox.and_then(|b| b.normal);
            if let Some(bbox) = own_box {
                previous_box = bbox;
            }
            edge_boxes.push(previous_box);
            own_edge_boxes.push(own_box);
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
    // Certified curve covers, made at an edge's first use (`None` inside: not certified).
    let mut edge_covers: Vec<Lazy<EdgeCover>> = edges.iter().map(|_| Lazy::Unmade).collect();
    let mut previous_face_box = Extent3d::new();
    for &f in &faces {
        let face_tol = b.face(f)?.tolerance;
        let mut pc = PointClassification::<EntityRef>::new(face_tol);
        if let Some(bbox) = ignored(query_face::calculate_bounding_box(k, &mut *b, f, false))? {
            previous_face_box = bbox;
        }
        let domain = b.face(f)?.uv_domain;
        // The face's cover, made when a candidate first needs it.
        let mut cover: Lazy<FaceCover> = Lazy::Unmade;
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
                if prune {
                    if let Some(cover) = cover.get_or_make(|| FaceCover::new(b, f))? {
                        if !cover.may_reach_point(point, margin) {
                            continue;
                        }
                    }
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
                if prune {
                    if let Some(cover) = cover.get_or_make(|| FaceCover::new(b, f))? {
                        let slot = edge_covers
                            .get_mut(j)
                            .ok_or_else(|| invalid("edge cover"))?;
                        let made = slot.get_or_make(|| {
                            let data = b.edge(e)?;
                            let own_box = own_edge_boxes.get(j).copied().flatten();
                            Ok(data
                                .curve
                                .as_ref()
                                .and_then(|c| EdgeCover::new(c, &data.interval, own_box)))
                        })?;
                        if let Some(curve_cover) = made {
                            if !cover.may_reach_curve(curve_cover, margin) {
                                continue;
                            }
                        }
                    }
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
    point: Point3,
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
pub(super) mod tests {
    use super::*;
    use crate::geom::extent2d::Extent2d;
    use crate::math::Vec3;
    use crate::nurbs::b_spline_surface::BSplineSurface;
    use crate::nurbs::math::{CPoint, NOW};
    use crate::nurbs::srf_basic::Surface as Net;
    use crate::primitive::api_primitives::{create_box, create_cylinder};
    use crate::surface::surface::Surface;
    use crate::topology::brep::p1 as brep1;
    use crate::topology::brep::p2::ArenaNotifyState;
    use crate::topology::brep::p8::{create_face_from_surface, FaceBuildEnv};
    use crate::topology::topology_traverser::Marks;

    /// A sheet body of `n` parallel vertical B-spline panels (`length` long along the
    /// diagonal `(1, 1, 0)`, `height` tall, bilinear: one span each), `spacing` apart
    /// across it. Each panel is its own face with four B-spline edges; every panel's box
    /// overlaps every other's while their pieces do not.
    pub(in super::super) fn panels(
        k: &mut Kernel,
        n: usize,
        length: f64,
        height: f64,
        spacing: f64,
    ) -> Brep {
        let mut b = crate::topology::brep::p3::new_brep(
            k,
            crate::geom::tol::USE_DEFAULT,
            crate::geom::tol::USE_DEFAULT,
        )
        .unwrap();
        let mut state = ArenaNotifyState::new(&b);
        let mut env = state.env(None);
        let mut marks = Marks::default();
        let this_face = |f: FaceId| loopuse::notify_ref(EntityRef::Face(f));
        let mut fenv = FaceBuildEnv {
            env: &mut env,
            marks: &mut marks,
            this_face: &this_face,
        };
        let root = std::f64::consts::FRAC_1_SQRT_2;
        let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        for i in 0..n {
            let off = i as f64 * spacing * root;
            let (ox, oy) = (off, -off);
            let (dx, dy) = (length * root, length * root);
            let corner = |a: f64, z: f64| CPoint::new(ox + a * dx, oy + a * dy, z, NOW);
            let net = vec![
                vec![corner(0.0, 0.0), corner(0.0, height)],
                vec![corner(1.0, 0.0), corner(1.0, height)],
            ];
            let knots = vec![0.0, 0.0, 1.0, 1.0];
            let patch = Net::new(net, 1, 1, knots.clone(), knots).unwrap();
            let surface = Surface::from(BSplineSurface::from_gw_nurb(&patch));
            create_face_from_surface(k, &mut b, Some(surface), &domain, &mut fenv).unwrap();
        }
        b.refresh_cached_topology();
        b
    }

    /// Both searches on one body: the pruned and the unpruned reference, each with its
    /// reports; returns `(pairs, reports)` of the pruned one after asserting they agree.
    fn both(k: &Kernel, body: &Brep, find_all: bool) -> (Vec<(EntityRef, EntityRef)>, String) {
        let identities = body.clone_standing_for_self().unwrap();
        let keys = |e| identities.owner_identity(e);
        let run = |prune: bool| {
            let mut b = body.clone_standing_for_self().unwrap();
            let mut reports = AssertArray::new();
            let r = search(
                k,
                &mut b,
                -1.0,
                find_all,
                Some(&mut reports),
                2,
                &keys,
                prune,
            )
            .unwrap();
            (r.found, r.pairs, format!("{:?}", reports.reports()))
        };
        let (found_a, pairs_a, reports_a) = run(true);
        let (found_b, pairs_b, reports_b) = run(false);
        assert_eq!(found_a, found_b);
        assert_eq!(pairs_a, pairs_b);
        assert_eq!(reports_a, reports_b);
        (pairs_a, reports_a)
    }

    #[test]
    fn pruned_and_unpruned_searches_agree_on_the_primitives() {
        let mut k = Kernel::default();
        let boxed = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0, 30.0).unwrap();
        let cylinder = create_cylinder(&mut k, Point3::new(1.0, 2.0, 3.0), 4.0, 9.0).unwrap();
        for body in [&boxed, &cylinder] {
            for find_all in [false, true] {
                let (pairs, _) = both(&k, body, find_all);
                assert!(pairs.is_empty());
            }
        }
    }

    #[test]
    fn pruned_and_unpruned_searches_agree_on_b_spline_panels() {
        let mut k = Kernel::default();
        let body = panels(&mut k, 5, 40.0, 1.0, 3.0);
        assert_eq!(body.faces().len(), 5);
        for find_all in [false, true] {
            let (pairs, _) = both(&k, &body, find_all);
            assert!(pairs.is_empty());
        }
    }

    /// Panel 1's first corner placed `gap` off the middle of panel 0, across it: a pair
    /// exactly when `gap <= tol`, found in both searches, and the same pairs and reports
    /// either side of the margin the prune uses (`2 * tol`).
    #[test]
    fn a_vertex_just_inside_and_just_past_the_margin_gives_the_same_pairs() {
        let mut k = Kernel::default();
        let mut body = panels(&mut k, 3, 40.0, 10.0, 7.0);
        let tol = body.tolerance();
        let vertices = brep1::vertices(&body, None).unwrap();
        let faces = brep1::faces(&body, None).unwrap();
        let root = std::f64::consts::FRAC_1_SQRT_2;
        // Middle of panel 0: x = y = 20 root, z = 5; its normal is (1, -1, 0) / sqrt 2.
        let middle = Point3::new(20.0 * root, 20.0 * root, 5.0);
        let moved = vertices[4];
        let mut outcomes = Vec::new();
        for factor in [0.0, 0.5, 0.99, 1.01, 1.5, 1.99, 2.01, 3.0, 100.0] {
            body.vertex_mut(moved).unwrap().point =
                middle + Vec3::new(root, -root, 0.0) * (factor * tol);
            let (pairs, _) = both(&k, &body, true);
            let hit = pairs.contains(&(EntityRef::Vertex(moved), EntityRef::Face(faces[0])));
            outcomes.push((factor, hit));
        }
        for (factor, hit) in outcomes {
            assert_eq!(hit, factor <= 1.0, "gap = {factor} tol");
        }
    }
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

    /// Edge/face and vertex/face queries of `panels` bodies, pruned or not.
    fn panel_queries(n: usize, prune: bool) -> (u64, u64) {
        let mut k = Kernel::default();
        let mut body = tests::panels(&mut k, n, 200.0, 1.0, 4.0);
        let identities = body.clone_standing_for_self().unwrap();
        let keys = |e| identities.owner_identity(e);
        let _ = k.take_profile();
        // The public entry point is the pruned search; `search` alone is the reference.
        let result = if prune {
            contains_coincident_topology(&k, &mut body, -1.0, true, None, 0, &keys)
        } else {
            search(&k, &mut body, -1.0, true, None, 0, &keys, false)
        }
        .unwrap();
        assert!(result.pairs.is_empty());
        let profile = k.take_profile();
        (
            count(
                &profile,
                "validation::coincidence::edge_face",
                "coincidence_queries",
            ),
            count(
                &profile,
                "validation::coincidence::vertex_face",
                "coincidence_queries",
            ),
        )
    }

    /// Parallel diagonal panels whose single boxes all overlap (the 32 of them span 88
    /// units across a 141 unit box): without the span prune every panel's edges are
    /// solved against every other panel, so queries grow with the square of the panels.
    /// The certified pieces of a panel (3 splits) only reach a few neighbours, so they
    /// grow with the panels.
    #[test]
    fn b_spline_panels_cost_queries_in_proportion_not_in_the_square() {
        // Measured edge/face queries: 8 panels 108 pruned, 112 plain; 32 panels 684
        // pruned, 1984 plain. Plain grows 17.7 times for 4 times the panels (the
        // square, 16, plus the panels' own short edges); pruned 6.3 times.
        let (few_pruned, _) = panel_queries(8, true);
        let (many_pruned, _) = panel_queries(32, true);
        let (few_plain, _) = panel_queries(8, false);
        let (many_plain, _) = panel_queries(32, false);
        assert!(
            many_plain > 16 * few_plain,
            "plain {few_plain} -> {many_plain}"
        );
        assert!(
            many_pruned < 8 * few_pruned,
            "pruned {few_pruned} -> {many_pruned}"
        );
        assert!(
            2 * many_pruned < many_plain,
            "{many_pruned} against {many_plain}"
        );
    }
}
