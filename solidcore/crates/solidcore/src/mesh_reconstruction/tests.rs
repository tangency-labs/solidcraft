//! Closed mesh fixtures use explicit input triangles, independent of a display tessellator.

use std::f64::consts::{PI, TAU};

use crate::surface::face_surface::{face_surface, record_face};
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::face;

use super::*;

fn convert(k: &mut Kernel, mesh: &TriMesh, tolerance: f64) -> Result<Brep> {
    reconstruct(k, &mesh.positions, &mesh.triangles, tolerance).map(|result| result.body)
}

fn cap_at(mesh: &mut TriMesh, outlines: &[&[Vec2]], height: f64) {
    let mut points = Vec::new();
    let mut ranges = Vec::new();
    for outline in outlines {
        let start = points.len();
        points.extend(outline.iter().map(|p| p.to_array()));
        ranges.push(start..points.len());
    }
    let offset = u32::try_from(mesh.positions.len()).unwrap();
    mesh.positions
        .extend(points.iter().map(|p| Vec3::new(p[0], p[1], height)));
    let triangles = cap::triangulate(&points, &ranges).unwrap();
    mesh.triangles
        .extend(triangles.iter().map(|t| t.map(|i| i + offset)));
}

fn walls(mesh: &mut TriMesh, outline: &[Vec2], lo: f64, hi: f64, skip_bottom_edge: bool) {
    let offset = u32::try_from(mesh.positions.len()).unwrap();
    for p in outline {
        mesh.positions
            .extend([Vec3::new(p.x, p.y, lo), Vec3::new(p.x, p.y, hi)]);
    }
    for (i, (a, b)) in outline
        .iter()
        .zip(outline.iter().cycle().skip(1))
        .enumerate()
    {
        if skip_bottom_edge && a.y == 0. && b.y == 0. {
            continue;
        }
        let i = offset + 2 * u32::try_from(i).unwrap();
        let j = if i + 2 == mesh.positions.len() as u32 {
            offset
        } else {
            i + 2
        };
        mesh.triangles.extend([[i, j, j + 1], [i, j + 1, i + 1]]);
    }
}

fn prism(outlines: &[&[Vec2]], height: f64) -> TriMesh {
    let mut mesh = TriMesh::default();
    cap_at(&mut mesh, outlines, 0.);
    cap_at(&mut mesh, outlines, height);
    for outline in outlines {
        walls(&mut mesh, outline, 0., height, false);
    }
    mesh
}

fn rectangle(x: f64, y: f64, width: f64, depth: f64) -> [Vec2; 4] {
    [
        Vec2::new(x, y),
        Vec2::new(x + width, y),
        Vec2::new(x + width, y + depth),
        Vec2::new(x, y + depth),
    ]
}

fn circular_points(center: Vec2, radius: f64, count: usize, phase: f64) -> Vec<Vec2> {
    (0..count)
        .map(|i| {
            let angle = (i as f64 + phase) * TAU / count as f64;
            center + Vec2::new(angle.cos(), angle.sin()) * radius
        })
        .collect()
}

fn move_mesh(mesh: &mut TriMesh, angle: f64, offset: Vec3, f32_input: bool) {
    let normal = normalize(Vec3::new(1., 2., 3.)).unwrap();
    for p in &mut mesh.positions {
        let q = *p * angle.cos()
            + normal.cross(*p) * angle.sin()
            + normal * (normal.dot(*p) * (1. - angle.cos()))
            + offset;
        *p = if f32_input {
            Vec3::new(
                f64::from(q.x as f32),
                f64::from(q.y as f32),
                f64::from(q.z as f32),
            )
        } else {
            q
        };
    }
}

fn cylinders(body: &Brep) -> usize {
    body.face_list()
        .iter()
        .filter(|&&id| {
            matches!(face::surface(body, id).unwrap().map(Surface::kind), Some(SurfaceKind::Cone(c)) if c.is_cylinder())
        })
        .count()
}

fn surface_kinds(body: &Brep) -> Vec<&'static str> {
    body.face_list()
        .iter()
        .map(
            |&id| match face::surface(body, id).unwrap().map(Surface::kind) {
                Some(SurfaceKind::Cone(c)) if c.is_cylinder() => "cylinder",
                Some(SurfaceKind::Plane(_)) => "plane",
                Some(SurfaceKind::SurfOfExtrusion(_)) => "extrusion",
                Some(SurfaceKind::BSpline(_)) => "spline",
                _ => "other",
            },
        )
        .collect()
}

fn assert_volume(k: &mut Kernel, body: &mut Brep, expected: f64, tolerance: f64) {
    assert!(native::is_valid(k, body).unwrap());
    let volume = native::volume(k, body).unwrap().unwrap();
    assert!(
        (volume - expected).abs() < tolerance,
        "{volume} of {expected}"
    );
}

#[test]
fn reconstructs_planes_holes_and_round_walls_in_arbitrary_orientation() {
    let outer = [
        [0., 0.],
        [40., 0.],
        [40., 20.],
        [20., 20.],
        [20., 30.],
        [0., 30.],
    ]
    .map(Vec2::from_array);
    let hole = circular_points(Vec2::new(10., 10.), 4., 96, 0.);
    let mut mesh = prism(&[&outer, &hole], 15.);
    move_mesh(&mut mesh, 0.73, Vec3::new(31., -17., 9.), false);
    let mut k = Kernel::default();
    let mut body = convert(&mut k, &mesh, 0.05).unwrap();
    assert_volume(&mut k, &mut body, (1000. - PI * 16.) * 15., 1e-6);
    assert_eq!(cylinders(&body), 1);
    assert!(body.face_list().len() < 15);
}

