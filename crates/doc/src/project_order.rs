//! Stable face projection order: outer boundary first, then holes by geometry.
//! B-rep edge indices depend on face enumeration and must not choose sketch curve IDs.

use std::collections::{BTreeMap, BTreeSet};

use solidcraft_geom::{Aabb3, Mesh, Plane, Vec3};

use crate::{DocError, Result};

type Key = (i64, i64, i64);

/// How far apart two edge ends of `mesh` may be and still meet. A round's fitted edges end a few
/// 1e-5 mm short of their neighbours, far more than the tolerance that ranks equal areas (#8284).
pub(super) fn join_tolerance(mesh: &Mesh) -> f64 {
    let diagonal = mesh.bounds().diagonal();
    (diagonal * 1e-6).max(1e-9)
}

/// Boundary of the picked face as drawn: a closed surface cut into pieces (a cylinder or a
/// round's torus in halves) projects as the whole patch, since its seams are hidden.
pub(super) fn face_edges(mesh: &Mesh, face: u32, plane: &Plane) -> Result<Vec<usize>> {
    let patch = mesh.surface_patch(face as usize);
    if patch.len() > 1 {
        return family_edges(mesh, &patch.into_iter().collect(), plane);
    }
    let edges = mesh.face_edges(face).into_iter().filter(|e| !mesh.seams.get(*e).copied().unwrap_or(false)).collect();
    boundary_edges(mesh, edges, &[face as usize].into_iter().collect(), plane)
}

/// Full boundary of an explicitly selected analytic planar piece. Display seams
/// hide subdivisions for picking, but cannot remove a raw piece's closing edge.
pub(super) fn raw_face_edges(mesh: &Mesh, face: u32, plane: &Plane) -> Result<Vec<usize>> {
    boundary_edges(mesh, mesh.face_edges(face), &[face as usize].into_iter().collect(), plane)
}

/// Exterior of an exact current face family. Shared topological edges are internal;
/// geometric proximity never merges distinct edges or chooses a different face.
pub(super) fn family_edges(mesh: &Mesh, selected: &std::collections::BTreeSet<usize>, plane: &Plane) -> Result<Vec<usize>> {
    let mut edges = Vec::new();
    for (index, faces) in mesh.edge_faces.iter().enumerate() {
        let count = faces.iter().filter(|face| selected.contains(&(**face as usize))).count();
        if count == 0 {
            continue;
        }
        if faces.len() > 2 || (faces.len() == 2 && faces.first() == faces.get(1)) {
            return Err(DocError::Invalid("face family has ambiguous edge adjacency".into()));
        }
        if count == 1 && !mesh.seams.get(index).copied().unwrap_or(false) {
            edges.push(index);
        }
    }
    boundary_edges(mesh, edges, selected, plane)
}

