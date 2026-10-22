//! Display-only tangent joins. Unknown geometry retains its existing presentation.
use solidcraft_geom::brep::{Curve, Loop, Shell, Surface};
use solidcraft_geom::{Mesh, Vec3};

use crate::body::Body;

const MAX_FACES: usize = 50_000;
const MAX_EDGES: usize = 200_000;
const WORK: usize = 2_000_000;
const ANGLE: f64 = 1e-8;

#[derive(Clone, Copy)]
struct Boundary {
    a: Vec3,
    b: Vec3,
    normal: Vec3,
    error: f64,
}

fn charge(work: &mut usize, amount: usize) -> Option<()> {
    *work = work.checked_sub(amount)?;
    Some(())
}

fn roundoff(points: &[Vec3]) -> f64 {
    points.iter().flat_map(|p| [p.x.abs(), p.y.abs(), p.z.abs()]).fold(1., f64::max) * f64::EPSILON * 64.
}

/// Only a clamped Bezier profile swept by one common linear translation (a B-spline surface
/// record: `points[i][j]`, i along u). Its whole ruling normal is bounded from the linear cross
/// derivative, not sampled. `orientation`: the face's outward normal is the natural one.
fn ruled(u_knots: &[f64], v_knots: &[f64], controls: &[Vec<[f64; 3]>], orientation: bool, work: &mut usize) -> Option<[Boundary; 2]> {
    let rows = controls.len();
    let cols = controls.first()?.len();
    if !(2..=33).contains(&rows) || !(2..=33).contains(&cols) || controls.iter().any(|row| row.len() != cols) {
        return None;
    }
    charge(work, rows.checked_mul(cols)?.checked_mul(4)?)?;
    let clamped = |knots: &[f64], count: usize| {
        let (Some(first), Some(last)) = (knots.first(), knots.last()) else { return false };
        knots.len() == count * 2
            && first.is_finite()
            && last.is_finite()
            && first < last
            && knots.iter().take(count).all(|k| k == first)
            && knots.iter().skip(count).all(|k| k == last)
    };
    if !clamped(u_knots, rows) || !clamped(v_knots, cols) {
        return None;
    }
    for transpose in [false, true] {
        let (count, axial) = if transpose { (cols, rows) } else { (rows, cols) };
        if axial != 2 {
            continue;
        }
        let at = |i: usize, j: usize| {
            let p = if transpose { controls.get(j)?.get(i)? } else { controls.get(i)?.get(j)? };
            let p = Vec3::new(p[0], p[1], p[2]);
            p.is_finite().then_some(p)
        };
        let a = at(0, 0)?;
        let b = at(0, 1)?;
        let ruling = b - a;
        if ruling.len() < 1e-6 {
            continue;
        }
        let mut valid = true;
        for i in 0..count {
            let p = at(i, 0)?;
            let q = at(i, 1)?;
            if (q - p - ruling).len() > roundoff(&[a, b, p, q]) {
                valid = false;
                break;
            }
        }
        if !valid {
            continue;
        }
        let boundary = |index: usize, next: usize, end: bool| {
            let a = at(index, 0)?;
            let b = at(index, 1)?;
            let tangent = if end { a - at(next, 0)? } else { at(next, 0)? - a };
            let error = roundoff(&[a, b, at(next, 0)?]);
            // Ill-conditioned derivatives cannot certify a tiny angular join.
            if tangent.len() <= error / ANGLE || ruling.len() <= error / ANGLE {
                return None;
            }
            let cross = if transpose { ruling.cross(tangent) } else { tangent.cross(ruling) };
            let normal = cross.normalized()? * if orientation { 1. } else { -1. };
            let top_tangent = if end { b - at(next, 1)? } else { at(next, 1)? - b };
            let top_cross = if transpose { ruling.cross(top_tangent) } else { top_tangent.cross(ruling) };
            let top_normal = top_cross.normalized()? * if orientation { 1. } else { -1. };
            // Along this linear ruling the cross derivative is a linear
            // interpolation of these endpoint vectors. Positive alignment
            // excludes a singularity; all intermediate normal directions lie
            // between their directions. Include that whole-span variation.
            if normal.dot(top_normal) <= 0. {
                return None;
            }
            let conditioning = cross.len().min(top_cross.len());
            let normal_error = 8. * error * (tangent.len().max(top_tangent.len()) + ruling.len()) / conditioning + normal.dist(top_normal);
            if !normal_error.is_finite() || normal_error > ANGLE {
                return None;
            }
            Some(Boundary { a, b, normal, error: normal_error })
        };
        if let (Some(first), Some(last)) = (boundary(0, 1, false), boundary(count - 1, count - 2, true)) {
            return Some([first, last]);
        }
    }
    None
}