#[test]
fn hinted_caps_keep_straight_sides_across_different_subdivisions() {
    let outer = [
        Vec2::new(0., 0.),
        Vec2::new(40., 0.),
        Vec2::new(40., 20.),
        Vec2::new(20., 20.),
        Vec2::new(20., 30.),
        Vec2::new(0., 30.),
    ];
    let subdivide = |counts: [usize; 6]| {
        outer
            .iter()
            .zip(outer.iter().cycle().skip(1))
            .zip(counts)
            .enumerate()
            .flat_map(|(edge, ((&a, &b), n))| {
                (0..n).map(move |i| {
                    let t = i as f64 / n as f64;
                    (a + (b - a) * t, (edge as f64 + t) / 6.)
                })
            })
            .collect::<Vec<_>>()
    };
    let circle = |count| {
        circular_points(Vec2::new(10., 10.), 4., count, 0.)
            .into_iter()
            .enumerate()
            .map(|(i, p)| (p, i as f64 / count as f64))
            .collect::<Vec<_>>()
    };
    let lower = subdivide([11, 2, 3, 2, 2, 10]);
    let upper = subdivide([7, 2, 2, 2, 4, 7]);
    let lower_circle = circle(64);
    let upper_circle = circle(68);
    assert_eq!((lower.len(), upper.len()), (30, 24));
    let coordinates = |ring: &[(Vec2, f64)]| ring.iter().map(|&(p, _)| p).collect::<Vec<_>>();
    let (lower_points, upper_points) = (coordinates(&lower), coordinates(&upper));
    let (lower_hole, upper_hole) = (coordinates(&lower_circle), coordinates(&upper_circle));
    let mut mesh = TriMesh::default();
    cap_at(&mut mesh, &[&lower_points, &lower_hole], 0.);
    cap_at(&mut mesh, &[&upper_points, &upper_hole], 15.);
    // Triangulate between the independently sampled rings by their original perimeter order.
    let bridge = |mesh: &mut TriMesh, lower: &[(Vec2, f64)], upper: &[(Vec2, f64)]| {
        let offset = mesh.positions.len() as u32;
        mesh.positions
            .extend(lower.iter().map(|(p, _)| Vec3::new(p.x, p.y, 0.)));
        mesh.positions
            .extend(upper.iter().map(|(p, _)| Vec3::new(p.x, p.y, 15.)));
        let (mut i, mut j) = (0, 0);
        while i < lower.len() || j < upper.len() {
            let a = offset + (i % lower.len()) as u32;
            let b = offset + lower.len() as u32 + (j % upper.len()) as u32;
            let next_lower = if i < lower.len() {
                lower.get(i + 1).map_or(1., |p| p.1)
            } else {
                f64::INFINITY
            };
            let next_upper = if j < upper.len() {
                upper.get(j + 1).map_or(1., |p| p.1)
            } else {
                f64::INFINITY
            };
            if next_lower <= next_upper {
                let c = offset + ((i + 1) % lower.len()) as u32;
                mesh.triangles.push([a, c, b]);
                i += 1;
            } else {
                let c = offset + lower.len() as u32 + ((j + 1) % upper.len()) as u32;
                mesh.triangles.push([a, c, b]);
                j += 1;
            }
        }
    };
    bridge(&mut mesh, &lower, &upper);
    bridge(&mut mesh, &lower_circle, &upper_circle);
    let mut k = Kernel::default();
    let result = reconstruct(&mut k, &mesh.positions, &mesh.triangles, 0.05).unwrap();
    assert_eq!(result.kind, ReconstructionKind::Analytic);
    let mut body = result.body;
    assert_eq!(body.face_list().len(), 9);
    assert_eq!(cylinders(&body), 1);
    assert_volume(&mut k, &mut body, (1000. - 16. * PI) * 15., 1e-6);
}

#[test]
fn planar_boxes_are_exact_and_coarse_polygons_are_not_rounded() {
    let mesh = prism(&[&rectangle(0., 0., 10., 20.)], 30.);
    let mut k = Kernel::default();
    let mut body = convert(&mut k, &mesh, 0.01).unwrap();
    assert_eq!(body.face_list().len(), 6);
    assert_volume(&mut k, &mut body, 6000., 1e-6);
    let points = circular_points(Vec2::ZERO, 10., 6, 0.);
    assert_eq!(fit_loop(&points, 1., 1e-6).segs.len(), 6);
    let points = circular_points(Vec2::ZERO, 10., 16, 0.);
    assert_eq!(fit_loop(&points, 0.001, 1e-6).segs.len(), 16);
    assert_exact_circle(&fit_loop(&points, 0.2, 1e-6), Vec2::ZERO, 10.);
}

fn assert_exact_circle(fitted: &Loop2, center: Vec2, radius: f64) {
    let [Seg2::Arc {
        center: c,
        radius: r,
        sweep,
        ..
    }] = fitted.segs.as_slice()
    else {
        panic!("not one arc: {fitted:?}")
    };
    assert!(
        c.dist(center) < 1e-9 && (r - radius).abs() < 1e-9,
        "centre {c:?}, radius {r}"
    );
    assert!((sweep.abs() - TAU).abs() < 1e-9, "sweep {sweep}");
}

