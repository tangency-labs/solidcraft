//! tangency-labs/solidcore#1477 (WR-565): `SmPrimitiveCreation::CreateLinearSweep` makes
//! an `SmCylinder` only of a whole circle swept along its axis. A circular arc short of a
//! whole turn becomes an `SmSurfOfExtrusion`, which exports as a NURBS, so a sheet-metal
//! convert finds no cylinder at a modelled bend. The opt-in
//! `Kernel::fixes.linear_sweep_arc_is_cylinder` (SolidCraft change, tangency-labs/smlib
//! 12918e9) makes that arc's wall a cylinder: its frame turned to the arc's start (for a
//! sweep against the arc's normal, to its end and then flipped), bounded 0..sweep angle by
//! 0..distance. A sweep that is not square to the arc's plane stays a surface of
//! extrusion, and a whole circle keeps upstream's cylinder.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::curve::line::Line;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point2;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, NurbCircleParam};
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::math::CPoint;
use solidcore::surface::cone::Cone;
use solidcore::surface::implicit::AnalyticSupport;
use solidcore::surface::surface::SurfaceKind;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::{Brep, Kernel, Vec3};

fn centre() -> Vec3 {
    Vec3::new(4.0, 4.0, 0.0)
}

/// The issue's frame at (4, 4, 0): X along -x and Y along -y, so Z is +z and the circle's
/// angle 0 is at (0, 4) and angle 90 at (4, 0).
fn issue_frame() -> Axis2Placement {
    Axis2Placement::new(
        centre(),
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::new(0.0, -1.0, 0.0),
    )
}

fn seg(a: Vec3, b: Vec3) -> Curve {
    Curve::from(Line::create_line_segment(3, a, b, None).unwrap())
}

fn circle(k: &Kernel, frame: &Axis2Placement, r: f64, start: f64, end: f64) -> Curve {
    let interval = Extent1d::from_min_max(start, end);
    Curve::from(Ellipse::create_canonical_circle(k, frame, r, Some(&interval), None).unwrap())
}

/// The surface kinds of the B-rep's faces, in face order.
fn kinds(b: &Brep) -> Vec<&'static str> {
    b.face_list()
        .iter()
        .map(
            |&f| match b.face(f).unwrap().surface.as_ref().unwrap().kind() {
                SurfaceKind::Cone(c) if c.is_cylinder_class() => "cylinder",
                SurfaceKind::Plane(_) => "plane",
                SurfaceKind::SurfOfExtrusion(_) => "extrusion",
                _ => "other",
            },
        )
        .collect()
}

/// The B-rep's cones and cylinders, in face order.
fn cylinders(b: &Brep) -> Vec<Cone> {
    b.face_list()
        .iter()
        .filter_map(
            |&f| match b.face(f).unwrap().surface.as_ref().unwrap().kind() {
                SurfaceKind::Cone(c) => Some((**c).clone()),
                _ => None,
            },
        )
        .collect()
}

/// Asserts a closed manifold solid of volume `want`.
fn assert_solid(k: &mut Kernel, b: &mut Brep, want: f64, case: &str) {
    assert!(
        is_manifold_solid(b).unwrap(),
        "{case}: not a manifold solid"
    );
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let got = brep_volume(k, b, 1e-6, &attrs).unwrap();
    assert!(
        (got - want).abs() <= 1e-6 * want,
        "{case}: volume {got}, want {want}"
    );
}