/// `faces`: the faces the edges bound. Where boundaries touch at a vertex (a pinch: an even number of
/// boundary edges meet there), the walk stays on the face it came along, or else takes the first
/// edge in geometric order; either way every loop closes. An odd number is a real branch.
fn boundary_edges(mesh: &Mesh, mut edges: Vec<usize>, faces: &BTreeSet<usize>, plane: &Plane) -> Result<Vec<usize>> {
    let bounds = mesh.bounds();
    let origin = bounds.min;
    let local_origin = plane.to_local(origin);
    let tol = (bounds.diagonal() * 1e-8).max(1e-9);
    let join = join_tolerance(mesh);
    if !tol.is_finite() || !join.is_finite() {
        return Err(DocError::Invalid("face projection has invalid bounds".into()));
    }
    let key = |p: Vec3| -> Key {
        let p = p - origin;
        ((p.x / join).round() as i64, (p.y / join).round() as i64, (p.z / join).round() as i64)
    };
    if edges.iter().any(|e| !mesh.edges.get(*e).is_some_and(|p| p.len() >= 2 && p.iter().all(|p| p.is_finite()))) {
        return Err(DocError::Invalid("face projection has an invalid boundary edge".into()));
    }
    if edges.len() > solidcraft_sketch::MAX_LINK_ENTITIES {
        return Err(DocError::Invalid("face projection exceeds the boundary edge limit".into()));
    }
    let geometry_key = |e: usize| {
        let pts = mesh.edges.get(e).map(Vec::as_slice).unwrap_or_default();
        let b: Aabb3 = pts.iter().copied().collect();
        (key(b.min), key(b.max))
    };
    // A round that dies away at a corner leaves an edge from a point to itself: it bounds nothing,
    // and left in, it reads as a branch where the real edges meet.
    edges.retain(|e| mesh.edges.get(*e).is_some_and(|p| p.len() > 2 || p.first().zip(p.last()).is_some_and(|(a, b)| a.dist(*b) > join)));
    edges.sort_by_key(|e| geometry_key(*e));
    let mut at: BTreeMap<Key, Vec<usize>> = BTreeMap::new();
    for e in &edges {
        if let Some(pts) = mesh.edges.get(*e) {
            for point in [pts.first(), pts.last()].into_iter().flatten() {
                at.entry(key(*point)).or_default().push(*e);
            }
        }
    }
    let mut used = vec![false; mesh.edges.len()];
    let mut boundaries = Vec::new();
    for start_edge in &edges {
        if used.get(*start_edge).copied().unwrap_or(true) {
            continue;
        }
        let Some(pts) = mesh.edges.get(*start_edge) else { continue };
        let (Some(a), Some(b)) = (pts.first(), pts.last()) else { continue };
        let start = if key(*a) <= key(*b) { *a } else { *b };
        let mut current = start;
        let mut edge = *start_edge;
        let mut boundary = Vec::new();
        let mut twice_area = 0.0;
        for _ in 0..edges.len() {
            let Some(pts) = mesh.edges.get(edge) else { break };
            let (Some(a), Some(b)) = (pts.first(), pts.last()) else { break };
            let forward = current.dist(*a) <= current.dist(*b);
            for pair in pts.windows(2) {
                let (Some(a), Some(b)) = (pair.first(), pair.get(1)) else { continue };
                let (a, b) = (plane.to_local(*a) - local_origin, plane.to_local(*b) - local_origin);
                twice_area += if forward { a.cross(b) } else { b.cross(a) };
            }
            current = if forward { *b } else { *a };
            if let Some(slot) = used.get_mut(edge) {
                *slot = true;
            }
            boundary.push(edge);
            if current.dist(start) <= join {
                break;
            }
            // Neighbouring quantization cells cover points on either side of a rounding
            // boundary. Connectivity is checked in world space, not in the projection.
            let (x, y, z) = key(current);
            let mut incident = Vec::new();
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        let cell = (x.saturating_add(dx), y.saturating_add(dy), z.saturating_add(dz));
                        if let Some(list) = at.get(&cell) {
                            incident.extend(list.iter().copied().filter(|e| {
                                mesh.edges.get(*e).is_some_and(|p| {
                                    p.first().is_some_and(|a| a.dist(current) <= join) || p.last().is_some_and(|b| b.dist(current) <= join)
                                })
                            }));
                        }
                    }
                }
            }
            incident.sort_by_key(|e| geometry_key(*e));
            incident.dedup();
            let mut candidates: Vec<usize> = incident.iter().copied().filter(|e| !used.get(*e).copied().unwrap_or(true)).collect();
            if candidates.len() > 1 {
                if incident.len() % 2 != 0 {
                    return Err(DocError::Invalid("face projection has a branched boundary".into()));
                }
                let face_of = |e: usize| -> Vec<u32> {
                    mesh.edge_faces.get(e).map(|fs| fs.iter().copied().filter(|f| faces.contains(&(*f as usize))).collect()).unwrap_or_default()
                };
                let here = face_of(edge);
                let same: Vec<usize> = candidates.iter().copied().filter(|c| face_of(*c).iter().any(|f| here.contains(f))).collect();
                if same.len() == 1 {
                    candidates = same;
                }
            }
            let Some(next) = candidates.first() else { break };
            edge = *next;
        }
        boundary.sort_by_key(|e| geometry_key(*e));
        let closed = current.dist(start) <= join;
        if !closed || !twice_area.is_finite() {
            return Err(DocError::Invalid("face projection has an open or invalid boundary".into()));
        }
        // Equal-sized holes use their geometric key, not round-off from summing the
        // same area at different positions or walking their edges in reverse.
        let area = (twice_area.abs() / (tol * bounds.diagonal().max(tol))).round() as i64;
        let first = boundary.first().copied().map(geometry_key);
        boundaries.push((area, first, boundary));
    }
    boundaries.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    Ok(boundaries.into_iter().flat_map(|(_, _, edges)| edges).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_family_omits_only_exact_shared_topological_edges() {
        let mut mesh = Mesh::default();
        for (a, b, faces) in [
            ((0., 0.), (2., 0.), vec![0]),
            ((2., 0.), (2., 1.), vec![0]),
            ((2., 1.), (0., 1.), vec![0, 1]),
            ((0., 1.), (0., 0.), vec![0]),
            ((2., 1.), (2., 2.), vec![1]),
            ((2., 2.), (0., 2.), vec![1]),
            ((0., 2.), (0., 1.), vec![1]),
        ] {
            mesh.edges.push(vec![Vec3::new(a.0, a.1, 0.), Vec3::new(b.0, b.1, 0.)]);
            mesh.positions.extend([Vec3::new(a.0, a.1, 0.), Vec3::new(b.0, b.1, 0.)]);
            mesh.edge_faces.push(faces);
        }
        let selected = [0, 1].into_iter().collect();
        let edges = family_edges(&mesh, &selected, &Plane::XY).unwrap();
        assert_eq!(edges.len(), 6);
        assert!(!edges.contains(&2));
        let geometry: Vec<_> = edges.iter().flat_map(|edge| super::super::fit_world(&Plane::XY, &mesh.edges[*edge])).collect();
        assert_eq!(super::super::merge_family_lines(geometry).unwrap().len(), 4);
        assert_eq!(family_edges(&mesh, &[0].into_iter().collect(), &Plane::XY).unwrap().len(), 4);
        mesh.edge_faces[2] = vec![0, 1, 2];
        assert!(family_edges(&mesh, &selected, &Plane::XY).is_err());
        mesh.edge_faces[2] = vec![0, 0];
        assert!(family_edges(&mesh, &selected, &Plane::XY).is_err());
    }

    #[test]
    fn boundaries_touching_at_a_vertex_project() {
        // Two squares of one face family meet only at the corner (1, 1): four boundary edges meet
        // there (a pinch), and the walk keeps each square's loop on its own face.
        let mut mesh = Mesh::default();
        for (face, x, y) in [(0, 0., 0.), (1, 1., 1.)] {
            let corners = [Vec3::new(x, y, 0.), Vec3::new(x + 1., y, 0.), Vec3::new(x + 1., y + 1., 0.), Vec3::new(x, y + 1., 0.)];
            mesh.positions.extend(corners);
            for i in 0..4 {
                mesh.edges.push(vec![corners[i], corners[(i + 1) % 4]]);
                mesh.edge_faces.push(vec![face]);
            }
        }
        let edges = family_edges(&mesh, &[0, 1].into_iter().collect(), &Plane::XY).unwrap();
        assert_eq!(edges.len(), 8);
        // A third edge ending at the corner leaves an odd number there: a real branch.
        mesh.edges.push(vec![Vec3::new(1., 1., 0.), Vec3::new(3., 0., 0.)]);
        mesh.edge_faces.push(vec![1]);
        assert!(family_edges(&mesh, &[0, 1].into_iter().collect(), &Plane::XY).unwrap_err().to_string().contains("branched"));
    }

    #[test]
    fn distinct_crossing_boundaries_are_not_geometrically_cancelled() {
        let mut mesh = Mesh::default();
        for (face, x, y) in [(0, 0., 0.), (1, 1., 1.)] {
            let corners = [Vec3::new(x, y, 0.), Vec3::new(x + 2., y, 0.), Vec3::new(x + 2., y + 2., 0.), Vec3::new(x, y + 2., 0.)];
            mesh.positions.extend(corners);
            for i in 0..4 {
                mesh.edges.push(vec![corners[i], corners[(i + 1) % 4]]);
                mesh.edge_faces.push(vec![face]);
            }
        }
        let edges = family_edges(&mesh, &[0, 1].into_iter().collect(), &Plane::XY).unwrap();
        assert_eq!(edges.len(), 8);
        let geom: Vec<_> = edges.iter().flat_map(|edge| super::super::fit_world(&Plane::XY, &mesh.edges[*edge])).collect();
        let mut sk = solidcraft_sketch::Sketch::new();
        let link = sk
            .add_link(solidcraft_sketch::LinkKind::Project, solidcraft_sketch::LinkSource::Face { body: "ambiguous".into(), at: Vec3::ZERO }, &geom)
            .unwrap();
        assert!(sk.update_link(&link, &geom).is_err());
    }

    #[test]
    fn family_line_normalization_preserves_corners_arcs_branches_and_retracing() {
        use solidcraft_geom::Vec2;
        use solidcraft_sketch::LinkGeom;
        let line = |a, b| LinkGeom::Line(Vec2::new(a, 0.), Vec2::new(b, 0.));
        assert_eq!(super::super::merge_family_lines(vec![line(0., 1.), line(2., 1.)]).unwrap(), vec![line(0., 2.)]);
        for geometry in [
            vec![line(0., 1.), line(1., 0.5)],
            vec![line(0., 1.), LinkGeom::Line(Vec2::new(1., 0.), Vec2::new(2., 1e-8))],
            vec![line(0., 1.), line(1., 2.), LinkGeom::Line(Vec2::new(1., 0.), Vec2::new(1., 1.))],
            vec![line(0., 1.), line(1., 2.), LinkGeom::Conic { a: Vec2::new(1., 0.), apex: Vec2::new(2., 1.), b: Vec2::new(3., 1.), rho: 0.5 }],
            vec![
                line(0., 1.),
                line(1., 2.),
                LinkGeom::Spline { pts: vec![Vec2::new(1., 0.), Vec2::new(2., 1.), Vec2::new(3., 1.)], control: false, degree: 2, periodic: false },
            ],
            vec![line(0., 1.), LinkGeom::Arc { c: Vec2::new(1., 1.), a: Vec2::new(1., 0.), b: Vec2::new(2., 1.) }],
            vec![line(0., 1.), line(1.00001, 2.)],
        ] {
            assert_eq!(super::super::merge_family_lines(geometry.clone()).unwrap(), geometry);
        }
    }

    fn rectangles() -> Mesh {
        let mut mesh = Mesh::default();
        for (x, y) in [(40.0, 30.0), (38.0, 28.0), (4.0, 2.0)] {
            let pts = [Vec3::new(-x, -y, 0.0), Vec3::new(x, -y, 0.0), Vec3::new(x, y, 0.0), Vec3::new(-x, y, 0.0)];
            mesh.positions.extend(pts);
            for i in 0..4 {
                mesh.edges.push(vec![pts[i], pts[(i + 1) % 4]]);
                mesh.edge_faces.push(vec![7]);
            }
        }
        mesh
    }

    #[test]
    fn boundaries_and_edge_identity_are_independent_of_mesh_enumeration() {
        let baseline = rectangles();
        let expected: Vec<_> =
            face_edges(&baseline, 7, &Plane::XY).unwrap().into_iter().map(|e| super::super::fit_world(&Plane::XY, &baseline.edges[e])).collect();
        for shift in 0..baseline.edges.len() {
            for reverse in [false, true] {
                let mut mesh = baseline.clone();
                mesh.edges = (0..mesh.edges.len()).map(|i| baseline.edges[(i * 5) % baseline.edges.len()].clone()).collect();
                mesh.edges.rotate_left(shift);
                if reverse {
                    mesh.edges.reverse();
                    for pts in &mut mesh.edges {
                        pts.reverse();
                    }
                }
                let actual: Vec<_> = face_edges(&mesh, 7, &Plane::XY)
                    .unwrap()
                    .into_iter()
                    .map(|e| {
                        let mut pts = mesh.edges[e].clone();
                        // Lines' direction does not change their geometry.
                        if pts.first().unwrap().x > pts.last().unwrap().x || pts.first().unwrap().y > pts.last().unwrap().y {
                            pts.reverse();
                        }
                        super::super::fit_world(&Plane::XY, &pts)
                    })
                    .collect();
                let normalized: Vec<_> = expected
                    .iter()
                    .map(|g| {
                        let solidcraft_sketch::LinkGeom::Line(a, b) = g[0] else { panic!("line") };
                        if a.x > b.x || a.y > b.y { vec![solidcraft_sketch::LinkGeom::Line(b, a)] } else { g.clone() }
                    })
                    .collect();
                assert_eq!(actual, normalized);
                let ordered = face_edges(&mesh, 7, &Plane::XY).unwrap();
                for (loop_index, chunk) in ordered.chunks(4).enumerate() {
                    let want = [40.0, 38.0, 4.0][loop_index];
                    assert!(chunk.iter().flat_map(|e| &mesh.edges[*e]).all(|p| p.x.abs() <= want));
                    assert!(chunk.iter().flat_map(|e| &mesh.edges[*e]).any(|p| p.x.abs() == want));
                }
            }
        }
    }

    #[test]
    fn rounded_boundary_arc_sweeps_survive_individual_edge_reversal() {
        use solidcraft_geom::{Seg2, Vec2};
        use solidcraft_sketch::LinkGeom;
        let mut mesh = Mesh::default();
        for (extent, radius) in [(10.0, 3.0), (8.0, 1.0)] {
            for k in 0..4 {
                let angle = k as f64 * std::f64::consts::FRAC_PI_2;
                let rotate = |p: Vec2| (0..k).fold(p, |p, _| p.perp());
                let centre = rotate(Vec2::new(extent - radius, extent - radius));
                let arc = Seg2::Arc { center: centre, radius, start: angle, sweep: std::f64::consts::FRAC_PI_2 };
                let line = Seg2::Line { a: arc.point_at(1.0), b: rotate(Vec2::new(-(extent - radius), extent)) };
                for seg in [arc, line] {
                    let pts: Vec<_> = (0..=16).map(|i| Plane::XY.to_world(seg.point_at(i as f64 / 16.0))).collect();
                    mesh.positions.extend(pts.iter().copied());
                    mesh.edges.push(pts);
                    mesh.edge_faces.push(vec![2]);
                }
            }
        }
        let normalized = |mesh: &Mesh| -> Vec<Vec<LinkGeom>> {
            face_edges(mesh, 2, &Plane::XY)
                .unwrap()
                .into_iter()
                .map(|e| {
                    super::super::fit_world(&Plane::XY, &mesh.edges[e])
                        .into_iter()
                        .map(|g| match g {
                            LinkGeom::Line(a, b) if (a.x, a.y) > (b.x, b.y) => LinkGeom::Line(b, a),
                            g => g,
                        })
                        .collect()
                })
                .collect()
        };
        let expected = normalized(&mesh);
        for shift in 0..mesh.edges.len() {
            let mut permuted = mesh.clone();
            permuted.edges = (0..mesh.edges.len()).map(|i| mesh.edges[(i * 5 + shift) % mesh.edges.len()].clone()).collect();
            for (i, points) in permuted.edges.iter_mut().enumerate() {
                if i % 2 == 0 {
                    points.reverse();
                }
            }
            let actual = normalized(&permuted);
            for (a, b) in actual.iter().zip(&expected) {
                assert_eq!(a.len(), b.len());
                for (a, b) in a.iter().zip(b) {
                    match (a, b) {
                        (LinkGeom::Line(a, b), LinkGeom::Line(c, d)) => assert!(a.dist(*c) < 1e-9 && b.dist(*d) < 1e-9),
                        (LinkGeom::Arc { c, a, b }, LinkGeom::Arc { c: cc, a: aa, b: bb }) => {
                            assert!(c.dist(*cc) < 1e-9 && a.dist(*aa) < 1e-9 && b.dist(*bb) < 1e-9);
                        }
                        _ => panic!("boundary changed kind"),
                    }
                }
            }
        }
    }

    /// A 100 mm square whose edges each end `gap` short of the next one's start.
    fn square_with_gaps(gap: f64) -> Mesh {
        let corners = [Vec3::new(0., 0., 0.), Vec3::new(100., 0., 0.), Vec3::new(100., 100., 0.), Vec3::new(0., 100., 0.)];
        let mut mesh = Mesh::default();
        mesh.positions.extend(corners);
        for i in 0..4 {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            let back = (a - b).normalized().unwrap() * gap;
            mesh.edges.push(vec![a, b + back]);
            mesh.edge_faces.push(vec![0]);
        }
        mesh
    }

    #[test]
    fn edges_of_a_round_that_end_a_hair_short_of_their_neighbours_still_close() {
        // A fitted round's edges end a few 1e-5 mm from the vertex they share (#8284).
        let mesh = square_with_gaps(3e-5);
        assert_eq!(face_edges(&mesh, 0, &Plane::XY).unwrap().len(), 4);
        // A real gap is still an open boundary.
        let open = square_with_gaps(1e-2);
        assert!(face_edges(&open, 0, &Plane::XY).unwrap_err().to_string().contains("open"));
    }

    #[test]
    fn an_edge_from_a_point_to_itself_is_not_a_branch() {
        // Where a round dies away at a corner, the mesh keeps a zero-length edge there (#8284).
        let mut mesh = square_with_gaps(0.);
        for corner in mesh.positions.clone() {
            mesh.edges.push(vec![corner, corner]);
            mesh.edge_faces.push(vec![0, 1]);
        }
        assert_eq!(face_edges(&mesh, 0, &Plane::XY).unwrap().len(), 4);
    }

    #[test]
    fn curve_ends_a_hair_apart_become_one_point_and_distant_ones_stay() {
        use solidcraft_geom::Vec2;
        use solidcraft_sketch::LinkGeom;
        let mut geom = vec![
            LinkGeom::Line(Vec2::new(0., 0.), Vec2::new(10., 0.)),
            LinkGeom::Spline {
                pts: vec![Vec2::new(10. + 3e-5, 2e-5), Vec2::new(11., 1.), Vec2::new(12., 5.)],
                control: true,
                degree: 3,
                periodic: false,
            },
            LinkGeom::Line(Vec2::new(12., 5. - 1e-5), Vec2::new(0., 0.1)),
        ];
        super::super::meet_at_shared_ends(&mut geom, 1e-4);
        assert_eq!(
            geom[1],
            LinkGeom::Spline { pts: vec![Vec2::new(10., 0.), Vec2::new(11., 1.), Vec2::new(12., 5.)], control: true, degree: 3, periodic: false }
        );
        assert_eq!(geom[2], LinkGeom::Line(Vec2::new(12., 5.), Vec2::new(0., 0.1)));
    }

    #[test]
    fn incomplete_and_branched_boundaries_are_reported_without_partial_geometry() {
        let mut open = rectangles();
        open.edges.pop();
        open.edge_faces.pop();
        assert!(face_edges(&open, 7, &Plane::XY).unwrap_err().to_string().contains("open"));
        let mut branched = rectangles();
        branched.edges.push(vec![Vec3::new(-40.0, -30.0, 0.0), Vec3::ZERO]);
        branched.edge_faces.push(vec![7]);
        assert!(face_edges(&branched, 7, &Plane::XY).is_err());
        let mut invalid = rectangles();
        invalid.edges[0].push(Vec3::new(f64::NAN, 0.0, 0.0));
        assert!(face_edges(&invalid, 7, &Plane::XY).unwrap_err().to_string().contains("invalid"));
    }

    #[test]
    fn equal_area_holes_use_geometric_order_after_translation() {
        let mut baseline = rectangles();
        baseline.edges.truncate(4);
        baseline.edge_faces.truncate(4);
        for x in [-12.0, 12.0] {
            let pts = [Vec3::new(x - 2.0, -2.0, 0.0), Vec3::new(x + 2.0, -2.0, 0.0), Vec3::new(x + 2.0, 2.0, 0.0), Vec3::new(x - 2.0, 2.0, 0.0)];
            for i in 0..4 {
                baseline.edges.push(vec![pts[i], pts[(i + 1) % 4]]);
                baseline.edge_faces.push(vec![7]);
            }
        }
        let shift = Vec3::new(12345.6789, -23456.7891, 987.0);
        for p in &mut baseline.positions {
            *p += shift;
        }
        for points in &mut baseline.edges {
            for p in points {
                *p += shift;
            }
        }
        for offset in 0..baseline.edges.len() {
            let mut mesh = baseline.clone();
            mesh.edges = (0..mesh.edges.len()).map(|i| baseline.edges[(i * 5 + offset) % baseline.edges.len()].clone()).collect();
            for (i, points) in mesh.edges.iter_mut().enumerate() {
                if i % 2 == 0 {
                    points.reverse();
                }
            }
            let order = face_edges(&mesh, 7, &Plane::XY).unwrap();
            assert!(order[4..8].iter().flat_map(|e| &mesh.edges[*e]).all(|p| p.x < shift.x));
            assert!(order[8..12].iter().flat_map(|e| &mesh.edges[*e]).all(|p| p.x > shift.x));
        }
    }
}