#[test]
fn refuses_open_and_hostile_inputs() {
    let mut k = Kernel::default();
    let open = [Vec3::ZERO, Vec3::X, Vec3::Y];
    assert!(reconstruct(&mut k, &open, &[[0, 1, 2]], 0.05).is_err());
    assert!(reconstruct(&mut k, &open, &[[0, 1, u32::MAX]], 0.05).is_err());
    let mesh = prism(&[&rectangle(0., 0., 10., 10.)], 10.);
    let mut nonmanifold = mesh.clone();
    nonmanifold.triangles.push(nonmanifold.triangles[0]);
    assert!(convert(&mut k, &nonmanifold, 0.05).is_err());
    for tolerance in [f64::NAN, f64::INFINITY, -1., 0., 2.] {
        assert!(convert(&mut k, &mesh, tolerance).is_err());
    }
    for coordinate in [f64::NAN, f64::INFINITY, f64::MAX, 1e9, 1e30] {
        let mesh = TriMesh {
            positions: vec![Vec3::new(coordinate, 0., 0.)],
            triangles: vec![],
        };
        assert!(convert(&mut k, &mesh, 0.05).is_err());
    }
}

#[test]
fn a_zero_area_triangle_is_dropped() {
    let mut mesh = prism(&[&rectangle(0., 0., 20., 10.)], 5.);
    let first = u32::try_from(mesh.positions.len()).unwrap();
    mesh.positions
        .extend([Vec3::ZERO, Vec3::new(10., 0., 0.), Vec3::new(20., 0., 0.)]);
    mesh.triangles.push([first, first + 1, first + 2]);
    let mut k = Kernel::default();
    let mut body = convert(&mut k, &mesh, 0.01).unwrap();
    assert_eq!(body.face_list().len(), 6);
    assert_volume(&mut k, &mut body, 1000., 1e-6);
    let sliver = [Vec3::ZERO, Vec3::X, Vec3::new(2., 0., 0.)];
    let error = reconstruct(&mut k, &sliver, &[[0, 1, 2]], 0.05)
        .unwrap_err()
        .to_string();
    assert!(error.contains("no triangle of any area"), "{error}");
}

#[test]
fn mesh_budget_is_checked_before_welding() {
    let mut k = Kernel::default();
    let triangles = vec![[0, 1, 2]; MAX_TRIANGLES + 1];
    let error = reconstruct(&mut k, &[], &triangles, 0.05)
        .unwrap_err()
        .to_string();
    assert!(error.contains("50000 triangles"), "{error}");
    let positions = vec![Vec3::ZERO; MAX_TRIANGLES * 3 + 1];
    let error = reconstruct(&mut k, &positions, &[], 0.05)
        .unwrap_err()
        .to_string();
    assert!(error.contains("150000 vertices"), "{error}");
}

#[test]
fn fitting_cannot_open_a_hole_through_a_nearby_wall() {
    let outer = polygon(&rectangle(-1., -1., 2., 2.));
    let points = circular_points(Vec2::ZERO, 1.02, 12, 0.5);
    let inner = polygon(&points);
    let fitted = fit_loop(&points, 0.05, 1e-6);
    assert_exact_circle(&fitted, Vec2::ZERO, 1.02);
    assert!(!certify_nesting(
        &mut Kernel::default(),
        &[outer.clone(), inner],
        &[outer, fitted],
        1e-6,
        3.
    )
    .unwrap());
}

fn perforated_plate(rows: usize, cols: usize) -> TriMesh {
    let outer = rectangle(0., 0., cols as f64 * 4. + 2., rows as f64 * 4. + 2.);
    let holes: Vec<_> = (0..rows)
        .flat_map(|r| {
            (0..cols).map(move |c| rectangle(c as f64 * 4. + 2., r as f64 * 4. + 2., 2., 2.))
        })
        .collect();
    let outlines: Vec<_> = std::iter::once(outer.as_slice())
        .chain(holes.iter().map(<[Vec2; 4]>::as_slice))
        .collect();
    prism(&outlines, 3.)
}

#[test]
fn many_holes_keep_the_source_facets_when_analytic_work_is_bounded() {
    let mesh = perforated_plate(20, 20);
    let mut k = Kernel::default();
    let result = reconstruct(&mut k, &mesh.positions, &mesh.triangles, 0.05).unwrap();
    assert_eq!(result.kind, ReconstructionKind::Faceted);
    assert_eq!(result.body.face_list().len(), 1606);
    let mut body = result.body;
    assert!(
        (native::volume(&mut k, &mut body).unwrap().unwrap() - (82. * 82. - 400. * 4.) * 3.).abs()
            < 1e-6
    );
    let mesh = perforated_plate(7, 7);
    let mut body = convert(&mut k, &mesh, 0.05).unwrap();
    assert_volume(&mut k, &mut body, (30. * 30. - 49. * 4.) * 3., 1e-6);
}

fn notch() -> Vec<Vec2> {
    let mut points = vec![Vec2::ZERO, Vec2::new(20., 0.), Vec2::new(20., 12.)];
    points.extend((0..=48).map(|i| {
        let angle = -f64::from(i) * PI / 48.;
        Vec2::new(10. + 4. * angle.cos(), 12. + 4. * angle.sin())
    }));
    points.push(Vec2::new(0., 12.));
    points
}