/// Asserts that `c` is the cylinder of radius `r` about the line through (4, 4, 0) along
/// `axis`, with STEP domain 0..`deg` degrees by 0..`height`, whose points at STEP (u, v) =
/// (0, 0), (`deg`, 0) and (0, `height`) are `corners`, and whose NURB (the face's
/// domain) spans exactly those corners and the fourth. The B-rep's orientation may have
/// swapped the NURB's u and v to turn the face's normal, so its corners are matched in
/// any order.
fn assert_cylinder(
    k: &Kernel,
    c: &Cone,
    r: f64,
    axis: Vec3,
    (deg, height): (f64, f64),
    corners: [Vec3; 3],
    case: &str,
) {
    assert!(c.is_cylinder_class(), "{case}: not a cylinder");
    assert!(
        (c.base_radius() - r).abs() < 1e-9,
        "{case}: radius {}",
        c.base_radius()
    );
    let frame = c.canonical().origin;
    assert!(
        (frame.origin() - centre()).length() < 1e-9,
        "{case}: origin {:?}",
        frame.origin()
    );
    assert!(
        (frame.z_axis() - axis).length() < 1e-12,
        "{case}: axis {:?}",
        frame.z_axis()
    );
    let step = c.rev().step_uv_domain();
    let (u, v) = (step.u_interval(), step.v_interval());
    assert!(
        u.min().abs() < 1e-12 && (u.max() - deg).abs() < 1e-12,
        "{case}: u {} .. {}",
        u.min(),
        u.max()
    );
    assert!(
        v.min().abs() < 1e-12 && (v.max() - height).abs() < 1e-12,
        "{case}: v {} .. {}",
        v.min(),
        v.max()
    );
    // STEP parameters (degrees, height): u 0 is where the frame was turned to
    for ((s, t), want) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)]
        .into_iter()
        .zip(corners)
    {
        let p = c
            .evaluate_step(Point2::new(s * deg, t * height), 0, 0)
            .unwrap()[0];
        assert!(
            (p - want).length() < 1e-9,
            "{case}: STEP corner ({s}, {t}) {p:?}, want {want:?}"
        );
    }
    let [c0, c1, c2] = corners;
    let mut expected = vec![c0, c1, c2, c1 + (c2 - c0)];
    let domain = c.rev().base().natural_uv_domain();
    for (s, t) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
        let p = c.evaluate_point(k, domain.evaluate(s, t)).unwrap();
        let Some(i) = expected.iter().position(|&q| (p - q).length() < 1e-9) else {
            panic!("{case}: NURB corner ({s}, {t}) {p:?} is not one of {expected:?}");
        };
        expected.swap_remove(i);
    }
}

/// The issue's repro: the quarter disc of radius 4 about (4, 4): the arc `arc` from
/// (0, 4) to (4, 0) and the two radii, swept `dist` along `dir`.
fn quarter_disc(k: &mut Kernel, arc: &Curve, dir: Vec3, dist: f64) -> Brep {
    let l1 = seg(Vec3::new(4.0, 0.0, 0.0), centre());
    let l2 = seg(centre(), Vec3::new(0.0, 4.0, 0.0));
    create_linear_sweep(k, &[Some(arc), Some(&l1), Some(&l2)], dir, dist, true).unwrap()
}

/// 50 pi 4^2 / 4
const QUARTER_DISC_VOLUME: f64 = 200.0 * std::f64::consts::PI;

#[test]
fn issue_1477_an_arc_extruded_square_to_its_plane_is_a_cylinder() {
    let mut k = Kernel::default();
    let arc = circle(&k, &issue_frame(), 4.0, 0.0, 90.0);

    // the fix off: upstream's surface of extrusion, unchanged
    let mut b = quarter_disc(&mut k, &arc, Vec3::Z, 50.0);
    assert_eq!(kinds(&b), ["extrusion", "plane", "plane", "plane", "plane"]);
    assert_solid(&mut k, &mut b, QUARTER_DISC_VOLUME, "fix off");

    // the fix on: a cylinder of radius 4 about x = 4, y = 4, domain 0..90 by 0..50, its
    // u from the arc's start at (0, 4) to its end at (4, 0)
    k.fixes.linear_sweep_arc_is_cylinder = true;
    let mut b = quarter_disc(&mut k, &arc, Vec3::Z, 50.0);
    assert_eq!(kinds(&b), ["cylinder", "plane", "plane", "plane", "plane"]);
    assert_solid(&mut k, &mut b, QUARTER_DISC_VOLUME, "fix on");
    let cylinders = cylinders(&b);
    assert_eq!(cylinders.len(), 1);
    assert_cylinder(
        &k,
        &cylinders[0],
        4.0,
        Vec3::Z,
        (90.0, 50.0),
        [
            Vec3::new(0.0, 4.0, 0.0),
            Vec3::new(4.0, 0.0, 0.0),
            Vec3::new(0.0, 4.0, 50.0),
        ],
        "fix on",
    );
    // the bend point of the issue's convert lies on it, at 45 degrees and half height
    let mid = cylinders[0]
        .evaluate_step(Point2::new(45.0, 25.0), 0, 0)
        .unwrap()[0];
    let half = 4.0 - 4.0 * std::f64::consts::FRAC_1_SQRT_2;
    assert!(
        (mid - Vec3::new(half, half, 25.0)).length() < 1e-9,
        "{mid:?}"
    );
}

