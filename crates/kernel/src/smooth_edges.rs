//! Display-only tangent joins: which edges the shaded view leaves out because the two faces meet
//! across them with one tangent plane (G1), and which it must keep because they are a crease or
//! an authored split.
//!
//! One rule for every surface: at stations along the edge, each face's outward normal is
//! evaluated on its exact surface ([`Face::parameters`]) at the edge's exact pcurve in that face
//! (the use's own pcurve, so a seam a face meets itself across is the same test with its two
//! pcurves), turned by the side the shell uses the face on. The edge is smooth when the normals
//! agree within [`ANGLE`] at every station, and a crease when they differ by more anywhere. The
//! flags are display-only: the edge keeps its identity and stays pickable, its faces stay apart
//! (pieces of one surface are [`crate::topo::seam_flags`]'s business), and unknown geometry
//! (a station that does not evaluate) keeps its existing presentation.
use solidcraft_geom::brep::eval::SurfaceEval;
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
/// line in the shading, and drawing one would only show how the surface was cut into faces
/// (a thread's flank pieces, a sweep's spans). Exact tangency (fillets, analytic seams) reads
/// 1e-12 or so, far inside it; real creases a model means are degrees.
const ANGLE: f64 = 0.5 * std::f64::consts::PI / 180.0;
/// Below this sine of the angle between ∂S/∂u and ∂S/∂v the normal is undefined (a pole, an
/// apex): such a station says nothing.
const DEGENERATE: f64 = 1e-9;

/// What the stations along one edge say.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Join {
    Smooth,
    Crease,
    Unknown,
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

/// The outward unit normal of `face` at `uv`; `Some(None)` where the normal is undefined there.
fn normal(face: &FaceEval, uv: [f64; 2]) -> Option<Option<Vec3>> {
    let (p, su, sv) = face.surface.derivs(uv[0], uv[1])?;
    if !p.is_finite() || !su.is_finite() || !sv.is_finite() {
        return None;
    }
    let n = su.cross(sv);
    if !(n.len() > DEGENERATE * su.len() * sv.len()) {
        return Some(None);
    }
    Some(n.normalized().map(|n| n * face.side))
}

/// How the two uses of edge `e` meet: their faces' outward normals compared at [`STATIONS`]
/// stations, each through its own pcurve.
fn join(record: &Brep, faces: &[Option<FaceEval>], e: usize, work: &mut usize) -> Join {
    let Some(edge) = record.edges.get(e) else { return Join::Unknown };
    let [a, b] = edge.uses.as_slice() else { return Join::Unknown };
    let side = |u: &solidcraft_geom::brep::EdgeUseRef| {
        let face = faces.get(u.face as usize)?.as_ref()?;
        let Loop::Edges { edges } = record.faces.get(u.face as usize)?.loops.get(u.r#loop as usize)? else { return None };
        let used = edges.get(u.position as usize)?;
        (used.edge as usize == e).then_some((face, &used.pcurve))
    };
    let (Some((fa, pa)), Some((fb, pb))) = (side(a), side(b)) else { return Join::Unknown };
    let [t0, t1] = edge.interval;
    if !(t0.is_finite() && t1.is_finite() && t0 < t1) {
        return Join::Unknown;
    }
    let mut measured = false;
    for k in 0..STATIONS {
        let Some(left) = work.checked_sub(2) else { return Join::Unknown };
        *work = left;
        let t = t0 + (t1 - t0) * (k as f64 + 0.5) / STATIONS as f64;
        let (Some(ua), Some(ub)) = (pa.eval(t), pb.eval(t)) else { return Join::Unknown };
        let (Some(na), Some(nb)) = (normal(fa, ua), normal(fb, ub)) else { return Join::Unknown };
        let (Some(na), Some(nb)) = (na, nb) else { continue };
        let angle = na.cross(nb).len().atan2(na.dot(nb));
        if !angle.is_finite() {
            return Join::Unknown;
        }
        if angle > ANGLE {
            return Join::Crease;
        }
        measured = true;
    }
    if measured { Join::Smooth } else { Join::Unknown }
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
            match join(&rec, &faces, current, &mut work) {
                Join::Smooth => smooth.get_mut(current),
                Join::Crease => sharp.get_mut(current),
                Join::Unknown => None,
            }
        };
        if let Some(flag) = flag {
            *flag = true;
        }
        if work == 0 {
            break;
        }
    }
    (smooth, sharp)
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
        assert_eq!(mesh.smooth_edges.iter().filter(|f| **f).count(), 7);
        assert!(!mesh.smooth_edges[crease]);
        assert!(mesh.sharp_edges[crease]);
        assert!(mesh.shaded_edge_visible(crease));
        assert!(mesh.edge_pickable(crease));
        let (_, mesh, crease) = creased(0.2);
        assert_eq!(mesh.smooth_edges.iter().filter(|f| **f).count(), 8);
        assert!(!mesh.shaded_edge_visible(crease));
        assert!(mesh.edge_pickable(crease), "a smooth join is only left undrawn");
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

    /// The edges of a body's mesh by how the shaded view draws them: (smooth, crease, neither).
    fn census(body: &Body) -> (usize, usize, usize) {
        let mesh = body.mesh(&crate::MeshTolerance::chord(0.02)).unwrap();
        let smooth = mesh.smooth_edges.iter().filter(|f| **f).count();
        let sharp = mesh.sharp_edges.iter().filter(|f| **f).count();
        assert!(mesh.smooth_edges.iter().zip(&mesh.sharp_edges).all(|(a, b)| !(*a && *b)));
        (smooth, sharp, mesh.edges.len() - smooth - sharp)
    }

    #[test]
    fn a_spheres_and_a_cylinders_seams_are_smooth_and_their_rims_creases() {
        let sphere = crate::sphere(Vec3::ZERO, 10.).unwrap();
        let (smooth, sharp, _) = census(&sphere);
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
    /// its neighbours tangentially, so none of those joins is drawn; the box's own edges are.
    #[test]
    fn a_filleted_boxs_tangent_joins_are_smooth_and_its_corners_creases() {
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
            if bottom || square_upright || miter {
                assert!(mesh.sharp_edges[i] && mesh.shaded_edge_visible(i), "edge {i} {line:?} is a crease");
            } else {
                assert!(mesh.smooth_edges[i] && !mesh.shaded_edge_visible(i), "edge {i} {:?}..{:?} is a tangent join", line.first(), line.last());
                assert!(mesh.edge_pickable(i), "edge {i} stays pickable");
            }
        }
    }

    #[test]
    fn a_degenerate_normal_says_nothing() {
        let face = FaceEval { surface: SurfaceEval::new(&Surface::Sphere { frame: solidcraft_geom::brep::Frame { origin: [0.; 3], x_axis: [1., 0., 0.], y_axis: [0., 1., 0.] }, radius: 2. }).unwrap(), side: 1. };
        assert_eq!(normal(&face, [0.3, std::f64::consts::FRAC_PI_2]), Some(None), "the pole");
        let n = normal(&face, [0., 0.]).unwrap().unwrap();
        assert!((n - Vec3::X).len() < 1e-12, "{n:?}");
        assert_eq!(normal(&face, [f64::NAN, 0.]), None);
    }
}
