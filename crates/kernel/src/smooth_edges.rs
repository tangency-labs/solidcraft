//! Display-only smooth joins: which edges the shaded view leaves out because they lie inside one
//! smooth surface (the two faces meet with one tangent plane and one curvature across the edge,
//! G2), and which it must keep because they are a crease or an authored split. A join that is
//! only tangent (G1: a fillet's boundary, a plane tangent to a cylinder) is a feature line and is
//! drawn.
//!
//! One rule for every surface: at stations along the edge, each face's outward normal and its
//! normal curvature across the edge are evaluated on its exact surface ([`Face::parameters`]) at
//! the edge's exact pcurve in that face (the use's own pcurve, so a seam a face meets itself
//! across is the same test with its two pcurves), turned by the side the shell uses the face on.
//! The edge is smooth when the normals agree within [`ANGLE`] and the curvatures within [`BEND`]
//! at every station; it is a crease where the normals differ by more than the
//! evaluation can account for anywhere (a crease far too small to see is both). Smoothness is
//! display-only: the edge keeps its identity, stays pickable and stays in the wireframe, and its
//! faces stay apart (pieces of one surface are [`crate::topo::seam_flags`]'s business). Unknown
//! geometry (a station that does not evaluate) keeps its existing presentation.
use solidcraft_geom::brep::eval::{CurveEval, SurfaceEval};
use solidcraft_geom::brep::{Brep, Loop, Shell};
use solidcraft_geom::{Mesh, Vec3};

use crate::body::Body;

const MAX_FACES: usize = 50_000;
const MAX_EDGES: usize = 200_000;
/// Surface evaluations one body may spend (two per station).
const WORK: usize = 2_000_000;
/// Stations along each edge: the middles of this many equal parameter spans, so neither end
/// (where a pole or a vertex of a degenerate face sits) is one.
const STATIONS: usize = 8;
/// Largest angle between the faces' normals, in radians, that still counts as tangent: half a
/// degree. Shading is the visible trace of a join: across a crease of angle θ the lit intensity
/// jumps by at most about θ of its range, and half a degree (0.0087) is under 1%, below the
/// 1–2% step the eye tells apart on a smooth gradient. So a join this close to tangent shows no
/// line in the shading. Exact tangency (fillets, analytic seams) reads 1e-12 or so, far inside
/// it; real creases a model means are degrees. Tangency alone does not hide an edge ([`BEND`]).
/// It is SolidCraft's one angle for smooth: a blend finds nothing to round below it too.
pub(crate) const ANGLE: f64 = 0.5 * std::f64::consts::PI / 180.0;
/// Largest difference of the two faces' normal curvatures across an edge, relative to the
/// larger one (or to 1 / the edge's length where both are flatter), that still counts as one
/// smooth surface (G2): [`ANGLE`], the same visibility criterion as for the normals. Continued
/// past the edge, two faces whose curvatures differ by Δκ turn their normals apart by Δκ per
/// mm; the bound keeps that under [`ANGLE`] over one radius of curvature (Δκ / κ) and over the
/// edge's own length (Δκ L), so the join leaves no visible kink in reflections either. A join
/// that is only tangent (G1) is a feature line and stays drawn: a fillet meets its plane with a
/// jump from 0 to 1 / radius (a relative difference of 1), a cylinder meets a tangent plane the
/// same way. Pieces of one surface pass: exact pieces agree to rounding, and a modelled thread's
/// flank pieces (spans of one sweep) differ by at most 0.0046 (measured, the thread test).
const BEND: f64 = ANGLE;
/// Below this sine of the angle between ∂S/∂u and ∂S/∂v the normal is undefined (a pole, an
/// apex): such a station says nothing.
const DEGENERATE: f64 = 1e-9;
/// Angle, in radians, that rounding alone can put between two unit normals evaluated in double
/// precision on exactly one tangent plane.
const ROUNDOFF: f64 = 1e-12;

/// What the stations along one edge say. The two verdicts are separate: a join can be too
/// close to tangent to show in the shading (`smooth`) and still be a measured crease
/// (`crease`) — a real edge between two surfaces, kept pickable and apart even where a coarse
/// fit takes its faces for one surface ([`crate::topo::seam_flags`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Join {
    /// Every station's normals agree within [`ANGLE`] and its curvatures across within [`BEND`].
    smooth: bool,
    /// Some station's normals differ by more than the evaluation can account for: the pcurve
    /// points lie some distance off the edge's curve (measured at the station), and over that
    /// distance a normal turns by at most the distance times the surface's largest curvature.
    crease: bool,
}