#[test]
fn issue_1477_a_sweep_against_the_arc_normal_turns_the_frame_to_the_arc_end() {
    // along -z, given either way: the frame turned to the arc's end at (4, 0) and
    // flipped, so u runs back to the start at (0, 4) and v down
    for (dir, dist) in [(-Vec3::Z, 50.0), (Vec3::Z, -50.0)] {
        let case = format!("{dir:?} {dist}");
        let mut k = Kernel::default();
        k.fixes.linear_sweep_arc_is_cylinder = true;
        let arc = circle(&k, &issue_frame(), 4.0, 0.0, 90.0);
        let mut b = quarter_disc(&mut k, &arc, dir, dist);
        assert_eq!(
            kinds(&b),
            ["cylinder", "plane", "plane", "plane", "plane"],
            "{case}"
        );
        assert_solid(&mut k, &mut b, QUARTER_DISC_VOLUME, &case);
        assert_cylinder(
            &k,
            &cylinders(&b)[0],
            4.0,
            -Vec3::Z,
            (90.0, 50.0),
            [
                Vec3::new(4.0, 0.0, 0.0),
                Vec3::new(0.0, 4.0, 0.0),
                Vec3::new(4.0, 0.0, -50.0),
            ],
            &case,
        );
    }
}

#[test]
fn issue_1477_the_frame_is_turned_to_the_arc_start() {
    // the same arc as circles whose angle 0 is not the arc's start: over 180..270
    // degrees of the XY frame, and over 90..180 of a frame with X along +y, whose turn
    // a wrong sense would put at (8, 4); and as a NURBS arc (`IsArc` of the base class)
    let k = Kernel::default();
    let frame = Axis2Placement::new(centre(), Vec3::X, Vec3::Y);
    let quarter_turned = Axis2Placement::new(centre(), Vec3::Y, -Vec3::X);
    let arcs = [
        ("circle 180..270", circle(&k, &frame, 4.0, 180.0, 270.0)),
        (
            "circle 90..180",
            circle(&k, &quarter_turned, 4.0, 90.0, 180.0),
        ),
        (
            "nurbs",
            Curve::from(
                BSplineCurve::create_circle_segment(
                    3,
                    &frame,
                    4.0,
                    180.0,
                    270.0,
                    NurbCircleParam::Quadratic,
                )
                .unwrap(),
            ),
        ),
    ];
    for (case, arc) in &arcs {
        let mut k = Kernel::default();
        let off = kinds(&quarter_disc(&mut k, arc, Vec3::Z, 50.0));
        assert_eq!(
            off,
            ["extrusion", "plane", "plane", "plane", "plane"],
            "{case}"
        );
        k.fixes.linear_sweep_arc_is_cylinder = true;
        let mut b = quarter_disc(&mut k, arc, Vec3::Z, 50.0);
        assert_eq!(
            kinds(&b),
            ["cylinder", "plane", "plane", "plane", "plane"],
            "{case}"
        );
        assert_solid(&mut k, &mut b, QUARTER_DISC_VOLUME, case);
        assert_cylinder(
            &k,
            &cylinders(&b)[0],
            4.0,
            Vec3::Z,
            (90.0, 50.0),
            [
                Vec3::new(0.0, 4.0, 0.0),
                Vec3::new(4.0, 0.0, 0.0),
                Vec3::new(0.0, 4.0, 50.0),
            ],
            case,
        );
    }
}