#[test]
fn restores_a_partial_circular_notch_without_faceting_its_wall() {
    let profile = notch();
    let mesh = prism(&[&profile], 5.);
    let fitted = fit_loop(&profile, 0.05, mesh.size() * 1e-7);
    assert!(
        fitted.segs.iter().any(|s| matches!(s, Seg2::Arc { .. })),
        "{fitted:?}"
    );
    let mut k = Kernel::default();
    let mut body = convert(&mut k, &mesh, 0.05).unwrap();
    assert_eq!(cylinders(&body), 1, "{:?}", surface_kinds(&body));
    assert!(body.face_list().len() < 12, "{}", body.face_list().len());
    for &id in body.face_list() {
        let surface = face::surface(&body, id).unwrap().unwrap();
        if !matches!(surface.kind(), SurfaceKind::Cone(c) if c.is_cylinder()) {
            continue;
        }
        let domain = face::uv_domain(&body, id).unwrap();
        let record = record_face(&k, surface, &domain).unwrap();
        assert!(
            matches!(&record.surface, crate::eval::Surface::Cylinder { .. }),
            "{record:?}"
        );
        assert!(record.nurbs.is_some());
        let rebuilt = face_surface(
            &k,
            &record.surface,
            record.nurbs.as_ref(),
            record.domain,
            &[],
        )
        .unwrap();
        assert_eq!(
            record_face(&k, &rebuilt.surface, &rebuilt.uv_domain).unwrap(),
            record
        );
    }
    assert_volume(&mut k, &mut body, (240. - 8. * PI) * 5., 0.01);
}

#[test]
fn reconstructs_a_plate_with_perpendicular_notched_extrusions() {
    let plate = rectangle(0., 0., 40., 28.);
    let left = rectangle(4., 4., 20., 4.);
    let right = rectangle(4., 20., 20., 4.);
    let mut source = TriMesh::default();
    cap_at(&mut source, &[&plate], 0.);
    cap_at(&mut source, &[&plate, &left, &right], 3.);
    walls(&mut source, &plate, 0., 3., false);
    let profile = notch();
    for y in [4., 20.] {
        let mut upper = TriMesh::default();
        cap_at(&mut upper, &[&profile], 0.);
        cap_at(&mut upper, &[&profile], 4.);
        walls(&mut upper, &profile, 0., 4., true);
        let offset = u32::try_from(source.positions.len()).unwrap();
        source.positions.extend(
            upper
                .positions
                .iter()
                .map(|p| Vec3::new(4. + p.x, y + p.z, 3. + p.y)),
        );
        source
            .triangles
            .extend(upper.triangles.iter().map(|t| t.map(|i| i + offset)));
    }
    for rotated in [false, true] {
        let mut mesh = source.clone();
        if rotated {
            move_mesh(&mut mesh, 0.4, Vec3::new(11., -7., 5.), true);
        }
        let mut k = Kernel::default();
        let mut body = convert(&mut k, &mesh, 0.05)
            .unwrap_or_else(|error| panic!("rotated={rotated}: {error:?}"));
        assert!(cylinders(&body) >= 2, "{:?}", surface_kinds(&body));
        assert!(body.face_list().len() < 50);
        assert_volume(&mut k, &mut body, 3360. + 8. * (240. - 8. * PI), 0.1);
    }
}

/// Build a stepped cylindrical mesh whose two sections use different angular sampling phases.
fn phase_shifted_stepped_cylinder() -> TriMesh {
    let n = 96usize;
    let mut positions = Vec::new();
    let mut triangles = Vec::new();
    for (radius, z, phase) in [(12., 0., 0.), (12., 4., 0.3), (8., 4., 0.7), (8., 9., 0.1)] {
        for i in 0..n {
            let angle = (i as f64 + phase) * TAU / n as f64;
            positions.push(Vec3::new(
                f64::from((radius * angle.cos()) as f32),
                f64::from((radius * angle.sin()) as f32),
                z,
            ));
        }
    }
    for layer in 0..3 {
        for i in 0..n {
            let a = (layer * n + i) as u32;
            let b = (layer * n + (i + 1) % n) as u32;
            let c = ((layer + 1) * n + (i + 1) % n) as u32;
            let d = ((layer + 1) * n + i) as u32;
            triangles.extend([[a, b, c], [a, c, d]]);
        }
    }
    positions.extend([Vec3::ZERO, Vec3::new(0., 0., 9.)]);
    for i in 0..n {
        triangles.push([(4 * n) as u32, ((i + 1) % n) as u32, i as u32]);
        triangles.push([
            (4 * n + 1) as u32,
            (3 * n + i) as u32,
            (3 * n + (i + 1) % n) as u32,
        ]);
    }
    TriMesh {
        positions,
        triangles,
    }
}

#[test]
fn reconstructs_a_stepped_cylinder_with_different_tessellation_phases() {
    let mesh = phase_shifted_stepped_cylinder();
    let mut k = Kernel::default();
    let mut body = convert(&mut k, &mesh, 0.05).unwrap();
    assert!(body.face_list().len() < 16, "{}", body.face_list().len());
    assert_eq!(cylinders(&body), 2);
    assert_volume(&mut k, &mut body, 896. * PI, 0.03);
}

