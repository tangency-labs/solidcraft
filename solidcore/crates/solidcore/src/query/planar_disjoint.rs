//! Certified interior-disjointness test for two closed manifold solids whose faces in the
//! overlap region are planar polygons with straight edges (not upstream).
//!
//! When a boolean intersection fails on coincident or near-coincident faces (for example a
//! tenon flush inside a matching slot with vertices within tens of microns), this module
//! certifies whether `int(a) ∩ int(b) == ∅` without constructing a merged B-rep:
//!
//! - [`Verdict::Yes`]: mathematically certified that the two solids share no interior
//!   volume (pure contact or disjoint).
//! - [`Verdict::No`]: proved interior overlap (a vertex, edge sub-segment, face interior,
//!   same-normal coplanar patch, or transverse face crossing lies in the other solid's
//!   interior).
//! - [`Verdict::Undecided`]: any face or edge in the overlap region is curved, degenerate,
//!   non-manifold, warped off its plane, or fails strict classification. Any uncertainty
//!   yields [`Verdict::Undecided`], never a false clearance.

use std::collections::{BTreeMap, BTreeSet};

use crate::curve::curve::CurveKind;
use crate::geom::certified::Verdict;
use crate::geom::tol::{brep_zone_tol_3d, SmallTopoType};
use crate::intersect::point_class::PointClassType;
use crate::kernel::Kernel;
use crate::mass::integrator::properties::region_material;
use crate::math::{Point3, Vec3};
use crate::query::api_queries::{face_bounding_box, face_normal_at_edge};
use crate::query::brep::BrepClassifier;
use crate::surface::surface::SurfaceKind;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::face::p2::{face_side_regions, faceuses};
use crate::topology::{loopuse, Brep, EdgeId, FaceId, FaceuseId};
use crate::{EntityRef, Result};

/// Operation name used for cancellation checks.
const OP: &str = "query::contact::planar_interiors_disjoint";

/// Maximum number of overlap-region faces per body inspected by the planar proof.
const MAX_OVERLAP_FACES: usize = 256;

/// Maximum segment-segment intersection tests per coplanar polygon overlap check.
const MAX_SEG_PAIRS: usize = 65_536;

/// Maximum horizontal scanline events processed in a 2D polygon sweep.
const MAX_Y_EVENTS: usize = 16_384;

/// Iteration stride between cooperative cancellation checks in tight inner loops.
const CANCEL_STRIDE: usize = 256;

/// Axis-aligned bounding box.
#[derive(Clone, Copy, Debug)]
struct Aabb {
    lo: Point3,
    hi: Point3,
}

impl Aabb {
    fn from_pair(lo: Point3, hi: Point3) -> Option<Self> {
        if lo.x <= hi.x
            && lo.y <= hi.y
            && lo.z <= hi.z
            && lo.x.is_finite()
            && lo.y.is_finite()
            && lo.z.is_finite()
            && hi.x.is_finite()
            && hi.y.is_finite()
            && hi.z.is_finite()
        {
            Some(Self { lo, hi })
        } else {
            None
        }
    }

    fn intersect(self, other: Self) -> Option<Self> {
        let lo = Point3::new(
            self.lo.x.max(other.lo.x),
            self.lo.y.max(other.lo.y),
            self.lo.z.max(other.lo.z),
        );
        let hi = Point3::new(
            self.hi.x.min(other.hi.x),
            self.hi.y.min(other.hi.y),
            self.hi.z.min(other.hi.z),
        );
        Self::from_pair(lo, hi)
    }

    fn inflated(self, margin: f64) -> Self {
        let m = Vec3::new(margin, margin, margin);
        Self {
            lo: self.lo - m,
            hi: self.hi + m,
        }
    }

    fn meets(self, other: Self) -> bool {
        self.lo.x <= other.hi.x
            && self.hi.x >= other.lo.x
            && self.lo.y <= other.hi.y
            && self.hi.y >= other.lo.y
            && self.lo.z <= other.hi.z
            && self.hi.z >= other.lo.z
    }

    fn from_segment(a: Point3, b: Point3, margin: f64) -> Self {
        Self {
            lo: Point3::new(
                a.x.min(b.x) - margin,
                a.y.min(b.y) - margin,
                a.z.min(b.z) - margin,
            ),
            hi: Point3::new(
                a.x.max(b.x) + margin,
                a.y.max(b.y) + margin,
                a.z.max(b.z) + margin,
            ),
        }
    }
}

/// One straight edge of a planar face, in loop order.
#[derive(Clone, Copy, Debug)]
struct PlanarEdge {
    edge_id: EdgeId,
    start: Point3,
    end: Point3,
    start_2d: [f64; 2],
    end_2d: [f64; 2],
}

/// A planar face in the overlap region, extracted as exact 3D and 2D polygon loops.
struct PlanarFace {
    face_id: FaceId,
    origin: Point3,
    normal: Vec3,
    u: Vec3,
    v: Vec3,
    bbox: Aabb,
    loops_3d: Vec<Vec<Point3>>,
    loops_2d: Vec<Vec<[f64; 2]>>,
    edges: Vec<PlanarEdge>,
}

impl PlanarFace {
    fn project_2d(&self, p: Point3) -> [f64; 2] {
        let d = p - self.origin;
        [d.dot(self.u), d.dot(self.v)]
    }

    fn lift_3d(&self, uv: [f64; 2]) -> Point3 {
        self.origin + self.u * uv[0] + self.v * uv[1]
    }
}

/// Strict classification of a 3D point against a solid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StrictPoint {
    Inside,
    Outside,
    OnBoundary,
    Undecided,
}

/// Classification of a 2D point against a planar polygon with optional holes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PolyClass {
    Inside,
    Boundary,
    Outside,
}

/// One span of an intersection line clipped against a planar face.
#[derive(Clone, Copy, Debug)]
struct LineSpan {
    s0: f64,
    s1: f64,
    kind: SpanKind,
}

#[derive(Clone, Copy, Debug)]
enum SpanKind {
    Interior,
    BoundaryEdge { edge_id: EdgeId, inward_3d: Vec3 },
}

