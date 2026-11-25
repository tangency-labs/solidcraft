//! tangency-labs/solidcore#1453 (the cylinder form of #1130): a cylinder r5, h10
//! shelled 1 inward with its top cap open. `MateEdges` intersects the offset faces'
//! tangent (G1R) B-spline extensions, so every new inner edge between two analytic
//! faces is marched: the floor circle and the seam line are B-splines, the rim was split
//! at an extra vertex while the marched edges' tolerances were doubled (#18162), and the
//! volume is 3.5e-7 over the closed form.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use core::f64::consts::PI;

use offset_fixture::{planar_faces_at, shell};
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::p2::{ELLIPSE_TYPE, LINE_TYPE};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{
    create_box, create_cone, create_cylinder, create_sphere,
};
use solidcore::topology::brep::p1;
use solidcore::{Brep, Kernel};

fn volume_at(k: &Kernel, b: &mut Brep, accuracy: f64) -> solidcore::Result<f64> {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, accuracy, &attrs)
}

/// The inner edges (every sample within 1e-3 of `on`, which gives a point's distance
/// from the inner surface): (count, how many are circles/ellipses or lines, worst
/// sample distance).
fn inner_edges(k: &Kernel, b: &Brep, on: impl Fn(Point3) -> f64) -> (usize, usize, f64) {
    let (mut count, mut exact, mut worst) = (0, 0, 0.0_f64);
    for e in p1::edges(b, None).unwrap() {
        let edge = b.edge(e).unwrap();
        let Some(curve) = edge.curve.as_ref() else {
            continue;
        };
        let mut dev = 0.0_f64;
        for i in 0..=32 {
            let t = edge.interval.evaluate(f64::from(i) / 32.0);
            let p = curve.evaluate_point(k, t).unwrap();
            dev = dev.max(on(p).abs());
        }
        if dev < 1e-3 {
            count += 1;
            worst = worst.max(dev);
            let kind = curve.is_kind_of(ELLIPSE_TYPE) || curve.is_kind_of(LINE_TYPE);
            eprintln!(
                "PROBE issue_1453 inner edge {e:?}: type {} dev {dev:e}",
                curve.type_code()
            );
            exact += usize::from(kind);
        }
    }
    (count, exact, worst)
}

/// The largest edge tolerance of `b`; each must be finite and nonnegative (`f64::max`
/// would skip a NaN).
fn largest_tolerance(b: &Brep) -> f64 {
    p1::edges(b, None)
        .unwrap()
        .into_iter()
        .map(|e| {
            let t = b.edge(e).unwrap().tolerance;
            assert!(t.is_finite() && t >= 0.0, "edge {e:?}: tolerance {t:e}");
            t
        })
        .fold(0.0, f64::max)
}

struct Shelled {
    edges: usize,
    vertices: usize,
    largest_tolerance: f64,
    inner: usize,
    exact: usize,
    worst: f64,
    volume: Option<f64>,
}

/// `part` shelled `distance` with the plane face at z = `open_z` open, measured (the
/// volume only when `want` is given: integrating a marched cone takes minutes).
fn shelled(
    label: &str,
    fixed: bool,
    part: impl Fn(&mut Kernel) -> Brep,
    open_z: f64,
    distance: f64,
    on: impl Fn(Point3) -> f64,
    want: Option<f64>,
) -> Shelled {
    let mut k = Kernel::default();
    k.fixes.offset_analytic_mate_edges = fixed;
    let part = part(&mut k);
    let open = planar_faces_at(&k, &part, 2, open_z);
    assert_eq!(open.len(), 1, "one open face");
    let mut b = shell(&mut k, part, distance, &open).unwrap();
    solidcore::topology::integrity::check(&b).unwrap();
    let (inner, exact, worst) = inner_edges(&k, &b, on);
    let volume = want.map(|_| volume_at(&k, &mut b, 1e-12).unwrap());
    let r = Shelled {
        edges: b.edge_list().len(),
        vertices: b.vertex_list().len(),
        largest_tolerance: largest_tolerance(&b),
        inner,
        exact,
        worst,
        volume,
    };
    eprintln!(
        "PROBE {label} fixed={fixed}: edges={} vertices={} inner={inner} exact={exact} worst={worst:e} volume={volume:?} want={want:?} largest tolerance={:e}",
        r.edges,
        r.vertices,
        r.largest_tolerance
    );
    r
}