#[test]
fn rotated_f32_stepped_cylinder_preserves_the_analytic_shape() {
    let mut mesh = phase_shifted_stepped_cylinder();
    move_mesh(&mut mesh, 0.43, Vec3::new(13., -9., 7.), true);
    let mut k = Kernel::default();
    let mut body = convert(&mut k, &mesh, 0.05).unwrap();
    assert!(body.face_list().len() < 16);
    assert_volume(&mut k, &mut body, 896. * PI, 0.03);
}

#[test]
fn radial_damage_outside_tolerance_is_not_fitted() {
    let mut mesh = phase_shifted_stepped_cylinder();
    mesh.positions[0].x += 0.4;
    let mut k = Kernel::default();
    let original = weld(&mesh.positions, &mesh.triangles, 0.05)
        .unwrap()
        .volume();
    let result = reconstruct(&mut k, &mesh.positions, &mesh.triangles, 0.05).unwrap();
    assert_eq!(result.kind, ReconstructionKind::Faceted);
    let mut body = result.body;
    assert!((native::volume(&mut k, &mut body).unwrap().unwrap() - original).abs() < 1e-5);
}

/// A cap with a square hole keeps its boundary indices, excludes the hole and has area 100 - 16.
#[test]
fn cap_triangulation_preserves_holes_and_boundary_indices() {
    let points = [
        [0., 0.],
        [10., 0.],
        [10., 10.],
        [0., 10.],
        [3., 3.],
        [7., 3.],
        [7., 7.],
        [3., 7.],
    ];
    let triangles = cap::triangulate(&points, &[0..4, 4..8]).unwrap();
    assert_eq!(triangles.len(), 8);
    let mut area = 0.;
    for t in &triangles {
        let [a, b, c] = t.map(|i| points[i as usize]);
        let cross = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        assert!(cross > 0.);
        area += cross * 0.5;
        let center = [(a[0] + b[0] + c[0]) / 3., (a[1] + b[1] + c[1]) / 3.];
        assert!(!(center[0] > 3. && center[0] < 7. && center[1] > 3. && center[1] < 7.));
    }
    assert!((area - 84.).abs() < 1e-12);
    for r in [0..4, 4..8] {
        for a in r.clone() {
            let b = r.start + (a + 1 - r.start) % r.len();
            assert_eq!(
                triangles
                    .iter()
                    .flat_map(|t| [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])])
                    .filter(|&(u, v)| (u as usize == a && v as usize == b)
                        || (u as usize == b && v as usize == a))
                    .count(),
                1
            );
        }
    }
}

/// Invalid ranges, non-finite coordinates and crossing boundaries never produce a partial cap.
#[test]
fn cap_triangulation_refuses_invalid_boundaries() {
    let crossing = [[0., 0.], [4., 4.], [0., 4.], [4., 0.]];
    assert!(cap::triangulate(&crossing, std::slice::from_ref(&(0..4))).is_none());
    assert!(cap::triangulate(&crossing, std::slice::from_ref(&(0..5))).is_none());
    assert!(cap::triangulate(&crossing, std::slice::from_ref(&(0..2))).is_none());
    assert!(cap::triangulate(
        &[[f64::NAN, 0.], [1., 0.], [0., 1.]],
        std::slice::from_ref(&(0..3))
    )
    .is_none());
}

/// Each convex face is triangulated independently; conversion welds the shared corners.
fn polyhedron(faces: &[Vec<Vec3>], inside: Vec3) -> TriMesh {
    let mut mesh = TriMesh::default();
    for face in faces {
        let offset = mesh.positions.len() as u32;
        mesh.positions.extend(face);
        for i in 1..face.len() - 1 {
            let mut t = [offset, offset + i as u32, offset + i as u32 + 1];
            let [a, b, c] = t.map(|i| mesh.positions[i as usize]);
            if (b - a).cross(c - a).dot((a + b + c) / 3. - inside) < 0. {
                t.swap(1, 2);
            }
            mesh.triangles.push(t);
        }
    }
    mesh
}

fn octahedron(radius: f64) -> TriMesh {
    let mut faces = Vec::new();
    for z in [-radius, radius] {
        for i in 0..4 {
            let a = f64::from(i) * PI / 2.;
            let b = f64::from(i + 1) * PI / 2.;
            faces.push(vec![
                Vec3::new(0., 0., z),
                Vec3::new(radius * a.cos(), radius * a.sin(), 0.),
                Vec3::new(radius * b.cos(), radius * b.sin(), 0.),
            ]);
        }
    }
    polyhedron(&faces, Vec3::ZERO)
}

#[test]
fn a_rounded_mesh_keeps_its_facets_and_exact_polygonal_volume() {
    let mesh = octahedron(10.);
    let mut k = Kernel::default();
    let result = reconstruct(&mut k, &mesh.positions, &mesh.triangles, 0.05).unwrap();
    assert_eq!(result.kind, ReconstructionKind::Faceted);
    assert_eq!(result.body.face_list().len(), 8);
    assert!(surface_kinds(&result.body).iter().all(|&s| s == "plane"));
    let mut body = result.body;
    assert_volume(&mut k, &mut body, 4000. / 3., 1e-7);
}