#[test]
fn issue_1477_an_extruded_l_with_a_modelled_bend_has_two_cylinders() {
    // an L section bent about (4, 4), outer radius 4, inner radius 2, legs 10 long past
    // the bend, swept 50 along +z: the extruded L of the issue (10 faces, 24 edges, 16
    // vertices on both kernels)
    let p = |x: f64, y: f64| Vec3::new(x, y, 0.0);
    for fix in [false, true] {
        let mut k = Kernel::default();
        k.fixes.linear_sweep_arc_is_cylinder = fix;
        let curves = [
            circle(&k, &issue_frame(), 4.0, 0.0, 90.0),
            seg(p(4.0, 0.0), p(14.0, 0.0)),
            seg(p(14.0, 0.0), p(14.0, 2.0)),
            seg(p(14.0, 2.0), p(4.0, 2.0)),
            circle(&k, &issue_frame(), 2.0, 0.0, 90.0),
            seg(p(2.0, 4.0), p(2.0, 14.0)),
            seg(p(2.0, 14.0), p(0.0, 14.0)),
            seg(p(0.0, 14.0), p(0.0, 4.0)),
        ];
        let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
        let mut b = create_linear_sweep(&mut k, &refs, Vec3::Z, 50.0, true).unwrap();
        let bend = if fix { "cylinder" } else { "extrusion" };
        assert_eq!(
            kinds(&b),
            [bend, "plane", "plane", "plane", bend, "plane", "plane", "plane", "plane", "plane"],
            "fix {fix}"
        );
        assert_eq!(p1::faces(&b, None).unwrap().len(), 10, "fix {fix}");
        assert_eq!(p1::edges(&b, None).unwrap().len(), 24, "fix {fix}");
        assert_eq!(p1::vertices(&b, None).unwrap().len(), 16, "fix {fix}");
        // 50 (2 legs of 2 x 10 and a quarter annulus of radii 2 and 4)
        let want = 50.0 * (40.0 + 3.0 * std::f64::consts::PI);
        assert_solid(&mut k, &mut b, want, &format!("fix {fix}"));
        if fix {
            let cylinders = cylinders(&b);
            assert_eq!(cylinders.len(), 2);
            for (c, r) in cylinders.iter().zip([4.0, 2.0]) {
                assert_cylinder(
                    &k,
                    c,
                    r,
                    Vec3::Z,
                    (90.0, 50.0),
                    [
                        Vec3::new(4.0 - r, 4.0, 0.0),
                        Vec3::new(4.0, 4.0 - r, 0.0),
                        Vec3::new(4.0 - r, 4.0, 50.0),
                    ],
                    &format!("radius {r}"),
                );
            }
        }
    }
}

#[test]
fn issue_1477_a_sweep_not_square_to_the_arc_stays_a_surface_of_extrusion() {
    let dir = Vec3::new(1.0, 0.0, 1.0);
    let mut off = Kernel::default();
    let arc = circle(&off, &issue_frame(), 4.0, 0.0, 90.0);
    let upstream = kinds(&quarter_disc(&mut off, &arc, dir, 50.0));
    let mut on = Kernel::default();
    on.fixes.linear_sweep_arc_is_cylinder = true;
    let fixed = kinds(&quarter_disc(&mut on, &arc, dir, 50.0));
    assert_eq!(fixed, upstream);
    assert_eq!(fixed[0], "extrusion");
    assert!(!fixed.contains(&"cylinder"), "{fixed:?}");
}

#[test]
fn issue_1477_a_whole_circle_keeps_upstream_cylinder() {
    for fix in [false, true] {
        let mut k = Kernel::default();
        k.fixes.linear_sweep_arc_is_cylinder = fix;
        let disc = circle(&k, &issue_frame(), 4.0, 0.0, 360.0);
        let mut b = create_linear_sweep(&mut k, &[Some(&disc)], Vec3::Z, 50.0, true).unwrap();
        assert_eq!(kinds(&b), ["cylinder", "plane", "plane"], "fix {fix}");
        let want = 50.0 * 16.0 * std::f64::consts::PI;
        assert_solid(&mut k, &mut b, want, &format!("fix {fix}"));
        let step = cylinders(&b)[0].rev().step_uv_domain();
        assert!(
            step.u_interval().min().abs() < 1e-12
                && (step.u_interval().max() - 360.0).abs() < 1e-12,
            "fix {fix}: u {} .. {}",
            step.u_interval().min(),
            step.u_interval().max()
        );
    }
}

/// The placement and radius of the B-rep's only cylinder.
fn the_cylinder(b: &Brep) -> (Axis2Placement, f64) {
    let found: Vec<_> = b
        .face_list()
        .iter()
        .filter_map(
            |&f| match AnalyticSupport::of(b.face(f).unwrap().surface.as_ref().unwrap()) {
                Some(AnalyticSupport::Cylinder { placement, radius }) => Some((placement, radius)),
                _ => None,
            },
        )
        .collect();
    let [one] = found.as_slice() else {
        panic!("one cylinder: {:?}", kinds(b));
    };
    *one
}