fn cylinder(k: &mut Kernel) -> Brep {
    create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 5.0, 10.0).unwrap()
}

fn on_cylinder(p: Point3) -> f64 {
    p.x.hypot(p.y) - 4.0
}

/// The issue's case: a cylinder r5, h10 shelled 1 inward, its top open. Without the fix
/// (upstream) `MateEdges` marches the new inner edges; the seam stays marched after the
/// shell's last step, `make_edges_on_analytics_exact` (WR-545). Upstream split the rim at an extra vertex
/// (7 edges and 5 vertices) because each replaced edge's tolerance was doubled to 2e-5
/// (#18009: the edge tolerance passed as the trim's distance, which `SetUVTrimCurve`
/// doubles); with the raise at the trims' proved distance (#18162) the march closes the rim
/// as one edge: 6 edges and 4 vertices, the minimal topology of the shelled cylinder, the
/// same as the fixed path's, and no edge's tolerance is above upstream's 2e-5. Its volume
/// read 3.5e-7 over until `brep_volume` closed the marched edges' gaps (#16735): that was
/// the integrator's leak through them, and the marched solid's volume is pi 25 10 -
/// pi 16 9 to 1e-9 too (1.1e-12 measured). With the fix: the floor and rim are exact
/// circles, 6 edges and 4 vertices, and the volume is that closed form to 1e-9.
#[test]
fn issue_1453_shelled_cylinder_edges_are_exact() {
    let want = PI * 25.0 * 10.0 - PI * 16.0 * 9.0;
    let stock = shelled(
        "issue_1453 cylinder",
        false,
        cylinder,
        10.0,
        -1.0,
        on_cylinder,
        Some(want),
    );
    assert_eq!(
        (stock.edges, stock.vertices),
        (6, 4),
        "the marched rim one closed edge"
    );
    assert!(
        stock.largest_tolerance <= 2e-5,
        "no edge looser than upstream's doubled 2e-5: {}",
        stock.largest_tolerance
    );
    assert!(stock.exact < stock.inner, "a marched inner edge");
    let over = stock.volume.unwrap() - want;
    assert!(
        over.abs() <= 1e-9 * want,
        "the marched solid's volume: {over}"
    );
    let fixed = shelled(
        "issue_1453 cylinder",
        true,
        cylinder,
        10.0,
        -1.0,
        on_cylinder,
        Some(want),
    );
    assert_eq!((fixed.edges, fixed.vertices), (6, 4), "no extra rim vertex");
    assert_eq!(fixed.inner, 3, "floor circle, seam, rim circle");
    // The floor and rim circles are exact. The seam, where the inner wall meets
    // itself, has no exact intersection of two analytic copies (one cylinder twice),
    // so it stays the marched B-spline, on the line x = 4, y = 0.
    assert_eq!(fixed.exact, 2, "floor and rim are circles");
    assert!(fixed.worst <= 1e-12, "inner edges on r 4: {}", fixed.worst);
    let got = fixed.volume.unwrap();
    assert!((got - want).abs() <= 1e-9 * want, "{got} vs {want}");
}

fn frustum(k: &mut Kernel) -> Brep {
    create_cone(k, Point3::new(0.0, 0.0, 0.0), 5.0, 3.0, 10.0).unwrap()
}

/// The frustum's inner wall: the cone moved sqrt(1.04) inward, from z = 1 to 10.
fn frustum_inner_radius(z: f64) -> f64 {
    5.0 - 0.2 * z - 1.04_f64.sqrt()
}

fn on_frustum(p: Point3) -> f64 {
    p.x.hypot(p.y) - frustum_inner_radius(p.z)
}