#[test]
fn a_chamfered_part_keeps_one_face_per_flat() {
    let p = |x, y, z| Vec3::new(x, y, z);
    let faces = vec![
        vec![
            p(0., 0., 0.),
            p(20., 0., 0.),
            p(20., 10., 0.),
            p(10., 20., 0.),
            p(0., 20., 0.),
        ],
        vec![
            p(0., 0., 0.),
            p(0., 0., 20.),
            p(0., 10., 20.),
            p(0., 20., 10.),
            p(0., 20., 0.),
        ],
        vec![
            p(0., 0., 0.),
            p(20., 0., 0.),
            p(20., 0., 10.),
            p(10., 0., 20.),
            p(0., 0., 20.),
        ],
        vec![p(20., 0., 0.), p(20., 10., 0.), p(20., 0., 10.)],
        vec![p(0., 20., 0.), p(10., 20., 0.), p(0., 20., 10.)],
        vec![p(0., 0., 20.), p(10., 0., 20.), p(0., 10., 20.)],
        vec![
            p(20., 10., 0.),
            p(10., 20., 0.),
            p(0., 20., 10.),
            p(0., 10., 20.),
            p(10., 0., 20.),
            p(20., 0., 10.),
        ],
    ];
    let mesh = polyhedron(&faces, p(5., 5., 5.));
    let mut k = Kernel::default();
    let result = reconstruct(&mut k, &mesh.positions, &mesh.triangles, 0.05).unwrap();
    assert_eq!(result.kind, ReconstructionKind::Faceted);
    assert_eq!(result.body.face_list().len(), 7);
    assert_volume(&mut k, &mut { result.body }, 4000., 1e-6);
}

#[test]
fn subdivided_coplanar_rims_keep_every_shared_edge() {
    let p = |x, y, z| Vec3::new(x, y, z);
    let mut mesh = TriMesh::default();
    for (o, u, v) in [
        (p(0., 0., 0.), p(0., 10., 0.), p(20., 0., 0.)),
        (p(0., 0., 4.), p(20., 0., 0.), p(0., 10., 0.)),
        (p(0., 0., 0.), p(0., 0., 4.), p(0., 10., 0.)),
        (p(20., 0., 0.), p(0., 10., 0.), p(0., 0., 4.)),
        (p(0., 0., 0.), p(20., 0., 0.), p(0., 0., 4.)),
        (p(0., 10., 0.), p(0., 0., 4.), p(20., 0., 0.)),
    ] {
        let n = mesh.positions.len() as u32;
        mesh.positions.extend([
            o,
            o + u * 0.5,
            o + u,
            o + u + v * 0.5,
            o + u + v,
            o + u * 0.5 + v,
            o + v,
            o + v * 0.5,
            o + (u + v) * 0.5,
        ]);
        for i in 0..8 {
            mesh.triangles.push([n + 8, n + i, n + (i + 1) % 8]);
        }
    }
    let mut k = Kernel::default();
    let welded = weld(&mesh.positions, &mesh.triangles, 0.05).unwrap();
    let mut body = faceted::build(&mut k, &welded).unwrap();
    assert_eq!(body.face_list().len(), 6);
    assert_eq!(body.edge_list().len(), 24);
    assert_volume(&mut k, &mut body, 800., 1e-7);
}

#[test]
fn a_collinear_sliver_is_resolved_without_opening_a_closed_mesh() {
    let positions = vec![
        Vec3::ZERO,
        Vec3::new(10., 0., 0.),
        Vec3::new(0., 10., 0.),
        Vec3::new(0., 0., 10.),
        Vec3::new(5., 0., 0.),
    ];
    let oriented = |mut t: [u32; 3]| {
        let [a, b, c] = t.map(|i| positions[i as usize]);
        if (b - a)
            .cross(c - a)
            .dot((a + b + c) / 3. - Vec3::new(2.5, 2.5, 2.5))
            < 0.
        {
            t.swap(1, 2);
        }
        t
    };
    let abc = oriented([0, 1, 2]);
    let abd = oriented([0, 1, 3]);
    let k = (0..3)
        .find(|&k| {
            [abc[k], abc[(k + 1) % 3]]
                .iter()
                .all(|v| *v == 0 || *v == 1)
        })
        .unwrap();
    let (x, y) = (abc[k], abc[(k + 1) % 3]);
    let k = (0..3)
        .find(|&k| abd[k] == y && abd[(k + 1) % 3] == x)
        .unwrap();
    let r = abd[(k + 2) % 3];
    let mut triangles = vec![
        abc,
        oriented([0, 2, 3]),
        oriented([1, 2, 3]),
        [y, 4, r],
        [4, x, r],
    ];
    let mut kernel = Kernel::default();
    assert!(reconstruct(&mut kernel, &positions, &triangles, 0.05).is_err());
    triangles.push([x, 4, y]);
    let result = reconstruct(&mut kernel, &positions, &triangles, 0.05).unwrap();
    assert_eq!(result.kind, ReconstructionKind::Faceted);
    assert_volume(&mut kernel, &mut { result.body }, 1000. / 6., 1e-7);
}