/// Whether `a` and `b` are proved to share no interior volume when every face meeting their
/// overlap box is planar with straight edges.
pub(crate) fn planar_interiors_disjoint(
    k: &Kernel,
    a: &mut Brep,
    b: &mut Brep,
    box_a: (Point3, Point3),
    box_b: (Point3, Point3),
) -> Result<Verdict> {
    let (Some(ba), Some(bb)) = (
        Aabb::from_pair(box_a.0, box_a.1),
        Aabb::from_pair(box_b.0, box_b.1),
    ) else {
        return Ok(Verdict::Undecided);
    };
    let Some(overlap) = ba.intersect(bb) else {
        return Ok(Verdict::Yes);
    };
    let union_lo = Point3::new(
        ba.lo.x.min(bb.lo.x),
        ba.lo.y.min(bb.lo.y),
        ba.lo.z.min(bb.lo.z),
    );
    let union_hi = Point3::new(
        ba.hi.x.max(bb.hi.x),
        ba.hi.y.max(bb.hi.y),
        ba.hi.z.max(bb.hi.z),
    );
    let scale = (union_hi - union_lo).length().max(1.0);
    if !scale.is_finite() {
        return Ok(Verdict::Undecided);
    }

    let tol_a = brep_zone_tol_3d(k, a, SmallTopoType::NormalSize);
    let tol_b = brep_zone_tol_3d(k, b, SmallTopoType::NormalSize);
    let tol_max = tol_a.max(tol_b).max(1e-5);
    let eps = 1e-9 * scale;
    let region = overlap.inflated(16.0 * tol_max);

    let Some(faces_a) = extract_planar_faces(k, a, region, eps)? else {
        return Ok(Verdict::Undecided);
    };
    let Some(faces_b) = extract_planar_faces(k, b, region, eps)? else {
        return Ok(Verdict::Undecided);
    };

    // Map each edge id to all overlap-region faces incident to it (for reflex-edge checks).
    let edge_faces_a = index_edge_faces(&faces_a);
    let edge_faces_b = index_edge_faces(&faces_b);

    // Track `Undecided` without short-circuiting so a proved `Verdict::No` overlap in a later
    // check always wins over a sub-tolerance `Undecided` in an earlier check. A false `Yes` is
    // impossible because `undecided` prevents returning `Verdict::Yes`.
    let mut undecided = false;
    let mut record = |v: Verdict| -> Option<Verdict> {
        match v {
            Verdict::Yes => None,
            Verdict::No => Some(Verdict::No),
            Verdict::Undecided => {
                undecided = true;
                None
            }
        }
    };

    // Check A: every vertex of `faces_a` against `b`, and vice versa.
    if let Some(no) = record(check_vertices(k, &faces_a, b, &faces_b, tol_b, eps)?) {
        return Ok(no);
    }
    if let Some(no) = record(check_vertices(k, &faces_b, a, &faces_a, tol_a, eps)?) {
        return Ok(no);
    }

    // Check B: every edge of `faces_a` split at every face plane of `faces_b`, and vice versa.
    if let Some(no) = record(check_edges(k, &faces_a, b, &faces_b, tol_b, eps)?) {
        return Ok(no);
    }
    if let Some(no) = record(check_edges(k, &faces_b, a, &faces_a, tol_a, eps)?) {
        return Ok(no);
    }

    // Check C: face-face coplanar and transverse crossing analysis.
    if let Some(no) = record(check_face_pairs(
        k,
        &faces_a,
        &faces_b,
        &edge_faces_a,
        &edge_faces_b,
        tol_max,
        eps,
    )?) {
        return Ok(no);
    }

    // Check D: one certified strict interior point of every face in `faces_a` against `b`,
    // and vice versa (catches full-face containment whose boundary lies on the other solid's
    // edges, such as an equatorial slice of an octahedron).
    if let Some(no) = record(check_face_interiors(k, &faces_a, b, &faces_b, tol_b, eps)?) {
        return Ok(no);
    }
    if let Some(no) = record(check_face_interiors(k, &faces_b, a, &faces_a, tol_a, eps)?) {
        return Ok(no);
    }

    Ok(if undecided {
        Verdict::Undecided
    } else {
        Verdict::Yes
    })
}

/// Extracts all faces of `brep` whose bounding boxes meet `region`. Returns `Ok(None)` if
/// any such face is non-planar, has curved edges, is non-manifold, deviates from its plane,
/// or exceeds [`MAX_OVERLAP_FACES`].
fn extract_planar_faces(
    k: &Kernel,
    brep: &mut Brep,
    region: Aabb,
    eps: f64,
) -> Result<Option<Vec<PlanarFace>>> {
    let face_ids: Vec<FaceId> = brep.face_list().to_vec();
    let mut out = Vec::new();
    for f in face_ids {
        k.check_cancelled("query::contact::planar_interiors_disjoint")?;
        let fb = face_bounding_box(k, brep, f, false)?;
        let Some(bbox) = Aabb::from_pair(fb.min(), fb.max()) else {
            return Ok(None);
        };
        if !bbox.meets(region) {
            continue;
        }
        if out.len() >= MAX_OVERLAP_FACES {
            return Ok(None);
        }
        let Some(pf) = extract_single_planar_face(k, brep, f, bbox, eps)? else {
            return Ok(None);
        };
        out.push(pf);
    }
    Ok(Some(out))
}

/// Finds the void-side faceuse of a solid boundary face `f` (pointing from material into
/// void, including internal cavities). Returns `None` for a partition or degenerate face.
fn void_side_faceuse(brep: &Brep, f: FaceId) -> Result<Option<FaceuseId>> {
    let (fu0, fu1) = faceuses(brep, f)?;
    let [r0, r1] = face_side_regions(brep, f)?;
    let (Some(r0), Some(r1)) = (r0, r1) else {
        return Ok(None);
    };
    let m0 = region_material(brep, r0)?;
    let m1 = region_material(brep, r1)?;
    if m0 == m1 {
        return Ok(None);
    }
    Ok(if m0 { fu1 } else { fu0 })
}