pub(crate) fn flags(body: &Body, mesh: &Mesh) -> (Vec<bool>, Vec<bool>) {
    crate::guard("smooth display edges", || Ok(classify(body, mesh, WORK))).unwrap_or_default()
}

/// A face ready to evaluate: its surface and the sign that turns the natural normal outward.
struct FaceEval {
    surface: SurfaceEval,
    side: f64,
}

/// Each face's evaluator, or `None` where its surface does not evaluate or no shell uses it. A
/// face's side is the one its first shell uses it on.
fn face_evals(record: &Brep) -> Vec<Option<FaceEval>> {
    let mut forward: Vec<Option<bool>> = vec![None; record.faces.len()];
    for shell in &record.shells {
        if let Shell::Faces { faces, .. } = shell {
            for f in faces {
                if let Some(slot) = forward.get_mut(f.face as usize) {
                    slot.get_or_insert(f.forward);
                }
            }
        }
    }
    record
        .faces
        .iter()
        .zip(forward)
        .map(|(face, forward)| {
            let surface = SurfaceEval::new(face.parameters()).ok()?;
            Some(FaceEval { surface, side: if forward? { 1.0 } else { -1.0 } })
        })
        .collect()
}

/// A face at a station: its point, outward unit normal, first and second derivatives, and a
/// bound on its curvature.
struct Station {
    point: Vec3,
    normal: Vec3,
    su: Vec3,
    sv: Vec3,
    /// `(S_uu, S_uv, S_vv)`; `None` where the surface has none (a revolution).
    second: Option<(Vec3, Vec3, Vec3)>,
    /// The larger principal curvature's size, 1/mm (`None`: no second derivatives, so a crease
    /// cannot be told from evaluation error).
    curvature: Option<f64>,
}

impl Station {
    /// The normal curvature in the tangent direction `w`, 1/mm, signed by the outward normal:
    /// II(w) / I(w), with `w` written in (u, v) by its projection on the tangent plane.
    fn across(&self, w: Vec3) -> Option<f64> {
        let (suu, suv, svv) = self.second?;
        let (e, f, g) = (self.su.dot(self.su), self.su.dot(self.sv), self.sv.dot(self.sv));
        let det = e * g - f * f;
        let (wu, wv) = (self.su.dot(w), self.sv.dot(w));
        let (a, b) = ((g * wu - f * wv) / det, (e * wv - f * wu) / det);
        let first = e * a * a + 2. * f * a * b + g * b * b;
        let (l, m, n) = (suu.dot(self.normal), suv.dot(self.normal), svv.dot(self.normal));
        let k = (l * a * a + 2. * m * a * b + n * b * b) / first;
        (det > 0. && first > 0. && k.is_finite()).then_some(k)
    }
}

/// `face` at `uv`; `Some(None)` where the normal is undefined there, `None` where it does not
/// evaluate.
fn station(face: &FaceEval, uv: [f64; 2]) -> Option<Option<Station>> {
    let (point, su, sv) = face.surface.derivs(uv[0], uv[1])?;
    if !point.is_finite() || !su.is_finite() || !sv.is_finite() {
        return None;
    }
    let n = su.cross(sv);
    if !(n.len() > DEGENERATE * su.len() * sv.len()) {
        return Some(None);
    }
    let Some(unit) = n.normalized() else { return Some(None) };
    let second = face.surface.second(uv[0], uv[1]).filter(|(a, b, c)| a.is_finite() && b.is_finite() && c.is_finite());
    // The principal curvatures, from the shape operator I⁻¹ II: H ± √(H² − K).
    let curvature = second.and_then(|(suu, suv, svv)| {
        let (e, f, g) = (su.dot(su), su.dot(sv), sv.dot(sv));
        let (l, m, n) = (suu.dot(unit), suv.dot(unit), svv.dot(unit));
        let det = e * g - f * f;
        let mean = (e * n - 2. * f * m + g * l) / (2. * det);
        let gauss = (l * n - m * m) / det;
        let most = mean.abs() + (mean * mean - gauss).max(0.).sqrt();
        (det > 0. && most.is_finite()).then_some(most)
    });
    Some(Some(Station { point, normal: unit * face.side, su, sv, second, curvature }))
}