/// How a face's outward normal along one of its straight edges is known: the two ruling
/// boundaries of a ruled patch, or the closed form of a plane, cylinder or cone.
enum Normals {
    Ruled([Boundary; 2]),
    Analytic { surface: Surface, orientation: bool },
}

/// The face's outward normal along the straight edge from `a` to `b`: a ruled patch's along its
/// boundary ruling there, an analytic face's from its closed form at both ends (their difference
/// is the uncertainty, so an edge that is no ruling, where the normal turns, certifies nothing).
fn normal_along(normals: &Normals, a: Vec3, b: Vec3) -> Option<Boundary> {
    match normals {
        Normals::Ruled(boundaries) => on_boundary(boundaries, a, b),
        Normals::Analytic { surface, orientation } => {
            let sign = if *orientation { 1. } else { -1. };
            let at = |p: Vec3| analytic_normal(surface, p).map(|n| n * sign);
            let (na, nb) = (at(a)?, at(b)?);
            let error = na.dist(nb) + roundoff(&[a, b]);
            (na.dot(nb) > 0. && error.is_finite()).then_some(Boundary { a, b, normal: na, error })
        }
    }
}

/// The natural (unit) normal of a plane, cylinder or cone at a point on it: the plane's u × v;
/// away from the axis for a cylinder; for a cone, away from the axis and back along it by its
/// half angle (the record's (u, v): radius `radius_at_origin + v tan a` at height v).
fn analytic_normal(surface: &Surface, p: Vec3) -> Option<Vec3> {
    let v3 = |x: [f64; 3]| Vec3::new(x[0], x[1], x[2]);
    let radial = |frame: &solidcraft_geom::brep::Frame| -> Option<(Vec3, Vec3)> {
        let z = v3(frame.x_axis).cross(v3(frame.y_axis)).normalized()?;
        let d = p - v3(frame.origin);
        Some(((d - z * d.dot(z)).normalized()?, z))
    };
    match surface {
        Surface::Plane { u_axis, v_axis, .. } => v3(*u_axis).cross(v3(*v_axis)).normalized(),
        Surface::Cylinder { frame, .. } => radial(frame).map(|(r, _)| r),
        Surface::Cone { frame, half_angle, .. } => radial(frame).and_then(|(r, z)| (r * half_angle.cos() - z * half_angle.sin()).normalized()),
        _ => None,
    }
}

/// A straight edge: a line, or a B-spline of degree 1 with two control points (as SMLib gives
/// an extrusion's rulings).
fn straight(c: &Curve) -> bool {
    match c {
        Curve::Line { .. } => true,
        Curve::Spline { knots, points } => points.len() == 2 && knots.len() == 4,
        _ => false,
    }
}

fn on_boundary(boundaries: &[Boundary; 2], a: Vec3, b: Vec3) -> Option<Boundary> {
    boundaries.iter().find_map(|boundary| {
        let tolerance = roundoff(&[a, b, boundary.a, boundary.b]);
        ((a.dist(boundary.a) <= tolerance && b.dist(boundary.b) <= tolerance) || (a.dist(boundary.b) <= tolerance && b.dist(boundary.a) <= tolerance))
            .then_some(*boundary)
    })
}