/// Extracts one planar face with straight line edges and a verified outward unit normal.
fn extract_single_planar_face(
    k: &Kernel,
    brep: &mut Brep,
    f: FaceId,
    bbox: Aabb,
    eps: f64,
) -> Result<Option<PlanarFace>> {
    let (support_origin, support_normal) = {
        let face_rec = brep.face(f)?;
        let Some(surf) = face_rec.surface.as_ref() else {
            return Ok(None);
        };
        match surf.kind() {
            SurfaceKind::Plane(plane) => {
                let pos = plane.position();
                let Ok(sn) = pos.z_axis().unitize() else {
                    return Ok(None);
                };
                let so = pos.origin();
                if !(so.x.is_finite()
                    && so.y.is_finite()
                    && so.z.is_finite()
                    && sn.x.is_finite()
                    && sn.y.is_finite()
                    && sn.z.is_finite())
                {
                    return Ok(None);
                }
                (so, sn)
            }
            _ => return Ok(None),
        }
    };
    let Some(fu) = void_side_faceuse(brep, f)? else {
        return Ok(None);
    };
    let loopuses = eu1::faceuse_loopuses(brep, fu, false, None)?;
    if loopuses.is_empty() {
        return Ok(None);
    }

    let mut first_edge: Option<EdgeId> = None;
    let mut raw_loops: Vec<Vec<(EdgeId, Point3)>> = Vec::new();
    for lu in loopuses {
        let eus = loopuse::edgeuses(brep, lu, None, None)?;
        if eus.len() < 3 {
            return Ok(None);
        }
        let mut loop_items = Vec::new();
        for eu in eus {
            let eu_rec = brep.edgeuse(eu)?;
            let (Some(e_id), Some(vu_id)) = (eu_rec.edge, eu_rec.vertexuse) else {
                return Ok(None);
            };
            let Some(v_id) = brep.vertexuse(vu_id)?.vertex else {
                return Ok(None);
            };
            let ed = brep.edge(e_id)?;
            let Some(curve) = ed.curve.as_ref() else {
                return Ok(None);
            };
            if !matches!(curve.kind(), CurveKind::Line(_)) {
                return Ok(None);
            }
            let pt = brep.vertex(v_id)?.point;
            if !(pt.x.is_finite() && pt.y.is_finite() && pt.z.is_finite()) {
                return Ok(None);
            }
            if first_edge.is_none() {
                first_edge = Some(e_id);
            }
            loop_items.push((e_id, pt));
        }
        raw_loops.push(loop_items);
    }

    let Some(e0) = first_edge else {
        return Ok(None);
    };
    let Ok(frame) = face_normal_at_edge(k, brep, e0, f, fu) else {
        return Ok(None);
    };
    let Ok(normal) = frame.normal.value.unitize() else {
        return Ok(None);
    };
    if !(normal.x.is_finite() && normal.y.is_finite() && normal.z.is_finite())
        || normal.dot(support_normal).abs() < 1.0 - 1e-10
    {
        return Ok(None);
    }
    let Ok([_, y_ref, z_ref]) = normal.make_unit_ortho_vectors(None) else {
        return Ok(None);
    };
    let (u, v) = if y_ref.cross(z_ref).dot(normal) < 0.0 {
        (z_ref, y_ref)
    } else {
        (y_ref, z_ref)
    };

    // Anchor the local 2D frame on the exact analytic support plane near the loop vertices,
    // and certify that every loop vertex lies on the analytic support plane within `eps`.
    let anchor = raw_loops
        .first()
        .and_then(|l| l.first())
        .map_or(support_origin, |&(_, p)| p);
    let plane_origin = anchor - normal * (anchor - support_origin).dot(normal);
    let mut loops_3d = Vec::new();
    let mut loops_2d = Vec::new();
    let mut edges = Vec::new();
    for loop_items in raw_loops {
        let n_pts = loop_items.len();
        let mut pts_3d = Vec::new();
        let mut pts_2d = Vec::new();
        for &(_, p) in &loop_items {
            if (p - support_origin).dot(normal).abs() > eps
                || (p - plane_origin).dot(normal).abs() > eps
            {
                return Ok(None);
            }
            let d = p - plane_origin;
            pts_3d.push(p);
            pts_2d.push([d.dot(u), d.dot(v)]);
        }
        for i in 0..n_pts {
            let next = if i + 1 < n_pts { i + 1 } else { 0 };
            let (Some(&(edge_id, start)), Some(&(_, end))) =
                (loop_items.get(i), loop_items.get(next))
            else {
                return Ok(None);
            };
            let (Some(&start_2d), Some(&end_2d)) = (pts_2d.get(i), pts_2d.get(next)) else {
                return Ok(None);
            };
            if start.distance(end) <= 1e-12 {
                return Ok(None);
            }
            edges.push(PlanarEdge {
                edge_id,
                start,
                end,
                start_2d,
                end_2d,
            });
        }
        loops_3d.push(pts_3d);
        loops_2d.push(pts_2d);
    }

    Ok(Some(PlanarFace {
        face_id: f,
        origin: plane_origin,
        normal,
        u,
        v,
        bbox,
        loops_3d,
        loops_2d,
        edges,
    }))
}

fn index_edge_faces(faces: &[PlanarFace]) -> BTreeMap<EdgeId, Vec<usize>> {
    let mut map: BTreeMap<EdgeId, Vec<usize>> = BTreeMap::new();
    for (idx, face) in faces.iter().enumerate() {
        for edge in &face.edges {
            let list = map.entry(edge.edge_id).or_default();
            if !list.contains(&idx) {
                list.push(idx);
            }
        }
    }
    map
}

/// Classifies `p` strictly against `brep` (whose overlap-region faces are `faces`) with no
/// sub-tolerance blind spot.
fn classify_point_strict(
    k: &Kernel,
    brep: &Brep,
    faces: &[PlanarFace],
    p: Point3,
    tol: f64,
    eps: f64,
) -> Result<StrictPoint> {
    let mut d_min = f64::INFINITY;
    let mut second_d_min = f64::INFINITY;
    let mut nearest: Option<&PlanarFace> = None;
    for face in faces {
        let d = point_to_face_distance_3d(face, p);
        if d < d_min {
            second_d_min = d_min;
            d_min = d;
            nearest = Some(face);
        } else if d < second_d_min {
            second_d_min = d;
        }
        if d_min <= eps {
            return Ok(StrictPoint::OnBoundary);
        }
    }

    // Use the body's normal tolerance when clear of all overlap faces, or a tightened
    // tolerance strictly below `d_min` inside the sub-tolerance band so a shallow penetration
    // (such as 5e-6 mm inside a 1e-5 mm tolerance) is never misclassified as OnBoundary.
    let query_tol = if d_min > 2.0 * tol {
        tol
    } else {
        (0.25 * d_min).max(eps * 0.25)
    };
    let mut classifier = BrepClassifier::of_brep(brep);
    let pc = match classifier.point_3d_classify(k, p, query_tol, true) {
        Ok(pc) => pc,
        Err(e) if e.is_cancelled() => return Err(e),
        Err(_) => return Ok(StrictPoint::Undecided),
    };
    Ok(match (pc.point_class(), pc.object()) {
        (PointClassType::Region, Some(EntityRef::Region(r))) => {
            if region_material(classifier.brep(), r)? {
                StrictPoint::Inside
            } else {
                StrictPoint::Outside
            }
        }
        _ => {
            // In the sub-tolerance band where `p` is strictly off every face (`d_min > eps`)
            // yet the B-rep's own face tolerance absorbed `point_3d_classify`, a unique
            // nearest planar face whose 2D interior contains `p`'s projection certifies the
            // exact side from its outward normal.
            nearest
                .filter(|face| {
                    d_min <= 2.0 * tol
                        && d_min + eps < second_d_min
                        && point_in_polygon_2d(&face.loops_2d, face.project_2d(p), eps)
                            == PolyClass::Inside
                })
                .map_or(StrictPoint::Undecided, |face| {
                    let signed = (p - face.origin).dot(face.normal);
                    if signed < -eps {
                        StrictPoint::Inside
                    } else if signed > eps {
                        StrictPoint::Outside
                    } else {
                        StrictPoint::Undecided
                    }
                })
        }
    })
}