/// A frustum r5 to r3 over h10 shelled 1 inward, its top open. `MateEdges` marches its
/// floor and rim without `offset_analytic_mate_edges`, and the shell's last step,
/// `make_edges_on_analytics_exact` (WR-545, every shell since its flag was retired), makes
/// each one the exact circle its plane meets the inner cone on (main pinned them marched,
/// 0 exact, while that step was off by default). With the fix `MateEdges` makes them
/// circles itself. Either way the seam, the inner cone against itself, stays marched.
#[test]
fn issue_1453_shelled_cone_edges_are_exact() {
    let stock = shelled(
        "issue_1453 cone",
        false,
        frustum,
        10.0,
        -1.0,
        on_frustum,
        None,
    );
    assert_eq!((stock.edges, stock.vertices), (6, 4));
    assert_eq!(stock.inner, 3, "floor circle, seam, rim circle");
    assert_eq!(
        stock.exact, 2,
        "the shell's last step makes floor and rim circles"
    );
    assert!(
        stock.worst <= 1e-12,
        "inner edges on the inner cone: {}",
        stock.worst
    );
    let fixed = shelled(
        "issue_1453 cone",
        true,
        frustum,
        10.0,
        -1.0,
        on_frustum,
        None,
    );
    assert_eq!((fixed.edges, fixed.vertices), (6, 4));
    assert_eq!(fixed.exact, 2, "floor and rim are circles");
    assert!(
        fixed.worst <= 1e-12,
        "inner edges on the inner cone: {}",
        fixed.worst
    );
}

/// The fixed frustum shell's volume is the closed form to 1e-9 (2.2e-14 measured;
/// upstream's is 1.9e-7 over). Ignored by default: integrating the cone faces takes
/// about four minutes.
#[test]
#[ignore = "slow: about four minutes of volume integration"]
fn issue_1453_shelled_cone_volume_is_exact() {
    let (r1, r2) = (frustum_inner_radius(1.0), frustum_inner_radius(10.0));
    let outer = PI * 10.0 / 3.0 * (25.0 + 15.0 + 9.0);
    let want = outer - PI * 9.0 / 3.0 * (r1 * r1 + r1 * r2 + r2 * r2);
    let fixed = shelled(
        "issue_1453 cone",
        true,
        frustum,
        10.0,
        -1.0,
        on_frustum,
        Some(want),
    );
    let got = fixed.volume.unwrap();
    assert!((got - want).abs() <= 1e-9 * want, "{got} vs {want}");
}

/// #1130's dome (a sphere r20 above z = 0) shelled 2 inward, its base open: the rim
/// between the inner sphere and the base plane is the circle r18 with or without the
/// fix (the port does not march it, plan/findings/offset.offset_geometry_creation--issue-1130-not-reproduced-in-port.md);
/// the fix keeps it so.
#[test]
fn issue_1453_shelled_dome_rim_stays_exact() {
    let dome = |k: &mut Kernel| {
        let ball = create_sphere(k, Point3::new(0.0, 0.0, 0.0), 20.0).unwrap();
        let below = create_box(k, Point3::new(-30.0, -30.0, -30.0), 60.0, 60.0, 30.0).unwrap();
        boolean_difference(k, ball, below).unwrap()
    };
    let on = |p: Point3| (p.distance(Point3::new(0.0, 0.0, 0.0)) - 18.0).abs() + p.z.abs();
    let stock = shelled("issue_1453 dome", false, dome, 0.0, -2.0, on, None);
    let fixed = shelled("issue_1453 dome", true, dome, 0.0, -2.0, on, None);
    assert!(
        fixed.inner >= 1 && fixed.exact == fixed.inner,
        "the rim a circle"
    );
    assert!(fixed.worst <= 1e-12, "rim on r 18: {}", fixed.worst);
    assert_eq!((fixed.edges, fixed.vertices), (stock.edges, stock.vertices));
}