pub(crate) fn flags(body: &Body, mesh: &Mesh) -> (Vec<bool>, Vec<bool>) {
    crate::guard("smooth display edges", || Ok(classify(body, mesh, WORK))).unwrap_or_default()
}

fn classify(body: &Body, mesh: &Mesh, mut work: usize) -> (Vec<bool>, Vec<bool>) {
    let Ok(rec) = crate::native::brep_record(body) else { return (Vec::new(), Vec::new()) };
    if rec.faces.len() > MAX_FACES || mesh.edges.len() > MAX_EDGES {
        return (Vec::new(), Vec::new());
    }
    let mut split_points = 0usize;
    for line in body.split_keep() {
        if charge(&mut work, line.len().saturating_add(1)).is_none() {
            return (Vec::new(), Vec::new());
        }
        let Some(total) = split_points.checked_add(line.len()) else { return (Vec::new(), Vec::new()) };
        split_points = total;
    }
    // Each face's outward normal is its surface's natural one.
    let mut forward = vec![None; rec.faces.len()];
    for sh in &rec.shells {
        if let Shell::Faces { faces, .. } = sh {
            for f in faces {
                if let Some(slot) = forward.get_mut(f.face as usize) {
                    *slot = Some(f.forward);
                }
            }
        }
    }
    let mut boundaries = Vec::with_capacity(rec.faces.len());
    for (face, orientation) in rec.faces.iter().zip(&forward) {
        let edge_count = face
            .loops
            .iter()
            .map(|l| match l {
                Loop::Edges { edges } => edges.len(),
                Loop::Vertex { .. } => 0,
            })
            .fold(0usize, usize::saturating_add);
        if face.loops.len() > 16 || edge_count > 16 || charge(&mut work, edge_count + 1).is_none() {
            boundaries.push(None);
            continue;
        }
        let normals = match (&face.surface, orientation) {
            (Surface::Spline { u_knots, v_knots, points }, Some(o)) => ruled(u_knots, v_knots, points, *o, &mut work).map(Normals::Ruled),
            (s @ (Surface::Plane { .. } | Surface::Cylinder { .. } | Surface::Cone { .. }), Some(o)) => {
                Some(Normals::Analytic { surface: s.clone(), orientation: *o })
            }
            _ => None,
        };
        boundaries.push(normals);
    }
    let mut out = vec![false; mesh.edges.len()];
    let mut sharp = vec![false; mesh.edges.len()];
    // The mesh's edges are the record's, in its order.
    for (current, edge) in rec.edges.iter().enumerate().take(mesh.edges.len().min(MAX_EDGES)) {
        if charge(&mut work, 1).is_none() {
            break;
        }
        let Some([a, b]) = mesh.edge_faces.get(current).map(Vec::as_slice) else { continue };
        if a == b {
            continue;
        }
        let Some(line) = mesh.edges.get(current) else { continue };
        let Some(split_work) = line.len().checked_add(1).and_then(|n| n.checked_mul(split_points)) else { break };
        if charge(&mut work, split_work).is_none() {
            break;
        }
        if crate::heal::on_split_line(line, body.split_keep()) {
            if let Some(flag) = sharp.get_mut(current) {
                *flag = true;
            }
            continue;
        }
        let (Some(Some(ba)), Some(Some(bb))) = (boundaries.get(*a as usize), boundaries.get(*b as usize)) else { continue };
        // Only exact straight rulings qualify; a bowed/twisted edge with matching endpoints
        // must remain visible.
        if !straight(&edge.curve) {
            continue;
        }
        let ends = |v: u32| rec.vertices.get(v as usize).map(|x| Vec3::new(x.point[0], x.point[1], x.point[2]));
        let (Some(p), Some(q)) = (ends(edge.vertices[0]), ends(edge.vertices[1])) else { continue };
        let (Some(na), Some(nb)) = (normal_along(ba, p, q), normal_along(bb, p, q)) else { continue };
        let uncertainty = na.error + nb.error;
        let cross = na.normal.cross(nb.normal).len();
        if na.normal.dot(nb.normal) > 0.
            && cross + uncertainty <= ANGLE
            && let Some(flag) = out.get_mut(current)
        {
            *flag = true;
        } else if (cross > ANGLE + uncertainty || na.normal.dot(nb.normal) < -uncertainty)
            && let Some(flag) = sharp.get_mut(current)
        {
            *flag = true;
        }
    }
    (out, sharp)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
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

    #[test]
    fn a_small_real_crease_is_visible() {
        let mut outer = circle(10., 8);
        if let Seg2::Cubic { p0, p1, .. } = &mut outer.segs[0] {
            p1.x += (p1.y - p0.y) * 1e-6;
        }
        let body = crate::extrude(&Plane::XY, &[Region2 { outer, holes: vec![] }], 0., 3.).unwrap().remove(0);
        let mesh = body.mesh(&crate::MeshTolerance::chord(0.02)).unwrap();
        assert_eq!(mesh.smooth_edges.iter().filter(|f| **f).count(), 7);
        let crease = mesh
            .edges
            .iter()
            .position(|line| {
                line.first().is_some_and(|p| (p.x - 10.).abs() < 1e-8 && p.y.abs() < 1e-8)
                    && line.last().is_some_and(|p| (p.x - 10.).abs() < 1e-8 && p.y.abs() < 1e-8)
                    && line.first().zip(line.last()).is_some_and(|(a, b)| (a.z - b.z).abs() > 2.9)
            })
            .unwrap();
        assert!(!mesh.smooth_edges[crease]);
        assert!(mesh.sharp_edges[crease]);
        assert!(mesh.shaded_edge_visible(crease));
        assert!(mesh.edge_pickable(crease));
    }

    #[test]
    fn actual_authored_split_of_a_curved_ruled_wall_stays_visible() {
        let body = ring(4);
        let face = crate::native::brep_record(&body)
            .unwrap()
            .faces
            .iter()
            .position(|face| match &face.surface {
                Surface::Spline { u_knots, v_knots, points } => ruled(u_knots, v_knots, points, true, &mut { WORK }).is_some(),
                _ => false,
            })
            .unwrap();
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
    fn singular_twisted_and_reversed_normal_certificates_are_conservative() {
        // A Bezier surface of `points[i][j]`, i along u, one span each way.
        let ruled_of = |points: Vec<Vec<[f64; 3]>>, orientation: bool, work: &mut usize| {
            let knots = |n: usize| [vec![0.0; n], vec![1.0; n]].concat();
            ruled(&knots(points.len()), &knots(points.first().map_or(0, Vec::len)), &points, orientation, work)
        };
        let p = |x: f64, y: f64, z: f64| [x, y, z];
        let straight = vec![vec![p(0., 0., 0.), p(0., 0., 3.)], vec![p(1., 0., 0.), p(1., 0., 3.)]];
        let forward = ruled_of(straight.clone(), true, &mut { WORK }).unwrap();
        let backward = ruled_of(straight.clone(), false, &mut { WORK }).unwrap();
        assert!(forward[0].normal.dot(backward[0].normal) < -0.99);
        let twisted = vec![vec![p(0., 0., 0.), p(0., 0., 3.)], vec![p(1., 0., 0.), p(1., 1., 3.)]];
        assert!(ruled_of(twisted, true, &mut { WORK }).is_none());
        let singular = vec![vec![p(0., 0., 0.), p(0., 0., 3.)], vec![p(0., 0., 0.), p(0., 0., 3.)], vec![p(1., 0., 0.), p(1., 0., 3.)]];
        assert!(ruled_of(singular, true, &mut { WORK }).is_none());
        assert!(ruled_of(straight, true, &mut 0).is_none());
    }
}