fn check_vertices(
    k: &Kernel,
    from_faces: &[PlanarFace],
    other: &Brep,
    other_faces: &[PlanarFace],
    other_tol: f64,
    eps: f64,
) -> Result<Verdict> {
    let mut tested: Vec<Point3> = Vec::new();
    let mut undecided = false;
    for face in from_faces {
        for lp in &face.loops_3d {
            for &p in lp {
                if tested.iter().any(|&q| q.distance(p) <= eps) {
                    continue;
                }
                tested.push(p);
                k.check_cancelled("query::contact::planar_interiors_disjoint")?;
                match classify_point_strict(k, other, other_faces, p, other_tol, eps)? {
                    StrictPoint::Inside => return Ok(Verdict::No),
                    StrictPoint::Undecided => undecided = true,
                    StrictPoint::Outside | StrictPoint::OnBoundary => {}
                }
            }
        }
    }
    Ok(if undecided {
        Verdict::Undecided
    } else {
        Verdict::Yes
    })
}

fn check_edges(
    k: &Kernel,
    from_faces: &[PlanarFace],
    other: &Brep,
    other_faces: &[PlanarFace],
    other_tol: f64,
    eps: f64,
) -> Result<Verdict> {
    let mut seen_edges: BTreeSet<EdgeId> = BTreeSet::new();
    let mut undecided = false;
    for face in from_faces {
        for edge in &face.edges {
            if !seen_edges.insert(edge.edge_id) {
                continue;
            }
            k.check_cancelled("query::contact::planar_interiors_disjoint")?;
            let dir = edge.end - edge.start;
            let len = dir.length();
            if !(len > eps && len.is_finite()) {
                undecided = true;
                continue;
            }
            let e_box = Aabb::from_segment(edge.start, edge.end, eps);
            let mut params = vec![0.0_f64, 1.0_f64];
            for other_face in other_faces {
                if !e_box.meets(other_face.bbox) {
                    continue;
                }
                let denom = dir.dot(other_face.normal);
                if denom.abs() <= 1e-12 * len {
                    continue;
                }
                let t = (other_face.origin - edge.start).dot(other_face.normal) / denom;
                if t > 0.0 && t < 1.0 && t.is_finite() {
                    params.push(t);
                }
            }
            params.sort_by(f64::total_cmp);
            let merged = dedup_sorted(&params, (eps / len).max(1e-12));
            for pair in merged.windows(2) {
                let (Some(&t0), Some(&t1)) = (pair.first(), pair.get(1)) else {
                    continue;
                };
                if (t1 - t0) * len <= eps {
                    continue;
                }
                let mid = edge.start + dir * (0.5 * (t0 + t1));
                match classify_point_strict(k, other, other_faces, mid, other_tol, eps)? {
                    StrictPoint::Inside => return Ok(Verdict::No),
                    StrictPoint::Undecided => undecided = true,
                    StrictPoint::Outside | StrictPoint::OnBoundary => {}
                }
            }
        }
    }
    Ok(if undecided {
        Verdict::Undecided
    } else {
        Verdict::Yes
    })
}

fn check_face_interiors(
    k: &Kernel,
    from_faces: &[PlanarFace],
    other: &Brep,
    other_faces: &[PlanarFace],
    other_tol: f64,
    eps: f64,
) -> Result<Verdict> {
    let mut undecided = false;
    for face in from_faces {
        k.check_cancelled(OP)?;
        let Some(pt) = face_interior_point_3d(k, face, eps)? else {
            undecided = true;
            continue;
        };
        match classify_point_strict(k, other, other_faces, pt, other_tol, eps)? {
            StrictPoint::Inside => return Ok(Verdict::No),
            StrictPoint::Undecided => undecided = true,
            StrictPoint::Outside | StrictPoint::OnBoundary => {}
        }
    }
    Ok(if undecided {
        Verdict::Undecided
    } else {
        Verdict::Yes
    })
}

fn check_face_pairs(
    k: &Kernel,
    faces_a: &[PlanarFace],
    faces_b: &[PlanarFace],
    edge_faces_a: &BTreeMap<EdgeId, Vec<usize>>,
    edge_faces_b: &BTreeMap<EdgeId, Vec<usize>>,
    tol_max: f64,
    eps: f64,
) -> Result<Verdict> {
    let mut undecided = false;
    for f in faces_a {
        k.check_cancelled(OP)?;
        let f_box = f.bbox.inflated(2.0 * tol_max);
        for g in faces_b {
            if !f_box.meets(g.bbox) {
                continue;
            }
            match check_single_face_pair(
                k,
                f,
                g,
                faces_a,
                faces_b,
                edge_faces_a,
                edge_faces_b,
                tol_max,
                eps,
            )? {
                Verdict::Yes => {}
                Verdict::No => return Ok(Verdict::No),
                Verdict::Undecided => undecided = true,
            }
        }
    }
    Ok(if undecided {
        Verdict::Undecided
    } else {
        Verdict::Yes
    })
}

fn signed_range_on_plane(
    face: &PlanarFace,
    plane_origin: Point3,
    plane_normal: Vec3,
) -> (f64, f64) {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for lp in &face.loops_3d {
        for &p in lp {
            let d = (p - plane_origin).dot(plane_normal);
            lo = lo.min(d);
            hi = hi.max(d);
        }
    }
    (lo, hi)
}