/// WR-671 on the issue's cylinder with its B-rep tolerance set to 1e-6 under its edges'
/// and vertices' own 1e-5 (golden `api_shell_cylinder_r5_top_open_tol_1e6`). The offset
/// edges and vertices keep the cylinder's own tolerances rather than the B-rep's 1e-6,
/// so the shell is the one the cylinder at its default B-rep tolerance gives
/// ([`issue_1453_shelled_cylinder_edges_are_exact`]'s upstream shell): 7 edges and 5
/// vertices, the rim split at an extra vertex. It checks clean at level 2, no edge or
/// vertex is tighter than the cylinder's, and its volume is pi 25 10 - pi 16 9 to 1e-9.
#[test]
fn wr_671_shell_at_a_tighter_brep_tolerance_keeps_the_cylinders_tolerances() {
    use solidcore::topology::brep::validation::{check_brep, CheckLevel};
    let want = PI * 25.0 * 10.0 - PI * 16.0 * 9.0;
    let mut k = Kernel::default();
    let mut part = cylinder(&mut k);
    part.set_tolerance(1e-6);
    let source = p1::edges(&part, None)
        .unwrap()
        .into_iter()
        .map(|e| part.edge(e).unwrap().tolerance)
        .chain(
            p1::vertices(&part, None)
                .unwrap()
                .into_iter()
                .map(|v| part.vertex(v).unwrap().tolerance),
        )
        .fold(f64::INFINITY, f64::min);
    assert_eq!(source, 1e-5, "the cylinder's own tolerances");
    let open = planar_faces_at(&k, &part, 2, 10.0);
    let mut b = shell(&mut k, part, -1.0, &open).unwrap();
    assert_eq!((b.edge_list().len(), b.vertex_list().len()), (7, 5));
    for e in p1::edges(&b, None).unwrap() {
        let t = b.edge(e).unwrap().tolerance;
        assert!(t >= source, "edge {e:?} tolerance {t}");
    }
    for v in p1::vertices(&b, None).unwrap() {
        let t = b.vertex(v).unwrap().tolerance;
        assert!(t >= source, "vertex {v:?} tolerance {t}");
    }
    let c = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert_eq!(c.assert_valid, Some(true), "{:?}", c.lines());
    assert_eq!(c.failures().count(), 0, "{:?}", c.lines());
    let got = volume_at(&k, &mut b, 1e-12).unwrap();
    assert!((got - want).abs() <= 1e-9 * want, "{got} vs {want}");
}

/// The volume of a hole of radius `r` drilled along a radius of a cylinder of radius `big`
/// about Z, from `depth` off the axis out through the cylinder's side: over the hole's
/// disk, `y = r sin t`, the length from `depth` to the side `sqrt(big² - y²)`; Gauss-free
/// midpoint quadrature in `t`, whose integrand is smooth and periodic, so 4096 points give
/// it to rounding.
fn radial_hole(big: f64, r: f64, depth: f64) -> f64 {
    let n = 4096;
    let h = PI / f64::from(n);
    (0..n)
        .map(|i| {
            let t = -0.5 * PI + (f64::from(i) + 0.5) * h;
            let y = r * t.sin();
            2.0 * r * r * t.cos() * t.cos() * ((big * big - y * y).sqrt() - depth)
        })
        .sum::<f64>()
        * h
}

/// The angle of `p` about Z, in degrees in `[0, 360)`.
fn angle_deg(p: Point3) -> f64 {
    p.y.atan2(p.x).to_degrees().rem_euclid(360.0)
}

/// The seam angles (degrees) of the faces of `b` on the cylinder of radius `radius` about Z:
/// the angle of each seam edge's start vertex.
fn seam_angles(k: &Kernel, b: &mut Brep, radius: f64) -> Vec<f64> {
    use solidcore::surface::surface::{Surface, SurfaceKind};
    use solidcore::topology::edge;
    let mut out = Vec::new();
    for f in b.face_list().to_vec() {
        let on = matches!(
            b.face(f).unwrap().surface.as_ref().map(Surface::kind),
            Some(SurfaceKind::Cone(c)) if c.is_cylinder() && (c.radius(0.0) - radius).abs() < 1e-9
        );
        if !on {
            continue;
        }
        for e in solidcore::topology::face::p4::edges(b, f, None, false, None).unwrap() {
            if edge::faces(b, e, None).unwrap().as_slice() == [f] && edge::is_seam(k, b, e).unwrap()
            {
                let v = edge::start_end_vertices(b, e).unwrap().0.unwrap();
                out.push(angle_deg(b.vertex(v).unwrap().point));
            }
        }
    }
    out
}