/// Review r2 of SolidCraft #17462: a short arc that really lies in a plane tilted by `tilt`
/// about its chord stays within the arc zero of the plane square to the sweep, but the
/// circle squared to the sweep keeps the fit's radius about the projected centre and so
/// misses it by about `r tilt² / 2` across: 4.4e-5 for r 88, 4.4e-3 (and no manifold
/// solid) at a tilt of 1e-2. A short `distance` lets the caller's `distance * tilt` check
/// pass. These keep the fit's frame, as upstream: the profile is on the cylinder's base
/// edge.
#[test]
fn issue_17462_a_short_arc_tilted_about_its_chord_stays_on_its_cylinder() {
    for (r, span, tilt, distance) in [
        (88.0, 0.005, 1e-3_f64, 0.005),
        (1000.0, 0.05, 2e-4, 0.04),
        (88.0, 0.005, 1e-2, 0.0005),
        (50.0, 10.0, 2e-7, 5.0),
    ] {
        let case = format!("r {r} span {span} tilt {tilt:e} distance {distance}");
        let mut k = Kernel::default();
        k.fixes.linear_sweep_arc_is_cylinder = true;
        let (s, c) = (tilt.sin(), tilt.cos());
        let frame = Axis2Placement::new(Vec3::ZERO, Vec3::new(c, 0.0, -s), Vec3::Y);
        let arc = circle(&k, &frame, r, 0.0, span);
        let ends = arc.ends(&k).unwrap();
        let chord = seg(ends.end, ends.start);
        let b = create_linear_sweep(&mut k, &[Some(&arc), Some(&chord)], Vec3::Z, distance, true)
            .unwrap();
        assert!(
            is_manifold_solid(&b).unwrap(),
            "{case}: not a manifold solid"
        );
        let (placement, radius) = the_cylinder(&b);
        let axis = placement.z_axis();
        for i in 0..=8 {
            let u = (span * f64::from(i) / 8.0).to_radians();
            let p = frame.x_axis() * (r * u.cos()) + frame.y_axis() * (r * u.sin());
            let off = (p - placement.origin()).cross(axis).length() - radius;
            assert!(
                off.abs() < 1e-9,
                "{case}: {p:?} is {off:e} off the cylinder"
            );
            let along = (p - placement.origin()).dot(axis);
            assert!(
                along.abs() < 1e-9,
                "{case}: {p:?} is {along:e} off the base edge's plane"
            );
        }
    }
}

/// The cubic Bezier of a 0.005 degree arc of radius 88.3 about (-8.19, 145.98) at z = -53,
/// its two inner control points an ulp or two off that plane, as a profile placed there by
/// a transform holds them (SolidCraft #17462: 0.008 mm of a spline).
fn short_arc() -> Curve {
    let (cx, cy, r) = (-8.185_840_780_2, 145.979_194_294, 88.295_980_686);
    let (a0, sweep) = (-1.4235_f64, 0.005_f64.to_radians());
    let on = |a: f64| (cx + r * a.cos(), cy + r * a.sin());
    let tangent = |a: f64| (-a.sin(), a.cos());
    let h = 4.0 / 3.0 * (sweep / 4.0).tan() * r;
    let (p0, p3) = (on(a0), on(a0 + sweep));
    let (t0, t3) = (tangent(a0), tangent(a0 + sweep));
    let points = [
        (p0.0, p0.1, -53.0),
        (p0.0 + h * t0.0, p0.1 + h * t0.1, -53.000_000_000_000_01),
        (p3.0 - h * t3.0, p3.1 - h * t3.1, -52.999_999_999_999_99),
        (p3.0, p3.1, -53.0),
    ]
    .into_iter()
    .map(|(x, y, z)| CPoint::from_point(Vec3::new(x, y, z)))
    .collect();
    let nurb = NurbCurve::new(points, 3, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]).unwrap();
    Curve::from(BSplineCurve::from_nurb(nurb, 3, false))
}