fn check_single_face_pair(
    k: &Kernel,
    f: &PlanarFace,
    g: &PlanarFace,
    faces_a: &[PlanarFace],
    faces_b: &[PlanarFace],
    edge_faces_a: &BTreeMap<EdgeId, Vec<usize>>,
    edge_faces_b: &BTreeMap<EdgeId, Vec<usize>>,
    tol_max: f64,
    eps: f64,
) -> Result<Verdict> {
    let (min_g_on_f, max_g_on_f) = signed_range_on_plane(g, f.origin, f.normal);
    let (min_f_on_g, max_f_on_g) = signed_range_on_plane(f, g.origin, g.normal);
    let ndot = f.normal.dot(g.normal);

    // Case C1: Every vertex of each face lies on the other's plane within `eps` (truly coplanar).
    if min_g_on_f.abs().max(max_g_on_f.abs()) <= eps
        && min_f_on_g.abs().max(max_f_on_g.abs()) <= eps
    {
        if ndot > 0.0 {
            let g_in_f: Vec<Vec<[f64; 2]>> = g
                .loops_3d
                .iter()
                .map(|lp| lp.iter().map(|&p| f.project_2d(p)).collect())
                .collect();
            match polygons_2d_interiors_overlap(k, &f.loops_2d, &g_in_f, eps)? {
                Some(true) => return Ok(Verdict::No),
                Some(false) => {}
                None => return Ok(Verdict::Undecided),
            }
        }
        return Ok(Verdict::Yes);
    }

    // Sub-tolerance opposite-normal slab guard: if `g` lies strictly on the inward side of `f`
    // within `2 * tol_max` (and opposite normal), overlapping 2D projections prove a thin slab
    // penetration even below normal B-rep tolerance.
    if max_g_on_f < -eps && max_g_on_f >= -2.0 * tol_max && ndot < -0.9999 {
        let g_in_f: Vec<Vec<[f64; 2]>> = g
            .loops_3d
            .iter()
            .map(|lp| lp.iter().map(|&p| f.project_2d(p)).collect())
            .collect();
        match polygons_2d_interiors_overlap(k, &f.loops_2d, &g_in_f, eps)? {
            Some(true) => return Ok(Verdict::No),
            Some(false) => {}
            None => return Ok(Verdict::Undecided),
        }
    }

    // Strictly separated on one side of either plane: cannot cross.
    if min_g_on_f > eps || max_g_on_f < -eps || min_f_on_g > eps || max_f_on_g < -eps {
        return Ok(Verdict::Yes);
    }

    // Case C2: Non-coplanar faces whose planes intersect across both faces.
    let cross = f.normal.cross(g.normal);
    let c2 = cross.dot(cross);
    if !(c2 > 1e-24 && c2.is_finite()) {
        return Ok(Verdict::Undecided);
    }
    let dir = cross / c2.sqrt();
    let df = f.origin.dot(f.normal);
    let dg = g.origin.dot(g.normal);
    let p_line_raw = (g.normal.cross(cross) * df + cross.cross(f.normal) * dg) / c2;
    let p_line = p_line_raw + dir * (f.origin - p_line_raw).dot(dir);
    if !(p_line.x.is_finite() && p_line.y.is_finite() && p_line.z.is_finite())
        || (p_line - f.origin).dot(f.normal).abs() > eps * 10.0
        || (p_line - g.origin).dot(g.normal).abs() > eps * 10.0
    {
        return Ok(Verdict::Undecided);
    }

    let f_dir = [dir.dot(f.u), dir.dot(f.v)];
    let g_dir = [dir.dot(g.u), dir.dot(g.v)];
    let Some(spans_f) = clip_line_to_face_2d(f, f.project_2d(p_line), f_dir, eps) else {
        return Ok(Verdict::Undecided);
    };
    let Some(spans_g) = clip_line_to_face_2d(g, g.project_2d(p_line), g_dir, eps) else {
        return Ok(Verdict::Undecided);
    };

    for sf in &spans_f {
        for sg in &spans_g {
            let s0 = sf.s0.max(sg.s0);
            let s1 = sf.s1.min(sg.s1);
            if s1 - s0 <= eps {
                continue;
            }
            match (sf.kind, sg.kind) {
                (SpanKind::Interior, SpanKind::Interior) => return Ok(Verdict::No),
                (SpanKind::Interior, SpanKind::BoundaryEdge { edge_id, inward_3d }) => {
                    match check_edge_touching_face_interior(
                        f.normal,
                        g.face_id,
                        edge_id,
                        inward_3d,
                        faces_b,
                        edge_faces_b,
                        eps,
                    ) {
                        Verdict::Yes => {}
                        other => return Ok(other),
                    }
                }
                (SpanKind::BoundaryEdge { edge_id, inward_3d }, SpanKind::Interior) => {
                    match check_edge_touching_face_interior(
                        g.normal,
                        f.face_id,
                        edge_id,
                        inward_3d,
                        faces_a,
                        edge_faces_a,
                        eps,
                    ) {
                        Verdict::Yes => {}
                        other => return Ok(other),
                    }
                }
                (SpanKind::BoundaryEdge { .. }, SpanKind::BoundaryEdge { .. }) => {}
            }
        }
    }

    Ok(Verdict::Yes)
}

/// Checks an edge of one solid lying in the strict interior of a face of the other solid
/// (with outward normal `target_normal`). Safe only when the edge is convex and its incident
/// faces stay strictly on the outward (`+target_normal`) side.
fn check_edge_touching_face_interior(
    target_normal: Vec3,
    face_id: FaceId,
    edge_id: EdgeId,
    inward_3d: Vec3,
    own_faces: &[PlanarFace],
    own_edge_faces: &BTreeMap<EdgeId, Vec<usize>>,
    eps: f64,
) -> Verdict {
    let side = inward_3d.dot(target_normal);
    if side < -eps {
        return Verdict::No;
    }
    if side <= eps {
        return Verdict::Undecided;
    }
    // `inward_3d` points strictly into the outward (+target_normal) half-space. Verify that
    // the edge is convex in its own solid; a reflex (concave) edge has material wrapping into
    // the `-target_normal` half-space.
    let Some(incident) = own_edge_faces.get(&edge_id) else {
        return Verdict::Undecided;
    };
    let mut other_face: Option<&PlanarFace> = None;
    for &idx in incident {
        if let Some(cand) = own_faces.get(idx) {
            if cand.face_id != face_id {
                if other_face.is_some() {
                    return Verdict::Undecided;
                }
                other_face = Some(cand);
            }
        }
    }
    let Some(other) = other_face else {
        return Verdict::Undecided;
    };
    let convex_dot = inward_3d.dot(other.normal);
    if convex_dot < -eps {
        Verdict::Yes
    } else if convex_dot > eps {
        Verdict::No
    } else {
        Verdict::Undecided
    }
}