#[test]
fn faceted_cavities_and_disjoint_shells_retain_their_material() {
    let mut source = octahedron(10.);
    for (mut part, offset, cavity) in [
        (octahedron(2.), Vec3::ZERO, true),
        (octahedron(3.), Vec3::new(30., 0., 0.), false),
    ] {
        let n = source.positions.len() as u32;
        source
            .positions
            .extend(part.positions.iter().map(|p| *p + offset));
        if cavity {
            for t in &mut part.triangles {
                t.swap(1, 2);
            }
        }
        source
            .triangles
            .extend(part.triangles.iter().map(|t| t.map(|i| i + n)));
    }
    let mut k = Kernel::default();
    let result = reconstruct(&mut k, &source.positions, &source.triangles, 0.05).unwrap();
    assert_eq!(result.kind, ReconstructionKind::Faceted);
    assert_volume(
        &mut k,
        &mut { result.body },
        4. / 3. * (1000. - 8. + 27.),
        1e-6,
    );
}

#[test]
fn partial_cylinders_do_not_depend_on_or_change_sweep_fixes() {
    let mesh = prism(&[&notch()], 5.);
    for enabled in [false, true] {
        let mut k = Kernel::default();
        k.fixes.linear_sweep_arc_is_cylinder = enabled;
        let result = reconstruct(&mut k, &mesh.positions, &mesh.triangles, 0.05).unwrap();
        assert_eq!(result.kind, ReconstructionKind::Analytic);
        assert_eq!(cylinders(&result.body), 1);
        assert_eq!(k.fixes.linear_sweep_arc_is_cylinder, enabled);
        assert_volume(&mut k, &mut { result.body }, (240. - 8. * PI) * 5., 0.01);
    }
}

#[test]
fn an_analytic_cap_budget_keeps_the_original_closed_facets() {
    let outline = circular_points(Vec2::ZERO, 10., MAX_BOUNDARY + 1, 0.);
    let mesh = prism(&[&outline], 3.);
    assert!(mesh.triangles.len() < MAX_TRIANGLES);
    let mut k = Kernel::default();
    let result = reconstruct(&mut k, &mesh.positions, &mesh.triangles, 0.05).unwrap();
    assert_eq!(result.kind, ReconstructionKind::Faceted);
    assert!(surface_kinds(&result.body).iter().all(|&s| s == "plane"));
}

#[test]
fn a_fitted_hole_that_crosses_a_near_wall_keeps_its_source_facets() {
    let outer = rectangle(-1., -1., 2., 2.);
    let hole = circular_points(Vec2::ZERO, 1.02, 12, 0.5);
    let mesh = prism(&[&outer, &hole], 3.);
    let mut k = Kernel::default();
    let result = reconstruct(&mut k, &mesh.positions, &mesh.triangles, 0.05).unwrap();
    assert_eq!(result.kind, ReconstructionKind::Faceted);
    assert_eq!(result.body.face_list().len(), 18);
    let expected = (4. - 6. * 1.02f64.powi(2) * (PI / 6.).sin()) * 3.;
    assert_volume(&mut k, &mut { result.body }, expected, 1e-7);
}

#[test]
fn many_closed_components_are_not_an_analytic_component_budget_refusal() {
    let mut mesh = TriMesh::default();
    for i in 0..33 {
        let part = octahedron(1.);
        let offset = mesh.positions.len() as u32;
        mesh.positions.extend(
            part.positions
                .iter()
                .map(|p| *p + Vec3::new(f64::from(i) * 4., 0., 0.)),
        );
        mesh.triangles
            .extend(part.triangles.iter().map(|t| t.map(|j| j + offset)));
    }
    let mut k = Kernel::default();
    let result = reconstruct(&mut k, &mesh.positions, &mesh.triangles, 0.05).unwrap();
    assert_eq!(result.kind, ReconstructionKind::Faceted);
    assert_eq!(result.body.face_list().len(), 33 * 8);
    assert_volume(&mut k, &mut { result.body }, 44., 1e-6);
}

#[test]
fn uncertain_or_invalid_analytic_loops_are_not_source_refusals() {
    let first = polygon(&rectangle(0., 0., 4., 4.));
    let touching = polygon(&rectangle(0., 1., 1., 1.));
    assert!(geometry::contains_loop(&first, &touching).is_none());
    assert!(!certify_nesting(
        &mut Kernel::default(),
        &[first.clone(), touching.clone()],
        &[first.clone(), touching.clone()],
        1e-6,
        6.
    )
    .unwrap());
    assert!(nest_regions(
        &[first.clone(), touching.clone()],
        &[first.clone(), touching]
    )
    .unwrap()
    .is_none());
    let other = polygon(&rectangle(6., 0., 2., 2.));
    let crossing = polygon(&[
        Vec2::new(0., 0.),
        Vec2::new(4., 4.),
        Vec2::new(0., 4.),
        Vec2::new(4., 0.),
    ]);
    assert!(!certify_nesting(
        &mut Kernel::default(),
        &[first, other.clone()],
        &[crossing, other],
        1e-6,
        10.
    )
    .unwrap());
}