/// SolidCraft #17462: a very short arc fitted from a planar curve has an ill-conditioned
/// frame, its normal 5e-8 rad off the plane's. The sweep accepted it (53 mm times that is
/// under the B-rep tolerance) and built the cylinder on that normal, so the wall leaned
/// across the sweep, its far edge 2.6e-6 mm off the swept curve. A profile whose control
/// points lie in a plane square to the sweep now gets a cylinder about the sweep direction,
/// with the profile on its base edge.
#[test]
fn issue_17462_a_very_short_arc_swept_square_is_a_cylinder_about_the_sweep_direction() {
    let mut k = Kernel::default();
    k.fixes.linear_sweep_arc_is_cylinder = true;
    let arc = short_arc();
    // The case needs the fit's normal off the sweep: it is what the sweep corrects.
    // (the sweep's zero, `1000 SM_EFF_ZERO (1 + the ends' largest coordinate)`)
    let (frame, ..) = arc
        .is_arc(&k, 8, 1e-9 * (1.0 + 146.0))
        .unwrap()
        .expect("an arc");
    let normal = frame.z_axis();
    assert!(
        normal.x != 0.0 || normal.y != 0.0,
        "the fitted normal is exact: {normal:?}"
    );
    assert!(normal.x.hypot(normal.y) < 1e-6, "{normal:?}");

    let ends = arc.ends(&k).unwrap();
    let chord = seg(ends.end, ends.start);
    let b = create_linear_sweep(
        &mut k,
        &[Some(&arc), Some(&chord)],
        Vec3::new(0.0, 0.0, 1.0),
        53.0,
        true,
    )
    .unwrap();
    assert!(is_manifold_solid(&b).unwrap(), "not a manifold solid");
    let (placement, radius) = the_cylinder(&b);
    let axis = placement.z_axis();
    assert!(
        axis.x == 0.0 && axis.y == 0.0 && axis.z.abs() == 1.0,
        "{axis:?}"
    );
    // The profile is the cylinder's base edge and its translate the far edge: on the
    // cylinder radially, and axially at 0 and 53 along it.
    for p in [ends.start, ends.end] {
        for (p, v) in [(p, 0.0), (p + Vec3::new(0.0, 0.0, 53.0), 53.0)] {
            let off = (p - placement.origin()).cross(axis).length() - radius;
            assert!(off.abs() < 1e-9, "{p:?} is {off:e} off the cylinder");
            let along = (p - placement.origin()).dot(axis).abs() - v;
            assert!(
                along.abs() < 1e-9,
                "{p:?} is {along:e} off the edge's plane"
            );
        }
    }
}

/// Review of SolidCraft #17462: a profile that really leans across the sweep, by more than
/// the arc zero over the curve (up to `2 r` times the tilt), keeps the fit's frame: squaring
/// it moved the cylinder off its own profile, 1e-2 mm off for a whole circle of radius 50
/// swept 0.01 at a tilt of 1e-4, and the body was not manifold. The circle lies in z = 0
/// and the sweep leans by `tilt` towards +x; `distance * tilt` is within the tolerance, so
/// the wall is a cylinder, which must keep the profile on its base edge.
#[test]
fn issue_17462_a_profile_tilted_across_the_sweep_stays_on_its_cylinder() {
    for (whole, tilt, distance, r) in [
        (true, 1e-4, 0.01, 50.0),
        (false, 1e-4, 0.01, 50.0),
        (true, 1e-7, 5.0, 100.0),
        (false, 1e-7, 5.0, 100.0),
    ] {
        let case = format!("whole {whole} tilt {tilt:e} distance {distance} r {r}");
        let mut k = Kernel::default();
        k.fixes.linear_sweep_arc_is_cylinder = true;
        let frame = Axis2Placement::new(Vec3::ZERO, Vec3::X, Vec3::Y);
        let end = if whole { 360.0 } else { 180.0 };
        let arc = circle(&k, &frame, r, 0.0, end);
        let mut profile = vec![arc];
        if !whole {
            profile.push(seg(Vec3::new(-r, 0.0, 0.0), Vec3::new(r, 0.0, 0.0)));
        }
        let refs: Vec<Option<&Curve>> = profile.iter().map(Some).collect();
        let b =
            create_linear_sweep(&mut k, &refs, Vec3::new(tilt, 0.0, 1.0), distance, true).unwrap();
        assert!(
            is_manifold_solid(&b).unwrap(),
            "{case}: not a manifold solid"
        );
        let (placement, radius) = the_cylinder(&b);
        let axis = placement.z_axis();
        for i in 0..=36 {
            let a = (end * f64::from(i) / 36.0).to_radians();
            let p = Vec3::new(r * a.cos(), r * a.sin(), 0.0);
            let along = (p - placement.origin()).dot(axis);
            assert!(
                along.abs() < 1e-9,
                "{case}: {p:?} is {along:e} off the base edge's plane"
            );
            let off = (p - placement.origin()).cross(axis).length() - radius;
            assert!(
                off.abs() < 1e-9,
                "{case}: {p:?} is {off:e} off the cylinder"
            );
        }
    }
}