// ---- Exact 2D polygon helpers --------------------------------------------------------------

fn dot2(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn cross2(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn sub2(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn dist_point_to_segment_2d(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let ab = sub2(b, a);
    let len2 = dot2(ab, ab);
    if len2 <= 0.0 {
        let d = sub2(p, a);
        return dot2(d, d).sqrt();
    }
    let t = (dot2(sub2(p, a), ab) / len2).clamp(0.0, 1.0);
    let foot = [a[0] + t * ab[0], a[1] + t * ab[1]];
    let d = sub2(p, foot);
    dot2(d, d).sqrt()
}

fn point_in_polygon_2d(loops: &[Vec<[f64; 2]>], p: [f64; 2], eps: f64) -> PolyClass {
    let mut inside = false;
    for lp in loops {
        let n = lp.len();
        for i in 0..n {
            let next = if i + 1 < n { i + 1 } else { 0 };
            let (Some(&a), Some(&b)) = (lp.get(i), lp.get(next)) else {
                continue;
            };
            if dist_point_to_segment_2d(p, a, b) <= eps {
                return PolyClass::Boundary;
            }
            if (a[1] > p[1]) != (b[1] > p[1]) {
                let x_hit = a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
                if x_hit > p[0] {
                    inside = !inside;
                }
            }
        }
    }
    if inside {
        PolyClass::Inside
    } else {
        PolyClass::Outside
    }
}

fn point_to_face_distance_3d(face: &PlanarFace, p: Point3) -> f64 {
    let h = (p - face.origin).dot(face.normal).abs();
    let uv = face.project_2d(p);
    if point_in_polygon_2d(&face.loops_2d, uv, 0.0) != PolyClass::Outside {
        return h;
    }
    let mut d_edge = f64::INFINITY;
    for lp in &face.loops_2d {
        let n = lp.len();
        for i in 0..n {
            let next = if i + 1 < n { i + 1 } else { 0 };
            let (Some(&a), Some(&b)) = (lp.get(i), lp.get(next)) else {
                continue;
            };
            d_edge = d_edge.min(dist_point_to_segment_2d(uv, a, b));
        }
    }
    h.hypot(d_edge)
}

fn scanline_intervals_2d(loops: &[Vec<[f64; 2]>], y: f64) -> Vec<(f64, f64)> {
    let mut xs = Vec::new();
    for lp in loops {
        let n = lp.len();
        for i in 0..n {
            let next = if i + 1 < n { i + 1 } else { 0 };
            let (Some(&a), Some(&b)) = (lp.get(i), lp.get(next)) else {
                continue;
            };
            if (a[1] > y) != (b[1] > y) {
                let x = a[0] + (y - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
                if x.is_finite() {
                    xs.push(x);
                }
            }
        }
    }
    xs.sort_by(f64::total_cmp);
    let mut out = Vec::new();
    let mut iter = xs.into_iter();
    while let (Some(x0), Some(x1)) = (iter.next(), iter.next()) {
        out.push((x0, x1));
    }
    out
}

fn face_interior_point_3d(k: &Kernel, face: &PlanarFace, eps: f64) -> Result<Option<Point3>> {
    let mut ys = Vec::new();
    for lp in &face.loops_2d {
        for &p in lp {
            if ys.len() >= MAX_Y_EVENTS {
                return Ok(None);
            }
            ys.push(p[1]);
        }
    }
    ys.sort_by(f64::total_cmp);
    let ys = dedup_sorted(&ys, eps);
    let mut best: Option<(f64, [f64; 2])> = None;
    for (band_idx, pair) in ys.windows(2).enumerate() {
        if band_idx % CANCEL_STRIDE == 0 {
            k.check_cancelled(OP)?;
        }
        let (Some(&y0), Some(&y1)) = (pair.first(), pair.get(1)) else {
            continue;
        };
        if y1 - y0 <= 2.0 * eps {
            continue;
        }
        let y_mid = 0.5 * (y0 + y1);
        for (x0, x1) in scanline_intervals_2d(&face.loops_2d, y_mid) {
            if x1 - x0 <= 2.0 * eps {
                continue;
            }
            let cand = [0.5 * (x0 + x1), y_mid];
            let mut clearance = f64::INFINITY;
            for lp in &face.loops_2d {
                let n = lp.len();
                for i in 0..n {
                    let next = if i + 1 < n { i + 1 } else { 0 };
                    let (Some(&a), Some(&b)) = (lp.get(i), lp.get(next)) else {
                        continue;
                    };
                    clearance = clearance.min(dist_point_to_segment_2d(cand, a, b));
                }
            }
            if clearance > eps && best.is_none_or(|(b_d, _)| clearance > b_d) {
                best = Some((clearance, cand));
            }
        }
    }
    let Some((_, uv)) = best else {
        return Ok(None);
    };
    if point_in_polygon_2d(&face.loops_2d, uv, eps) != PolyClass::Inside {
        return Ok(None);
    }
    Ok(Some(face.lift_3d(uv)))
}

/// Checks whether two 2D polygons (each with optional holes) have overlapping interiors.
///
/// Returns `Ok(Some(true))` when overlap is proved, `Ok(Some(false))` when all segment pairs
/// and horizontal bands were checked without finding overlap, `Ok(None)` when the work budget
/// ([`MAX_SEG_PAIRS`] / [`MAX_Y_EVENTS`]) is exceeded (never returning a verdict from partial
/// events), or `Err` on cooperative cancellation.
fn polygons_2d_interiors_overlap(
    k: &Kernel,
    a: &[Vec<[f64; 2]>],
    b: &[Vec<[f64; 2]>],
    eps: f64,
) -> Result<Option<bool>> {
    let mut ys = Vec::new();
    for lp in a.iter().chain(b.iter()) {
        for &p in lp {
            if ys.len() >= MAX_Y_EVENTS {
                return Ok(None);
            }
            ys.push(p[1]);
        }
    }
    // Include y-coordinates of all proper segment intersections between edges of `a` and `b`.
    let mut pairs_checked = 0_usize;
    for la in a {
        let na = la.len();
        for i in 0..na {
            let next_a = if i + 1 < na { i + 1 } else { 0 };
            let (Some(&a0), Some(&a1)) = (la.get(i), la.get(next_a)) else {
                continue;
            };
            let da = sub2(a1, a0);
            for lb in b {
                let nb = lb.len();
                for j in 0..nb {
                    if pairs_checked >= MAX_SEG_PAIRS {
                        return Ok(None);
                    }
                    if pairs_checked % CANCEL_STRIDE == 0 {
                        k.check_cancelled(OP)?;
                    }
                    pairs_checked += 1;
                    let next_b = if j + 1 < nb { j + 1 } else { 0 };
                    let (Some(&b0), Some(&b1)) = (lb.get(j), lb.get(next_b)) else {
                        continue;
                    };
                    let db = sub2(b1, b0);
                    let denom = cross2(da, db);
                    if denom.abs() <= 1e-18 {
                        continue;
                    }
                    let diff = sub2(b0, a0);
                    let t = cross2(diff, db) / denom;
                    let u = cross2(diff, da) / denom;
                    if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
                        let y = a0[1] + t * da[1];
                        if y.is_finite() {
                            if ys.len() >= MAX_Y_EVENTS {
                                return Ok(None);
                            }
                            ys.push(y);
                        }
                    }
                }
            }
        }
    }
    ys.sort_by(f64::total_cmp);
    let ys = dedup_sorted(&ys, eps);
    for (band_idx, pair) in ys.windows(2).enumerate() {
        if band_idx % CANCEL_STRIDE == 0 {
            k.check_cancelled(OP)?;
        }
        let (Some(&y0), Some(&y1)) = (pair.first(), pair.get(1)) else {
            continue;
        };
        if y1 - y0 <= eps {
            continue;
        }
        let y_mid = 0.5 * (y0 + y1);
        let ivs_a = scanline_intervals_2d(a, y_mid);
        let ivs_b = scanline_intervals_2d(b, y_mid);
        for &(a0, a1) in &ivs_a {
            for &(b0, b1) in &ivs_b {
                if a1.min(b1) - a0.max(b0) > eps {
                    return Ok(Some(true));
                }
            }
        }
    }
    Ok(Some(false))
}

fn clip_line_to_face_2d(
    face: &PlanarFace,
    line_origin: [f64; 2],
    line_dir: [f64; 2],
    eps: f64,
) -> Option<Vec<LineSpan>> {
    let dir_len = dot2(line_dir, line_dir).sqrt();
    if !(dir_len > 1e-12 && dir_len.is_finite()) {
        return None;
    }
    let dir = [line_dir[0] / dir_len, line_dir[1] / dir_len];
    let perp = [-dir[1], dir[0]];

    let mut params = Vec::new();
    for edge in &face.edges {
        let da = cross2(dir, sub2(edge.start_2d, line_origin));
        let db = cross2(dir, sub2(edge.end_2d, line_origin));
        if da.abs() <= eps && db.abs() <= eps {
            params.push(dot2(sub2(edge.start_2d, line_origin), dir));
            params.push(dot2(sub2(edge.end_2d, line_origin), dir));
        } else if (da <= eps && db >= -eps) || (da >= -eps && db <= eps) {
            let denom = da - db;
            if denom.abs() > 1e-18 {
                let t = (da / denom).clamp(0.0, 1.0);
                let hit = [
                    edge.start_2d[0] + t * (edge.end_2d[0] - edge.start_2d[0]),
                    edge.start_2d[1] + t * (edge.end_2d[1] - edge.start_2d[1]),
                ];
                let s = dot2(sub2(hit, line_origin), dir);
                if s.is_finite() {
                    params.push(s);
                }
            }
        }
    }
    if params.len() < 2 {
        return Some(Vec::new());
    }
    params.sort_by(f64::total_cmp);
    let params = dedup_sorted(&params, eps);

    let mut out = Vec::new();
    for pair in params.windows(2) {
        let (Some(&s0), Some(&s1)) = (pair.first(), pair.get(1)) else {
            continue;
        };
        if s1 - s0 <= eps {
            continue;
        }
        let s_mid = 0.5 * (s0 + s1);
        let mid = [
            line_origin[0] + s_mid * dir[0],
            line_origin[1] + s_mid * dir[1],
        ];
        match point_in_polygon_2d(&face.loops_2d, mid, eps) {
            PolyClass::Outside => {}
            PolyClass::Inside => {
                out.push(LineSpan {
                    s0,
                    s1,
                    kind: SpanKind::Interior,
                });
            }
            PolyClass::Boundary => {
                // Find the unique collinear edge covering `mid`, and compute the inward 2D
                // direction into the face perpendicular to `dir`.
                let mut found_edge: Option<EdgeId> = None;
                let mut min_other_dist = f64::INFINITY;
                for edge in &face.edges {
                    let da = cross2(dir, sub2(edge.start_2d, line_origin));
                    let db = cross2(dir, sub2(edge.end_2d, line_origin));
                    if da.abs() <= eps
                        && db.abs() <= eps
                        && dist_point_to_segment_2d(mid, edge.start_2d, edge.end_2d) <= eps
                    {
                        if found_edge.is_some() {
                            return None;
                        }
                        found_edge = Some(edge.edge_id);
                    } else {
                        min_other_dist = min_other_dist.min(dist_point_to_segment_2d(
                            mid,
                            edge.start_2d,
                            edge.end_2d,
                        ));
                    }
                }
                let edge_id = found_edge?;
                let probe = (0.25 * min_other_dist).min(0.25 * (s1 - s0));
                if !(probe > 2.0 * eps && probe.is_finite()) {
                    return None;
                }
                let p_plus = [mid[0] + probe * perp[0], mid[1] + probe * perp[1]];
                let p_minus = [mid[0] - probe * perp[0], mid[1] - probe * perp[1]];
                let c_plus = point_in_polygon_2d(&face.loops_2d, p_plus, eps);
                let c_minus = point_in_polygon_2d(&face.loops_2d, p_minus, eps);
                let inward_2d = match (c_plus, c_minus) {
                    (PolyClass::Inside, PolyClass::Outside) => perp,
                    (PolyClass::Outside, PolyClass::Inside) => [-perp[0], -perp[1]],
                    _ => return None,
                };
                let inward_3d = face.u * inward_2d[0] + face.v * inward_2d[1];
                out.push(LineSpan {
                    s0,
                    s1,
                    kind: SpanKind::BoundaryEdge { edge_id, inward_3d },
                });
            }
        }
    }
    Some(out)
}

fn dedup_sorted(sorted: &[f64], tol: f64) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::new();
    for &x in sorted {
        if out.last().is_none_or(|&prev| (x - prev).abs() > tol) {
            out.push(x);
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::boolean::api_brep::boolean_difference;
    use crate::kernel::UpstreamFixes;
    use crate::primitive::api_primitives::{create_box, create_cylinder};
    use crate::query::contact::interiors_disjoint;

    fn kernel() -> Kernel {
        Kernel {
            fixes: UpstreamFixes::all(),
            ..Kernel::default()
        }
    }

    fn make_box(k: &mut Kernel, min: [f64; 3], size: [f64; 3]) -> Brep {
        create_box(
            k,
            Point3::new(min[0], min[1], min[2]),
            size[0],
            size[1],
            size[2],
        )
        .unwrap()
    }

    fn slotted_plate(k: &mut Kernel) -> Brep {
        let outer = make_box(k, [-20.0, -20.0, 0.0], [50.0, 50.0, 10.0]);
        let slot = make_box(k, [0.0, 0.0, -2.0], [10.0, 10.0, 14.0]);
        boolean_difference(k, outer, slot).unwrap()
    }

    fn hollow_cube(k: &mut Kernel, half_outer: f64, half_inner: f64) -> Brep {
        let outer = make_box(
            k,
            [-half_outer, -half_outer, -half_outer],
            [2.0 * half_outer, 2.0 * half_outer, 2.0 * half_outer],
        );
        let inner = make_box(
            k,
            [-half_inner, -half_inner, -half_inner],
            [2.0 * half_inner, 2.0 * half_inner, 2.0 * half_inner],
        );
        boolean_difference(k, outer, inner).unwrap()
    }

    #[test]
    fn flush_tenon_in_slot_with_3e5_vertex_shift_is_certified_disjoint() {
        let mut k = kernel();
        let mut plate = slotted_plate(&mut k);
        let mut tenon = make_box(&mut k, [0.0, 0.0, 3e-5], [10.0, 10.0, 10.0]);
        let v = interiors_disjoint(&k, &mut plate, &mut tenon).unwrap();
        assert_eq!(v, Verdict::Yes);
    }

    #[test]
    fn tenon_penetrating_slot_wall_is_rejected_even_sub_tolerance() {
        let mut k = kernel();
        for depth in [0.5, 3e-5, 5e-6] {
            let mut plate = slotted_plate(&mut k);
            let mut tenon = make_box(&mut k, [-depth, 0.0, 0.0], [10.0, 10.0, 10.0]);
            let v = interiors_disjoint(&k, &mut plate, &mut tenon).unwrap();
            assert_eq!(v, Verdict::No, "depth {depth} must be detected as overlap");
        }
    }

    #[test]
    fn crossing_bars_with_no_vertex_inside_are_rejected() {
        let mut k = kernel();
        let mut a = make_box(&mut k, [-10.0, -2.0, 0.0], [20.0, 4.0, 5.0]);
        let mut b = make_box(&mut k, [-2.0, -10.0, -1.0], [4.0, 20.0, 7.0]);
        let v = interiors_disjoint(&k, &mut a, &mut b).unwrap();
        assert_eq!(v, Verdict::No);
    }

    #[test]
    fn identical_coinciding_boxes_are_rejected() {
        let mut k = kernel();
        let mut a = make_box(&mut k, [0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let mut b = make_box(&mut k, [0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let v = interiors_disjoint(&k, &mut a, &mut b).unwrap();
        assert_eq!(v, Verdict::No);
    }

    #[test]
    fn flush_boxes_and_small_gap_boxes_are_certified_disjoint() {
        let mut k = kernel();
        for gap in [0.0, 3e-5] {
            let mut a = make_box(&mut k, [0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
            let mut b = make_box(&mut k, [10.0 + gap, 0.0, 3e-5], [10.0, 10.0, 10.0]);
            let v = interiors_disjoint(&k, &mut a, &mut b).unwrap();
            assert_eq!(v, Verdict::Yes, "gap {gap}");
        }
    }

    #[test]
    fn curved_overlap_without_separating_plane_is_undecided() {
        let mut k = kernel();
        let mut a = make_box(&mut k, [0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let mut cyl = create_cylinder(&mut k, Point3::new(0.0, 5.0, 0.0), 2.0, 10.0).unwrap();
        let v = interiors_disjoint(&k, &mut a, &mut cyl).unwrap();
        assert_eq!(v, Verdict::Undecided);
    }

    /// Concentric hollow cubes A (wall 2..4) and B (wall 1..3) have strictly separated
    /// boundaries (1 mm apart) yet overlap in material across 2..3: neither `meets` nor
    /// `interiors_disjoint` may certify them disjoint.
    #[test]
    fn concentric_hollow_cubes_with_separated_boundaries_are_not_disjoint() {
        let mut k = kernel();
        let mut a = hollow_cube(&mut k, 4.0, 2.0);
        let mut b = hollow_cube(&mut k, 3.0, 1.0);
        let contact = crate::query::contact::meets(&k, &mut a, &mut b, 1e-9).unwrap();
        assert_eq!(contact.verdict, Verdict::Yes);
        let disjoint = interiors_disjoint(&k, &mut a, &mut b).unwrap();
        assert_eq!(disjoint, Verdict::No);
    }

    /// Two boxes with a tiny positive penetration (5e-7 mm) must never be certified disjoint.
    #[test]
    fn tiny_slab_penetration_is_not_certified_disjoint() {
        let mut k = kernel();
        let mut a = make_box(&mut k, [0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let mut b = make_box(&mut k, [10.0 - 5e-7, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let v = interiors_disjoint(&k, &mut a, &mut b).unwrap();
        assert_ne!(v, Verdict::Yes);
    }

    /// Exceeding the segment-pair budget in `polygons_2d_interiors_overlap` returns `Ok(None)`
    /// rather than deciding from incomplete events.
    #[test]
    fn polygon_overlap_budget_exhaustion_returns_none() {
        let k = kernel();
        let ring = |n: usize, r: f64, ox: f64| -> Vec<Vec<[f64; 2]>> {
            let pts = (0..n)
                .map(|i| {
                    let ang = std::f64::consts::TAU * (i as f64) / (n as f64);
                    [ox + r * ang.cos(), r * ang.sin()]
                })
                .collect();
            vec![pts]
        };
        let a = ring(300, 1.0, 0.0);
        let b = ring(300, 1.0, 5.0);
        // 300 * 300 = 90_000 > MAX_SEG_PAIRS (65_536)
        let res = polygons_2d_interiors_overlap(&k, &a, &b, 1e-9).unwrap();
        assert_eq!(res, None);
    }
}