/// solidcore#3353 (acid-8 stage 5): a Ø100 × 120 drum with one Ø6 radial blind hole 20 deep
/// at Z = 20, its axis 5° round from the drum's seam, closed-shelled 2 inward. The hole's
/// rim covers 3.44° either side of its axis, clear of the seam; inset by 2 the hole is r 5 in
/// a drum of r 48, so its offset covers asin(5 / 48) = 5.98° either side and crosses the seam
/// its rim did not reach. The offset faces keep the input's loops, so that offset rim was
/// refused ("an offset edge could not be split where a loop gap or a step corner needs it").
/// The seam is the cylinder's parameterisation only: turned clear of the hole for the shell,
/// the shell builds, and the drum's outside gets its seam back where it was.
///
/// Closed form: the drum less its hole, less the cavity, the drum r 48 from Z = 2 to 118 less
/// the hole's offset (r 5, from 28 off the axis out through the cavity's side), the holes'
/// volumes by [`radial_hole`]: 103844.0029879396.
#[test]
fn a_closed_shell_whose_hole_offset_crosses_the_seam_builds() {
    use solidcore::offset::offset_executive::p2::{
        shell_brep_simple_reporting, OffsetOptions, ShellOptions,
    };
    use solidcore::topology::brep::validation::{check_brep, CheckLevel};
    use solidcore::topology::proto_topology::p1::api_transform::{api_rotate, TransformObject};
    let mut k = Kernel::default();
    let drum = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 50.0, 120.0).unwrap();
    let mut probe = drum.clone_standing_for_self().unwrap();
    let seam = seam_angles(&k, &mut probe, 50.0);
    assert_eq!(seam.len(), 1, "one seam on the drum's side: {seam:?}");
    // the drill along +X from 30 off the axis out past the side, turned 5° past the seam
    let mut drill = create_cylinder(&mut k, Point3::new(30.0, 0.0, 20.0), 3.0, 25.0).unwrap();
    let z = solidcore::math::Vec3::new(0.0, 0.0, 1.0);
    let y = solidcore::math::Vec3::new(0.0, 1.0, 0.0);
    api_rotate(
        &mut k,
        Some(TransformObject::Brep(&mut drill)),
        Point3::new(30.0, 0.0, 20.0),
        y,
        90.0,
    )
    .unwrap();
    api_rotate(
        &mut k,
        Some(TransformObject::Brep(&mut drill)),
        Point3::new(0.0, 0.0, 0.0),
        z,
        seam[0] + 5.0,
    )
    .unwrap();
    let mut part = boolean_difference(&mut k, drum, drill).unwrap();
    part.refresh_cached_topology();
    let options = ShellOptions {
        offset: OffsetOptions {
            distance: -2.0,
            extend_convex_edges: true,
            merge_results: true,
        },
        create_offset_solid: true,
        merge_sheet_side_walls: true,
    };
    let made = shell_brep_simple_reporting(&mut k, &part, options, &[]).unwrap();
    // every seam turned and turned back: no turn failed
    assert!(
        made.seams.not_cleared.is_empty() && made.seams.not_restored.is_empty(),
        "{:?}",
        made.seams
    );
    let mut b = made.shell;
    let c = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert!(c.is_valid(), "{:?}", c.lines());
    // the drum's two sides, its two caps, the hole's and its offset's walls and ends
    assert_eq!(b.face_list().len(), 10);
    let want = PI * 50.0 * 50.0 * 120.0
        - radial_hole(50.0, 3.0, 30.0)
        - (PI * 48.0 * 48.0 * 116.0 - radial_hole(48.0, 5.0, 28.0));
    assert!((want - 103_844.002_987_939_6).abs() < 1e-6, "{want}");
    let got = volume_at(&k, &mut b, 1e-9).unwrap();
    assert!((got - want).abs() <= 1e-7 * want, "{got} vs {want}");
    // the outside's seam is the input's
    let outside = seam_angles(&k, &mut b, 50.0);
    assert_eq!(outside.len(), 1, "{outside:?}");
    let off = (outside[0] - seam[0]).rem_euclid(360.0);
    assert!(off.min(360.0 - off) < 1e-9, "{outside:?} vs {seam:?}");
}
