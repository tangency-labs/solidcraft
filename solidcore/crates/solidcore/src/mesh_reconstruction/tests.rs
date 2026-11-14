//! Closed mesh fixtures use explicit input triangles, independent of a display tessellator.

use std::f64::consts::{PI, TAU};

use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::face;

use super::*;

fn convert(k: &mut Kernel, mesh: &TriMesh, tolerance: f64) -> Result<Option<Brep>> {
    reconstruct(k, &mesh.positions, &mesh.triangles, tolerance)
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
    let volume = native::volume(k, body).unwrap();
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
    let mut body = convert(&mut k, &mesh, 0.05).unwrap().unwrap();
    assert_volume(&mut k, &mut body, (1000. - PI * 16.) * 15., 1e-6);
    assert_eq!(cylinders(&body), 1);
    assert!(body.face_list().len() < 15);
}

#[test]
fn planar_boxes_are_exact_and_coarse_polygons_are_not_rounded() {
    let mesh = prism(&[&rectangle(0., 0., 10., 20.)], 30.);
    let mut k = Kernel::default();
    let mut body = convert(&mut k, &mesh, 0.01).unwrap().unwrap();
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
    let mut body = convert(&mut k, &mesh, 0.01).unwrap().unwrap();
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
    assert!(certify_nesting(
        &mut Kernel::default(),
        &[outer.clone(), inner],
        &[outer, fitted],
        1e-6,
        3.
    )
    .is_err());
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
fn many_holes_are_refused_promptly_instead_of_stalling() {
    let mesh = perforated_plate(20, 20);
    let start = std::time::Instant::now();
    let mut k = Kernel::default();
    let error = convert(&mut k, &mesh, 0.05).unwrap_err().to_string();
    assert!(error.contains("too complex to convert"), "{error}");
    assert!(
        start.elapsed() < std::time::Duration::from_secs(10),
        "took {:?}",
        start.elapsed()
    );
    let mesh = perforated_plate(7, 7);
    let mut body = convert(&mut k, &mesh, 0.05).unwrap().unwrap();
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
    let mut body = convert(&mut k, &mesh, 0.05).unwrap().unwrap();
    assert_eq!(cylinders(&body), 1, "{:?}", surface_kinds(&body));
    assert!(body.face_list().len() < 12, "{}", body.face_list().len());
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
        let mut body = convert(&mut k, &mesh, 0.05).unwrap().unwrap();
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
    let mut body = convert(&mut k, &mesh, 0.05).unwrap().unwrap();
    assert!(body.face_list().len() < 16, "{}", body.face_list().len());
    assert_eq!(cylinders(&body), 2);
    assert_volume(&mut k, &mut body, 896. * PI, 0.03);
}

#[test]
fn rotated_f32_stepped_cylinder_preserves_the_analytic_shape() {
    let mut mesh = phase_shifted_stepped_cylinder();
    move_mesh(&mut mesh, 0.43, Vec3::new(13., -9., 7.), true);
    let mut k = Kernel::default();
    let mut body = convert(&mut k, &mesh, 0.05).unwrap().unwrap();
    assert!(body.face_list().len() < 16);
    assert_volume(&mut k, &mut body, 896. * PI, 0.03);
}

#[test]
fn radial_damage_outside_tolerance_is_not_fitted() {
    let mut mesh = phase_shifted_stepped_cylinder();
    mesh.positions[0].x += 0.4;
    assert!(!matches!(
        convert(&mut Kernel::default(), &mesh, 0.05),
        Ok(Some(_))
    ));
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