#[test]
fn uncertain_analytic_volume_keeps_cancellation_and_internal_errors_distinct() {
    let mesh = prism(&[&rectangle(0., 0., 10., 10.)], 10.);
    let mut k = Kernel::default();
    let mut body = convert(&mut k, &mesh, 0.05).unwrap();
    let region = crate::topology::brep::regions(&body, None)
        .unwrap()
        .into_iter()
        .find(|&r| Some(r) != body.infinite_region())
        .unwrap();
    body.region_mut(region).unwrap().is_void = crate::error::SM_UNSURE;
    assert!(native::volume(&mut k, &mut body).unwrap().is_none());
    assert!(!native::is_valid(&mut k, &mut body).unwrap());
    let token = crate::kernel::CancelToken::new();
    token.cancel();
    k.set_cancel_token(token);
    assert!(native::volume(&mut k, &mut body)
        .unwrap_err()
        .is_cancelled());
    assert!(native::is_valid(&mut k, &mut body)
        .unwrap_err()
        .is_cancelled());
    k.clear_cancel_token();
    let unexpected = SolidError::boolean(
        "boolean::merge::p2::make_faceuse",
        crate::error::boolean::BooleanFailure::NoRadialEdgeUse,
    );
    let error = native::certificate::<()>(Err(unexpected)).unwrap_err();
    assert!(matches!(
        error.failure().map(|failure| &failure.reason),
        Some(crate::error::Reason::Boolean(
            crate::error::boolean::BooleanFailure::NoRadialEdgeUse
        ))
    ));
}

#[test]
fn representable_large_offsets_keep_the_source_geometry() {
    let mut mesh = prism(&[&rectangle(0., 0., 10., 10.)], 10.);
    for p in &mut mesh.positions {
        p.x += 1_000_000_000.;
    }
    let mut k = Kernel::default();
    let welded = weld(&mesh.positions, &mesh.triangles, 0.05).unwrap();
    assert_eq!(welded.volume(), 1000.);
    let result = reconstruct(&mut k, &mesh.positions, &mesh.triangles, 0.05).unwrap();
    assert_eq!(result.body.face_list().len(), 6);
    assert!(result
        .body
        .vertex_list()
        .iter()
        .all(|&v| result.body.vertex(v).unwrap().point.x >= 1_000_000_000.));
}

/// Repeated F32 shoulder normals are one geometric constraint, not a new X direction
/// obtained by normalizing the small difference between two Z normals.
#[test]
fn nearparallel_shoulders_do_not_promote_normal_noise_to_a_constraint() {
    for sign in [-1., 1.] {
        let noisy = normalize(Vec3::Z * sign + Vec3::X * 1e-8).unwrap();
        for first in [Vec3::Z, -Vec3::Z] {
            let tangent = parts::shoulder_direction(Vec3::X, &[first, noisy], 10., 1e-6).unwrap();
            assert!(
                tangent.distance(Vec3::X) < 1e-14,
                "{first:?}, {noisy:?}: {tangent:?}"
            );
            let normal = parts::shoulder_direction(noisy, &[first], 10., 1e-6).unwrap();
            assert!(normal.distance(Vec3::Z * sign) < 1e-14, "{normal:?}");
            let same = parts::shoulder_direction(normal, &[first, noisy], 10., 1e-6).unwrap();
            assert!(same.distance(normal) < 1e-14);
        }
    }
    let oblique = normalize(Vec3::X + Vec3::Z).unwrap();
    assert!(
        parts::shoulder_direction(oblique, &[Vec3::Z], 10., 1e-6)
            .unwrap()
            .distance(oblique)
            < 1e-14
    );
    let normal = parts::shoulder_direction(Vec3::Z, &[Vec3::Z], 10., 1e-6).unwrap();
    assert!(normal.distance(Vec3::Z) < 1e-14);
}

/// Two independently sampled F32 shoulders use their shared cut-direction family
/// throughout recursive recovery, while all three walls remain certified cylinders.
#[test]
fn rotated_f32_parallel_shoulders_keep_their_analytic_contacts() {
    let n = 96usize;
    let mut mesh = TriMesh::default();
    for (radius, z, phase) in [
        (12., 0., 0.),
        (12., 4., 0.3),
        (10., 4., 0.7),
        (10., 7., 0.1),
        (8., 7., 0.8),
        (8., 10., 0.2),
    ] {
        for i in 0..n {
            let angle = (i as f64 + phase) * TAU / n as f64;
            mesh.positions.push(Vec3::new(
                f64::from((radius * angle.cos()) as f32),
                f64::from((radius * angle.sin()) as f32),
                z,
            ));
        }
    }
    for layer in 0..5 {
        for i in 0..n {
            let a = (layer * n + i) as u32;
            let b = (layer * n + (i + 1) % n) as u32;
            let c = ((layer + 1) * n + (i + 1) % n) as u32;
            let d = ((layer + 1) * n + i) as u32;
            mesh.triangles.extend([[a, b, c], [a, c, d]]);
        }
    }
    mesh.positions.extend([Vec3::ZERO, Vec3::new(0., 0., 10.)]);
    for i in 0..n {
        mesh.triangles
            .push([(6 * n) as u32, ((i + 1) % n) as u32, i as u32]);
        mesh.triangles.push([
            (6 * n + 1) as u32,
            (5 * n + i) as u32,
            (5 * n + (i + 1) % n) as u32,
        ]);
    }
    move_mesh(&mut mesh, 0.43, Vec3::new(13., -9., 7.), true);
    let mut k = Kernel::default();
    let result = reconstruct(&mut k, &mesh.positions, &mesh.triangles, 0.05).unwrap();
    assert_eq!(result.kind, ReconstructionKind::Analytic);
    assert_eq!(cylinders(&result.body), 3);
    assert!(result.body.face_list().len() < 24);
    assert_volume(&mut k, &mut { result.body }, 1068. * PI, 0.04);
}