/// How the two uses of edge `e` meet: their faces' outward normals compared at [`STATIONS`]
/// stations, each through its own pcurve.
fn join(record: &Brep, faces: &[Option<FaceEval>], e: usize, length: f64, work: &mut usize) -> Join {
    let unknown = Join::default();
    let Some(edge) = record.edges.get(e) else { return unknown };
    let [a, b] = edge.uses.as_slice() else { return unknown };
    let side = |u: &solidcraft_geom::brep::EdgeUseRef| {
        let face = faces.get(u.face as usize)?.as_ref()?;
        let Loop::Edges { edges } = record.faces.get(u.face as usize)?.loops.get(u.r#loop as usize)? else { return None };
        let used = edges.get(u.position as usize)?;
        (used.edge as usize == e).then_some((face, &used.pcurve))
    };
    let (Some((fa, pa)), Some((fb, pb))) = (side(a), side(b)) else { return unknown };
    let [t0, t1] = edge.interval;
    if !(t0.is_finite() && t1.is_finite() && t0 < t1 && edge.tolerance.is_finite() && edge.tolerance >= 0.) {
        return unknown;
    }
    let curve = CurveEval::new(edge.parameters()).ok();
    let (mut measured, mut smooth, mut crease) = (false, true, false);
    for k in 0..STATIONS {
        let Some(left) = work.checked_sub(2) else { return unknown };
        *work = left;
        let t = t0 + (t1 - t0) * (k as f64 + 0.5) / STATIONS as f64;
        let (Some(ua), Some(ub)) = (pa.eval(t), pb.eval(t)) else { return unknown };
        let (Some(sa), Some(sb)) = (station(fa, ua), station(fb, ub)) else { return unknown };
        let (Some(sa), Some(sb)) = (sa, sb) else { continue };
        let angle = sa.normal.cross(sb.normal).len().atan2(sa.normal.dot(sb.normal));
        if !angle.is_finite() {
            return unknown;
        }
        measured = true;
        smooth &= angle <= ANGLE && same_bend(&sa, &sb, curve.as_ref().and_then(|c| c.derivs(t)).map(|d| d.1), length);
        // How far each evaluated point lies off the edge, measured on the edge's own curve
        // (its tolerance where the curve does not evaluate).
        let off = match curve.as_ref().and_then(|c| c.point(t)).filter(|c| c.is_finite()) {
            Some(c) => sa.point.dist(c).max(sb.point.dist(c)).max(sa.point.dist(sb.point)),
            None => edge.tolerance,
        };
        if let (Some(ka), Some(kb)) = (sa.curvature, sb.curvature) {
            crease |= angle > off * (ka + kb) + ROUNDOFF;
        }
        crease |= angle > ANGLE;
    }
    if measured { Join { smooth, crease } } else { unknown }
}

/// Do the two faces bend alike across the edge at a station (G2 there): their normal
/// curvatures in the direction across the edge (in the tangent plane, square to the edge's
/// tangent `along`) within [`BEND`] of the larger, or of 1 / `length` where both are flatter
/// than that? Unknown (no tangent, no second derivatives) is no.
fn same_bend(a: &Station, b: &Station, along: Option<Vec3>, length: f64) -> bool {
    let Some(w) = along.and_then(|t| a.normal.cross(t).normalized()) else { return false };
    let (Some(ka), Some(kb)) = (a.across(w), b.across(w)) else { return false };
    let scale = ka.abs().max(kb.abs()).max(1. / length);
    (ka - kb).abs() <= BEND * scale
}

/// Does the mesh's edge `e` bound the faces the record's edge `e` is used by? A mesh that says
/// otherwise is not this record's, and its edge keeps its presentation.
fn same_faces(record: &Brep, mesh: &Mesh, e: usize) -> bool {
    let (Some(edge), Some(listed)) = (record.edges.get(e), mesh.edge_faces.get(e)) else { return false };
    let mut used: Vec<u32> = edge.uses.iter().map(|u| u.face).collect();
    let mut listed = listed.clone();
    used.sort_unstable();
    used.dedup();
    listed.sort_unstable();
    listed.dedup();
    used == listed
}

fn classify(body: &Body, mesh: &Mesh, mut work: usize) -> (Vec<bool>, Vec<bool>) {
    let Ok(rec) = crate::native::brep_record(body) else { return (Vec::new(), Vec::new()) };
    if rec.faces.len() > MAX_FACES || mesh.edges.len() > MAX_EDGES {
        return (Vec::new(), Vec::new());
    }
    let mut split_points = 0usize;
    for line in body.split_keep() {
        let Some(left) = work.checked_sub(line.len().saturating_add(1)) else { return (Vec::new(), Vec::new()) };
        work = left;
        let Some(total) = split_points.checked_add(line.len()) else { return (Vec::new(), Vec::new()) };
        split_points = total;
    }
    let faces = face_evals(&rec);
    let mut smooth = vec![false; mesh.edges.len()];
    let mut sharp = vec![false; mesh.edges.len()];
    // Edges of one join: the measured edges between the same faces that continue each other.
    let mut joins: Vec<usize> = Vec::new();
    // The mesh's edges are the record's, in its order.
    for (current, line) in mesh.edges.iter().enumerate().take(rec.edges.len()) {
        let Some(split_work) = line.len().checked_add(1).and_then(|n| n.checked_mul(split_points)) else { break };
        let Some(left) = work.checked_sub(split_work) else { break };
        work = left;
        let flag = if crate::heal::on_split_line(line, body.split_keep()) {
            sharp.get_mut(current)
        } else if !same_faces(&rec, mesh, current) {
            None
        } else {
            joins.push(current);
            let length: f64 = line.windows(2).map(|w| if let [a, b] = w { a.dist(*b) } else { 0. }).sum();
            let Join { smooth: tangent, crease } =
                if length > 0. && length.is_finite() { join(&rec, &faces, current, length, &mut work) } else { Join::default() };
            if let (true, Some(flag)) = (tangent, smooth.get_mut(current)) {
                *flag = true;
            }
            crease.then(|| sharp.get_mut(current)).flatten()
        };
        if let Some(flag) = flag {
            *flag = true;
        }
        if work == 0 {
            break;
        }
    }
    whole_joins(&rec, &joins, &mut smooth, work);
    (smooth, sharp)
}

/// One verdict for each join, however the B-rep splits it: the edges in `measured` between the
/// same faces that continue each other at a shared vertex (their curves' tangents there within
/// [`ANGLE`] of one line) are one join, smooth only where every one of its edges is. A seam
/// SMLib holds as two edges, G2 along one and only near G2 along the other (a G2 corner patch's
/// side next to the face it ends on), is drawn whole, never to a vertex in the middle of the
/// line. `work` bounds the pairs compared; past it the edges keep their own verdicts.
fn whole_joins(record: &Brep, measured: &[usize], smooth: &mut [bool], mut work: usize) {
    // Per vertex, each measured edge ending there: its faces and its tangent leaving the vertex.
    let mut ends: std::collections::HashMap<u32, Vec<(usize, Vec<u32>, Vec3)>> = std::collections::HashMap::new();
    for &e in measured {
        let Some(edge) = record.edges.get(e) else { continue };
        let Some(curve) = CurveEval::new(edge.parameters()).ok() else { continue };
        let mut faces: Vec<u32> = edge.uses.iter().map(|u| u.face).collect();
        faces.sort_unstable();
        faces.dedup();
        let [t0, t1] = edge.interval;
        for (vertex, t, sign) in [(edge.vertices[0], t0, 1.), (edge.vertices[1], t1, -1.)] {
            if let Some(d) = curve.derivs(t).and_then(|d| (d.1 * sign).normalized()) {
                ends.entry(vertex).or_default().push((e, faces.clone(), d));
            }
        }
    }
    let mut parent: Vec<usize> = (0..smooth.len()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while let Some(&p) = parent.get(i) {
            if p == i {
                break;
            }
            let up = parent.get(p).copied().unwrap_or(p);
            if let Some(slot) = parent.get_mut(i) {
                *slot = up;
            }
            i = up;
        }
        i
    }
    let cos = ANGLE.cos();
    'vertices: for at in ends.values() {
        for (i, (a, fa, da)) in at.iter().enumerate() {
            for (b, fb, db) in at.iter().skip(i + 1) {
                let Some(left) = work.checked_sub(1) else { break 'vertices };
                work = left;
                // Leaving the vertex in opposite directions: one continues the other.
                if a != b && fa == fb && da.dot(*db) <= -cos {
                    let (ra, rb) = (root(&mut parent, *a), root(&mut parent, *b));
                    if let Some(slot) = parent.get_mut(ra) {
                        *slot = rb;
                    }
                }
            }
        }
    }
    let mut whole: std::collections::HashMap<usize, bool> = std::collections::HashMap::new();
    for &e in measured {
        let r = root(&mut parent, e);
        let own = smooth.get(e).copied().unwrap_or(false);
        whole.entry(r).and_modify(|s| *s &= own).or_insert(own);
    }
    for &e in measured {
        let r = root(&mut parent, e);
        if let (Some(slot), Some(&all)) = (smooth.get_mut(e), whole.get(&r)) {
            *slot = all;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use solidcraft_geom::brep::Surface;
    use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2};

    fn circle(radius: f64, count: usize) -> Loop2 {
        let step = std::f64::consts::TAU / count as f64;
        let k = 4. / 3. * (step / 4.).tan() * radius;
        let point = |angle: f64| Vec2::new(angle.cos(), angle.sin()) * radius;
        let tangent = |angle: f64| Vec2::new(-angle.sin(), angle.cos()) * k;
        Loop2 {
            segs: (0..count)
                .map(|i| {
                    let a = i as f64 * step;
                    let b = (i + 1) as f64 * step;
                    let p0 = point(a);
                    let p3 = if i + 1 == count { point(0.) } else { point(b) };
                    Seg2::Cubic { p0, p1: p0 + tangent(a), p2: p3 - tangent(b), p3 }
                })
                .collect(),
        }
    }

    fn ring(count: usize) -> Body {
        crate::extrude(&Plane::XY, &[Region2 { outer: circle(10., count), holes: vec![circle(9., count).reversed()] }], 0., 3.).unwrap().remove(0)
    }

    /// A square sheet with a round hole a closed curve cut (its loop one closed edge) is
    /// classified without a check refusing the closed edge (a panic the guard would catch).
    #[test]
    fn a_hole_of_one_closed_edge_is_classified() {
        let sheet = crate::patch_region(
            &Plane::XY,
            &Region2 {
                outer: Loop2::polygon(&[Vec2::new(-10., -10.), Vec2::new(10., -10.), Vec2::new(10., 10.), Vec2::new(-10., 10.)]),
                holes: vec![],
            },
        )
        .unwrap();
        let ring: Vec<Seg2> = Loop2::circle(Vec2::ZERO, 4.).segs;
        let tool = crate::SplitTool::Curves { plane: Plane::XY, curves: vec![ring], along: None };
        let holed = crate::trim(&sheet, &tool, Vec3::new(9., 9., 0.)).unwrap();
        assert!(crate::native::brep_record(&holed).unwrap().edges.iter().any(|e| e.vertices[0] == e.vertices[1]), "a closed edge");
        let mesh = holed.mesh(&crate::MeshTolerance::chord(0.02)).unwrap();
        let (smooth, sharp) = classify(&holed, &mesh, WORK);
        assert_eq!((smooth.len(), sharp.len()), (mesh.edges.len(), mesh.edges.len()));
    }

    #[test]
    fn periodic_ring_hides_only_whole_smooth_wall_joins() {
        let body = ring(256);
        let mesh = body.mesh(&crate::MeshTolerance::chord(0.02)).unwrap();
        assert_eq!(mesh.edges.len(), 1536);
        assert_eq!(mesh.smooth_edges.iter().filter(|f| **f).count(), 512);
        for (index, (line, smooth)) in mesh.edges.iter().zip(&mesh.smooth_edges).enumerate() {
            let vertical = (line.first().unwrap().z - line.last().unwrap().z).abs() > 2.9;
            assert_eq!(*smooth, vertical);
            assert!(!(vertical && mesh.sharp_edges[index]), "a tangent join is measured as no crease");
            if !vertical {
                assert!(mesh.shaded_edge_visible(index));
            }
        }
        // Smoothing flags are display-only: the edges and faces keep the body's identities.
        let raw = body.mesh(&crate::MeshTolerance::chord(0.02)).unwrap();
        assert_eq!(mesh.edges.len(), raw.edges.len());
        assert_eq!(mesh.edge_faces, raw.edge_faces);
        for (a, b) in mesh.edges.iter().zip(&raw.edges) {
            assert_eq!((a.first(), a.last()), (b.first(), b.last()));
        }
        let faces = |m: &Mesh| m.tri_face.iter().copied().collect::<std::collections::BTreeSet<u32>>();
        assert_eq!(faces(&mesh), faces(&raw));
        let moved = crate::transform_matrix(&body, [[1., 0., 0., 0.], [0., 0., 1., 0.], [0., -1., 0., 0.], [30., 20., 10., 1.]]).unwrap();
        assert_eq!(moved.mesh(&crate::MeshTolerance::chord(0.02)).unwrap().smooth_edges.iter().filter(|f| **f).count(), 512);
    }

    #[test]
    fn authored_split_unknown_adjacency_and_exhausted_work_stay_visible() {
        let body = ring(8);
        let mesh = body.mesh(&crate::MeshTolerance::chord(0.02)).unwrap();
        let index = mesh.smooth_edges.iter().position(|f| *f).unwrap();
        let split = body.clone().with_split_lines(Some(std::sync::Arc::new(vec![mesh.edges[index].clone()])));
        assert!(!flags(&split, &mesh).0[index]);
        let mut unknown = mesh.clone();
        unknown.edge_faces[index].push(999);
        assert!(!flags(&body, &unknown).0[index]);
        assert!(classify(&body, &mesh, 0).0.iter().all(|f| !*f));
    }

    /// A G2 corner patch's sides are G2 to their rounds except near the faces they end on, and
    /// SMLib holds each side as two edges: the side is one join, drawn whole, not up to the
    /// vertex in its middle where the G2 half begins (the 20 mm box's r3 corner).
    #[test]
    fn a_seam_split_in_two_edges_is_drawn_whole() {
        let body = crate::box_solid(Vec3::ZERO, Vec3::new(20., 20., 20.)).unwrap();
        let picks = [Vec3::new(20., 20., 10.), Vec3::new(20., 10., 20.), Vec3::new(10., 20., 20.)];
        let g2 = crate::fillet_g2_corners(&body, &picks, 3., &[]).unwrap();
        let mesh = g2.mesh(&crate::MeshTolerance::chord(0.02)).unwrap();
        let rec = crate::native::brep_record(&g2).unwrap();
        let patch = rec
            .faces
            .iter()
            .position(|f| !matches!(f.surface, Surface::Plane { .. } | Surface::PlaneThrough { .. } | Surface::Cylinder { .. }))
            .unwrap() as u32;
        let rounds: Vec<u32> = (0..rec.faces.len() as u32).filter(|f| matches!(rec.faces[*f as usize].surface, Surface::Cylinder { .. })).collect();
        assert_eq!(rounds.len(), 3);
        for round in rounds {
            let side: Vec<usize> =
                (0..mesh.edges.len()).filter(|e| mesh.edge_faces[*e].contains(&patch) && mesh.edge_faces[*e].contains(&round)).collect();
            assert_eq!(side.len(), 2, "SMLib splits the side in two: {side:?}");
            assert!(side.iter().all(|e| mesh.shaded_edge_visible(*e)), "the side to round {round} is drawn whole: {side:?}");
        }
    }

    /// An extruded circle of eight cubics whose first cubic leaves its start turned by `degrees`:
    /// the wall join there is a crease of that angle.
    fn creased(degrees: f64) -> (Body, Mesh, usize) {
        let mut outer = circle(10., 8);
        if let Seg2::Cubic { p0, p1, .. } = &mut outer.segs[0] {
            p1.x += (p1.y - p0.y) * degrees.to_radians().tan();
        }
        let body = crate::extrude(&Plane::XY, &[Region2 { outer, holes: vec![] }], 0., 3.).unwrap().remove(0);
        let mesh = body.mesh(&crate::MeshTolerance::chord(0.02)).unwrap();
        let crease = mesh
            .edges
            .iter()
            .position(|line| {
                line.first().is_some_and(|p| (p.x - 10.).abs() < 1e-8 && p.y.abs() < 1e-8)
                    && line.last().is_some_and(|p| (p.x - 10.).abs() < 1e-8 && p.y.abs() < 1e-8)
                    && line.first().zip(line.last()).is_some_and(|(a, b)| (a.z - b.z).abs() > 2.9)
            })
            .unwrap();
        (body, mesh, crease)
    }

    #[test]
    fn a_crease_over_half_a_degree_is_visible_and_one_under_it_is_not() {
        let (_, mesh, crease) = creased(1.0);
        // The crease's other end bends differently too: only six joins are one smooth surface.
        assert_eq!(mesh.smooth_edges.iter().filter(|f| **f).count(), 6);
        assert!(!mesh.smooth_edges[crease]);
        assert!(mesh.sharp_edges[crease]);
        assert!(mesh.shaded_edge_visible(crease));
        assert!(mesh.edge_pickable(crease));
        // Too small to see, but measured: not drawn in the shading, still a real edge.
        for degrees in [0.2, 1e-4] {
            let (_, mesh, crease) = creased(degrees);
            assert_eq!(mesh.smooth_edges.iter().filter(|f| **f).count(), 8, "{degrees}");
            assert!(mesh.sharp_edges[crease], "{degrees}");
            assert!(!mesh.shaded_edge_visible(crease), "{degrees}");
            assert!(mesh.edge_visible(crease) && mesh.edge_pickable(crease), "{degrees}: a smooth join is only left undrawn");
        }
    }

    #[test]
    fn actual_authored_split_of_a_curved_ruled_wall_stays_visible() {
        let body = ring(4);
        let face = crate::native::brep_record(&body).unwrap().faces.iter().position(|face| matches!(face.surface, Surface::Spline { .. })).unwrap();
        let tool = crate::SplitTool::Plane(Plane::new(Vec3::new(7., 0., 0.), Vec3::Y, Vec3::Z).unwrap());
        let split = crate::split_faces(&body, &[face], &tool).unwrap();
        assert!(split.face_count() > body.face_count());
        assert!(!split.split_keep().is_empty());
        let mesh = split.mesh(&crate::MeshTolerance::chord(0.02)).unwrap();
        let authored: Vec<_> =
            mesh.edges.iter().enumerate().filter(|(_, line)| crate::heal::on_split_line(line, split.split_keep())).map(|(i, _)| i).collect();
        assert!(!authored.is_empty());
        for index in authored {
            assert!(!mesh.smooth_edges[index]);
            assert!(mesh.shaded_edge_visible(index));
        }
    }

    #[test]
    fn a_spheres_and_a_cylinders_seams_are_smooth_and_their_rims_creases() {
        let sphere = crate::sphere(Vec3::ZERO, 10.).unwrap();
        let mesh = sphere.mesh(&crate::MeshTolerance::chord(0.02)).unwrap();
        let (smooth, sharp) = (mesh.smooth_edges.iter().filter(|f| **f).count(), mesh.sharp_edges.iter().filter(|f| **f).count());
        assert!(smooth >= 1 && sharp == 0, "sphere: {smooth} smooth, {sharp} creases");
        let cylinder = crate::cylinder(Vec3::ZERO, Vec3::Z, 8., 20.).unwrap();
        let mesh = cylinder.mesh(&crate::MeshTolerance::chord(0.02)).unwrap();
        for (i, line) in mesh.edges.iter().enumerate() {
            let vertical = line.first().zip(line.last()).is_some_and(|(a, b)| (a.z - b.z).abs() > 19.);
            assert_eq!(mesh.smooth_edges[i], vertical, "edge {i}");
            assert_eq!(mesh.sharp_edges[i], !vertical, "edge {i}: a rim is a crease");
        }
    }

    /// A box with its top edges and one upright edge rounded: every round meets its faces and
    /// its neighbours tangentially (G1, not G2), so each of those joins is a drawn feature line
    /// but no crease; the box's own edges and the miters are creases.
    #[test]
    fn a_filleted_boxs_tangent_joins_are_drawn_and_its_corners_creases() {
        let block = crate::box_solid(Vec3::ZERO, Vec3::new(20., 20., 10.)).unwrap();
        let picks = [Vec3::new(10., 0., 10.), Vec3::new(20., 10., 10.), Vec3::new(10., 20., 10.), Vec3::new(0., 10., 10.), Vec3::new(20., 20., 5.)];
        let rounded = crate::fillet(&block, &picks, 3.).unwrap();
        let mesh = rounded.mesh(&crate::MeshTolerance::chord(0.02)).unwrap();
        for (i, line) in mesh.edges.iter().enumerate() {
            let (Some(a), Some(b)) = (line.first(), line.last()) else { panic!("edge {i} has no points") };
            let corner = |p: &Vec3| [p.x, p.y].iter().all(|c| c.abs() < 1e-6 || (c - 20.).abs() < 1e-6);
            // The bottom rim, the three square upright edges, and the miters where two top
            // rounds meet over a square corner are creases.
            let bottom = line.iter().all(|p| p.z.abs() < 1e-6);
            let square_upright = corner(a) && corner(b) && (a.z - b.z).abs() > 1.;
            let miter = (corner(a) && (b.z - 10.).abs() < 1e-6) || (corner(b) && (a.z - 10.).abs() < 1e-6);
            assert!(!mesh.smooth_edges[i] && mesh.shaded_edge_visible(i) && mesh.edge_pickable(i), "edge {i} {a:?}..{b:?} is drawn");
            assert_eq!(mesh.sharp_edges[i], bottom || square_upright || miter, "edge {i} {a:?}..{b:?}: only the box's edges are creases");
        }
    }

    /// A plane tangent to a cylinder (a D-shaped prism): one tangent plane, but the curvature
    /// jumps from 0 to 1 / radius, so the join is drawn.
    #[test]
    fn a_plane_tangent_to_a_cylinder_is_drawn() {
        let (a, b, c, d) = (Vec2::new(-5., -5.), Vec2::new(5., -5.), Vec2::new(5., 5.), Vec2::new(-5., 5.));
        let outer = Loop2 {
            segs: vec![
                Seg2::Line { a, b },
                Seg2::Arc { center: Vec2::new(5., 0.), radius: 5., start: -std::f64::consts::FRAC_PI_2, sweep: std::f64::consts::PI },
                Seg2::Line { a: c, b: d },
                Seg2::Line { a: d, b: a },
            ],
        };
        let body = crate::extrude(&Plane::XY, &[Region2 { outer, holes: vec![] }], 0., 3.).unwrap().remove(0);
        let mesh = body.mesh(&crate::MeshTolerance::chord(0.02)).unwrap();
        let joins: Vec<usize> = (0..mesh.edges.len())
            .filter(|&i| {
                let line = &mesh.edges[i];
                line.first()
                    .zip(line.last())
                    .is_some_and(|(p, q)| (p.x - 5.).abs() < 1e-6 && (p.y.abs() - 5.).abs() < 1e-6 && (p.z - q.z).abs() > 2.9)
            })
            .collect();
        assert_eq!(joins.len(), 2, "{:?}", mesh.edges.iter().map(|l| (l.first(), l.last())).collect::<Vec<_>>());
        for i in joins {
            assert!(!mesh.smooth_edges[i] && !mesh.sharp_edges[i], "edge {i}: tangent, not curvature continuous, no crease");
            assert!(mesh.shaded_edge_visible(i) && mesh.edge_pickable(i));
        }
    }

    #[test]
    fn a_degenerate_normal_says_nothing() {
        let face = FaceEval {
            surface: SurfaceEval::new(&Surface::Sphere {
                frame: solidcraft_geom::brep::Frame { origin: [0.; 3], x_axis: [1., 0., 0.], y_axis: [0., 1., 0.] },
                radius: 2.,
            })
            .unwrap(),
            side: 1.,
        };
        assert!(matches!(station(&face, [0.3, std::f64::consts::FRAC_PI_2]), Some(None)), "the pole");
        let at = station(&face, [0., 0.]).unwrap().unwrap();
        assert!((at.normal - Vec3::X).len() < 1e-12, "{:?}", at.normal);
        // A sphere's normal curvature is 1 / radius in every direction.
        assert!(at.curvature.is_some_and(|k| (k - 0.5).abs() < 1e-9), "{:?}", at.curvature);
        assert!(station(&face, [f64::NAN, 0.]).is_none());
    }
}
