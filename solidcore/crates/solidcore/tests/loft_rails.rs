//! tangency-labs/solidcore#3140: the loft guided by rails, its guided edges certified.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::curve::curve::Curve;
use solidcore::error::sweep::SweepFailure;
use solidcore::error::{Reason, SolidError};
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use solidcore::query::api_queries::surface_closest_point;
use solidcore::sweep::loft_rails::{loft_rails, RailLoftOptions, RailSection, RailedLoft};
use solidcore::sweep::stations::RailFit;
use solidcore::Kernel;

/// A B-spline curve of `degree` through the control points `points` with the full knots
/// `knots`.
fn spline(degree: usize, points: &[Point3], knots: &[f64]) -> Curve {
    let mut values: Vec<f64> = Vec::new();
    let mut mults: Vec<usize> = Vec::new();
    for &u in knots {
        if values.last() == Some(&u) {
            *mults.last_mut().unwrap() += 1;
        } else {
            values.push(u);
            mults.push(1);
        }
    }
    Curve::from(
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree,
            control_points: points,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &mults,
            knots: &values,
            knot_type: KnotType::Unspecified,
            weights: None,
            trim: None,
        })
        .unwrap(),
    )
}

/// A line as a degree-1 B-spline.
fn line(a: Point3, b: Point3) -> Curve {
    spline(1, &[a, b], &[0.0, 0.0, 1.0, 1.0])
}

/// The natural cubic through `points` at chord-length parameters (SolidCraft's sketch
/// spline, `fit_spans`), as its Bézier pieces: each piece's four control points.
fn natural_pieces(points: &[Point3]) -> Vec<[Point3; 4]> {
    let n = points.len() - 1;
    let h: Vec<f64> = points.windows(2).map(|w| w[0].distance(w[1])).collect();
    // second derivatives M (natural: M_0 = M_n = 0), by the tridiagonal system
    let mut m = vec![Vec3::ZERO; n + 1];
    if n >= 2 {
        let size = n - 1;
        let mut diag = vec![0.0; size];
        let mut rhs = vec![Vec3::ZERO; size];
        for i in 1..n {
            diag[i - 1] = 2.0 * (h[i - 1] + h[i]);
            rhs[i - 1] = ((points[i + 1] - points[i]) * (1.0 / h[i])
                - (points[i] - points[i - 1]) * (1.0 / h[i - 1]))
                * 6.0;
        }
        // forward sweep (sub- and super-diagonals h[i])
        for i in 1..size {
            let w = h[i] / diag[i - 1];
            diag[i] -= w * h[i];
            rhs[i] = rhs[i] - rhs[i - 1] * w;
        }
        let mut x = vec![Vec3::ZERO; size];
        x[size - 1] = rhs[size - 1] * (1.0 / diag[size - 1]);
        for i in (0..size - 1).rev() {
            x[i] = (rhs[i] - x[i + 1] * h[i + 1]) * (1.0 / diag[i]);
        }
        m[1..n].copy_from_slice(&x[..n - 1]);
    }
    (0..n)
        .map(|i| {
            let (p0, p3, hi) = (points[i], points[i + 1], h[i]);
            let chord = (p3 - p0) * (1.0 / hi);
            let d0 = chord - (m[i] * 2.0 + m[i + 1]) * (hi / 6.0);
            let d1 = chord + (m[i] + m[i + 1] * 2.0) * (hi / 6.0);
            [p0, p0 + d0 * (hi / 3.0), p3 - d1 * (hi / 3.0), p3]
        })
        .collect()
}

/// The natural cubic through `points` as one B-spline (its pieces joined, knots at the
/// chord-length parameters, each interior one three times).
fn natural(points: &[Point3]) -> Curve {
    let pieces = natural_pieces(points);
    let mut cps = vec![pieces[0][0]];
    let mut knots = vec![0.0; 4];
    let mut t = 0.0;
    for (i, p) in pieces.iter().enumerate() {
        cps.extend_from_slice(&p[1..]);
        t += points[i].distance(points[i + 1]);
        knots.extend([t; 3]);
    }
    knots.push(t);
    spline(3, &cps, &knots)
}

/// Each Bézier piece of the natural cubic through `points` as its own curve on `[0, 1]`.
fn natural_section(points: &[Point3]) -> Vec<Curve> {
    natural_pieces(points)
        .iter()
        .map(|p| spline(3, p, &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]))
        .collect()
}

/// The largest distance from `samples` points along `rail` to the skin's side faces.
fn sampled_off(k: &Kernel, made: &RailedLoft, rail: &Curve, samples: usize) -> f64 {
    let interval = rail.natural_interval();
    let (t0, t1) = (interval.min(), interval.max());
    let mut worst = 0.0_f64;
    for i in 0..=samples {
        let t = t0 + (t1 - t0) * (i as f64) / (samples as f64);
        let q = rail.evaluate_point(k, t).unwrap();
        let d = made
            .skin
            .side_faces
            .iter()
            .filter_map(|&f| {
                let s = made.skin.brep.face(f).unwrap().surface.as_ref()?;
                surface_closest_point(k, s, q)
                    .ok()
                    .map(|c| c.closest.distance)
            })
            .fold(f64::INFINITY, f64::min);
        worst = worst.max(d);
    }
    worst
}

/// A prism tapered by a straight rail: two sections, the second the first scaled 2 about the
/// loft's axis and lifted 10, the rail the straight line through their first corners. Every
/// station's map is the similarity of the frustum, so the skin is the frustum's wall, and the
/// guided edge is the rail itself: certified, and every point of the rail on the skin.
#[test]
fn a_straight_rail_tapering_similar_sections_gives_the_frustum() {
    let mut k = Kernel::default();
    let at = |s: f64, z: f64| {
        [
            Point3::new(s, 0.0, z),
            Point3::new(s, s, z),
            Point3::new(0.0, s, z),
        ]
    };
    let [a0, a1, a2] = at(1.0, 0.0);
    let [b0, b1, b2] = at(2.0, 10.0);
    let lower = [line(a0, a1), line(a1, a2)];
    let upper = [line(b0, b1), line(b1, b2)];
    let sections = [
        RailSection {
            origin: a0,
            normal: Vec3::Z,
            curves: lower.iter().collect(),
        },
        RailSection {
            origin: b0,
            normal: Vec3::Z,
            curves: upper.iter().collect(),
        },
    ];
    let rail = line(a0, b0);
    let opts = RailLoftOptions::new(1e-6);
    let made = loft_rails(&mut k, &sections, &[&rail], &opts).unwrap();
    assert!(made.bounds[0] <= 1e-6, "{:?}", made.bounds);
    let off = sampled_off(&k, &made, &rail, 50);
    assert!(off <= 1e-9, "{off}");
    // the wall's middle corner line: the frustum's, (1 + t, 1 + t, 10 t)
    let corner = line(a1, b1);
    let off = sampled_off(&k, &made, &corner, 50);
    assert!(off <= 1e-9, "{off}");
}

/// The rectangle and trapezoid lie on perpendicular planes. Their corresponding
/// corners interpolate to nonplanar closed wires, which are valid skin sections.
/// At x in [-2, 2], the YZ area is (20 * (22.5 + 1.25*x) - 10^2) / 2:
/// the outer triangle less the inner triangle. Integrating over x gives 700 mm^3.
#[test]
fn perpendicular_sections_with_a_guide_can_have_nonplanar_interior_wires() {
    let mut k = Kernel::default();
    let first = [
        Point3::new(-2.0, 0.0, 10.0),
        Point3::new(2.0, 0.0, 10.0),
        Point3::new(2.0, 0.0, 20.0),
        Point3::new(-2.0, 0.0, 20.0),
    ];
    let last = [
        Point3::new(-2.0, 10.0, 0.0),
        Point3::new(2.0, 10.0, 0.0),
        Point3::new(2.0, 25.0, 0.0),
        Point3::new(-2.0, 20.0, 0.0),
    ];
    let wire = |points: &[Point3; 4]| {
        points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .map(|(&a, &b)| line(a, b))
            .collect::<Vec<_>>()
    };
    let lower = wire(&first);
    let upper = wire(&last);
    let sections = [
        RailSection {
            origin: Point3::ZERO,
            normal: -Vec3::Y,
            curves: lower.iter().collect(),
        },
        RailSection {
            origin: Point3::ZERO,
            normal: Vec3::Z,
            curves: upper.iter().collect(),
        },
    ];
    let rail = line(Point3::new(0.0, 0.0, 10.0), Point3::new(0.0, 10.0, 0.0));
    let mut opts = RailLoftOptions::new(1e-6);
    opts.fit = RailFit::Stretch;
    opts.closed = true;
    opts.cap = true;
    let mut made = loft_rails(&mut k, &sections, &[&rail], &opts).unwrap();
    assert!(made.bounds.iter().all(|&b| b <= 1e-6));
    assert!(solidcore::topology::brep::is_outward_solid(&mut k, &mut made.skin.brep).unwrap());
    let volume =
        solidcore::mass::integrator::properties::brep_volume_default(&k, &mut made.skin.brep, 1e-9)
            .unwrap();
    assert!((volume - 700.0).abs() < 1e-6, "volume {volume} vs 700");
}

/// The hull side of #3140 (the case its first comment gives): keel and sheer sections, stem and transom rails, each a
/// natural cubic through its points (mm). SolidCraft's even fill of 15 stations per span left
/// the bow edge up to 2.65 mm off the stem in the forefoot's hook, against its own tolerance
/// of 1e-4 of the diagonal (about 1 mm). Here every guided edge is certified within that
/// tolerance, and sampled along each rail the skin is within it.
#[test]
fn a_hull_side_follows_its_stem_and_transom_within_the_tolerance() {
    let mut k = Kernel::default();
    let keel = natural_section(&[
        Point3::new(0.0, 0.0, -160.0),
        Point3::new(4880.0, 0.0, -600.0),
        Point3::new(10000.0, 0.0, 0.0),
    ]);
    let sheer = natural_section(&[
        Point3::new(0.0, 40.0, 1200.0),
        Point3::new(5000.0, 1650.0, 1200.0),
        Point3::new(10000.0, 1500.0, 1200.0),
    ]);
    let stem = natural(&[
        Point3::new(0.0, 40.0, 1200.0),
        Point3::new(0.0, 45.373_332_545_769_244, -62.868_622_627_622_926),
        Point3::new(0.0, 17.293_450_399_707_694, -155.245_265_955_680_08),
        Point3::new(0.0, 0.0, -160.0),
    ]);
    let transom = natural(&[
        Point3::new(10000.0, 0.0, 0.0),
        Point3::new(10000.0, 1_377.264_719_438_087_5, 394.831_623_162_362_3),
        Point3::new(10000.0, 1500.0, 1200.0),
    ]);
    let sections = [
        RailSection {
            origin: Point3::ZERO,
            normal: Vec3::Y,
            curves: keel.iter().collect(),
        },
        RailSection {
            origin: Point3::new(0.0, 0.0, 1200.0),
            normal: Vec3::Z,
            curves: sheer.iter().collect(),
        },
    ];
    // SolidCraft's tolerance: 1e-4 of the sections' control points' diagonal
    let mut lo = Point3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut hi = Point3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for c in keel.iter().chain(&sheer) {
        for p in c.b_spline_base().unwrap().control_polygon().unwrap().0 {
            lo = Point3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
            hi = Point3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
        }
    }
    let tol = (lo.distance(hi) * 1e-4).max(1e-6);
    let mut opts = RailLoftOptions::new(tol);
    opts.max_wires = 33;
    let made = loft_rails(&mut k, &sections, &[&stem, &transom], &opts).unwrap();
    println!(
        "RES hull: tol {tol}, wires {}, bounds {:?}",
        made.wires, made.bounds
    );
    // sampled along each rail first: what the skin does, whatever was certified
    for (name, rail) in [("stem", &stem), ("transom", &transom)] {
        let off = sampled_off(&k, &made, rail, 400);
        println!("RES hull {name}: sampled {off}");
        assert!(
            off <= tol,
            "{name}: the skin is {off} off its rail, past {tol}"
        );
    }
    for b in &made.bounds {
        assert!(*b <= tol, "certified {b} against {tol}");
    }
}

/// Asked tighter than the wires allowed can certify, the loft is refused naming the bound it
/// reached, the tolerance and the wires.
#[test]
fn a_rail_loft_past_its_wires_names_the_bound_reached() {
    let mut k = Kernel::default();
    let keel = natural_section(&[
        Point3::new(0.0, 0.0, -160.0),
        Point3::new(4880.0, 0.0, -600.0),
        Point3::new(10000.0, 0.0, 0.0),
    ]);
    let sheer = natural_section(&[
        Point3::new(0.0, 40.0, 1200.0),
        Point3::new(5000.0, 1650.0, 1200.0),
        Point3::new(10000.0, 1500.0, 1200.0),
    ]);
    let stem = natural(&[
        Point3::new(0.0, 40.0, 1200.0),
        Point3::new(0.0, 45.373_332_545_769_244, -62.868_622_627_622_926),
        Point3::new(0.0, 17.293_450_399_707_694, -155.245_265_955_680_08),
        Point3::new(0.0, 0.0, -160.0),
    ]);
    let sections = [
        RailSection {
            origin: Point3::ZERO,
            normal: Vec3::Y,
            curves: keel.iter().collect(),
        },
        RailSection {
            origin: Point3::new(0.0, 0.0, 1200.0),
            normal: Vec3::Z,
            curves: sheer.iter().collect(),
        },
    ];
    let mut opts = RailLoftOptions::new(1e-3);
    opts.max_wires = 5;
    match loft_rails(&mut k, &sections, &[&stem], &opts) {
        Err(SolidError::Failed(f)) => match f.reason {
            Reason::Sweep(SweepFailure::RailPastStationCap {
                asked,
                reached,
                wires,
            }) => {
                assert_eq!(asked, 1e-3);
                assert!(reached > asked, "{reached}");
                assert!(wires <= 5, "{wires}");
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

/// The refusals the loft names.
#[test]
fn malformed_rail_lofts_are_refused() {
    let mut k = Kernel::default();
    let a = line(Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0));
    let b = line(Point3::new(0.0, 0.0, 5.0), Point3::new(1.0, 0.0, 5.0));
    let sections = [
        RailSection {
            origin: Point3::ZERO,
            normal: Vec3::Z,
            curves: vec![&a],
        },
        RailSection {
            origin: Point3::new(0.0, 0.0, 5.0),
            normal: Vec3::Z,
            curves: vec![&b],
        },
    ];
    let rail = line(Point3::ZERO, Point3::new(0.0, 0.0, 5.0));
    let invalid = |r: solidcore::Result<RailedLoft>, why: &str| match r {
        Err(SolidError::InvalidInput { reason, .. }) => {
            assert!(reason.starts_with(why), "{reason:?} is not {why:?}");
        }
        other => panic!("{other:?}: expected {why:?}"),
    };
    // a tolerance not positive
    invalid(
        loft_rails(&mut k, &sections, &[&rail], &RailLoftOptions::new(0.0)),
        "a railed loft's tolerance is finite and positive",
    );
    // no rail, seventeen rails
    invalid(
        loft_rails(&mut k, &sections, &[], &RailLoftOptions::new(1e-6)),
        "a loft takes 1 to 16 guide rails",
    );
    invalid(
        loft_rails(&mut k, &sections, &[&rail; 17], &RailLoftOptions::new(1e-6)),
        "a loft takes 1 to 16 guide rails",
    );
    // three rails, two of them at one point of the sections
    invalid(
        loft_rails(
            &mut k,
            &sections,
            &[&rail, &rail, &rail],
            &RailLoftOptions::new(1e-6),
        ),
        "the guide rails meet a section at the same point",
    );
    // one section
    invalid(
        loft_rails(
            &mut k,
            &sections[..1],
            &[&rail],
            &RailLoftOptions::new(1e-6),
        ),
        "a railed loft runs through 2 to 100 sections",
    );
    // a rail crossing a section's plane away from its curves
    let away = line(Point3::new(0.5, 3.0, 0.0), Point3::new(0.5, 3.0, 5.0));
    invalid(
        loft_rails(&mut k, &sections, &[&away], &RailLoftOptions::new(1e-6)),
        "a guide rail crosses a section's plane away from its curves",
    );
    // two rails at one point of the sections
    invalid(
        loft_rails(
            &mut k,
            &sections,
            &[&rail, &rail],
            &RailLoftOptions::new(1e-6),
        ),
        "the two guide rails meet a section at the same point",
    );
    // three rails, one crossing a section's plane away from its curves
    let second = line(Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 0.0, 5.0));
    invalid(
        loft_rails(
            &mut k,
            &sections,
            &[&rail, &second, &away],
            &RailLoftOptions::new(1e-6),
        ),
        "a guide rail crosses a section's plane away from its curves",
    );
}

/// Four rails bulge the sides of a square-to-square loft out between its corners: each guided
/// edge is certified within the tolerance of its rail, and the skin passes through every rail's
/// bulge (the straight sides of a plain loft between the corners would not).
#[test]
fn four_rails_bulge_the_sides_of_a_square_loft_through_their_bulges() {
    let mut k = Kernel::default();
    let square = |z: f64| {
        [
            Point3::new(1.0, -1.0, z),
            Point3::new(1.0, 1.0, z),
            Point3::new(-1.0, 1.0, z),
            Point3::new(-1.0, -1.0, z),
        ]
    };
    let [a0, a1, a2, a3] = square(0.0);
    let [b0, b1, b2, b3] = square(10.0);
    let lower = [line(a0, a1), line(a1, a2), line(a2, a3), line(a3, a0)];
    let upper = [line(b0, b1), line(b1, b2), line(b2, b3), line(b3, b0)];
    let sections = [
        RailSection {
            origin: Point3::ZERO,
            normal: Vec3::Z,
            curves: lower.iter().collect(),
        },
        RailSection {
            origin: Point3::new(0.0, 0.0, 10.0),
            normal: Vec3::Z,
            curves: upper.iter().collect(),
        },
    ];
    // each corner's rail bulges out through (1.5 x, 1.5 y, 5)
    let bulge = |a: Point3, b: Point3| natural(&[a, Point3::new(1.5 * a.x, 1.5 * a.y, 5.0), b]);
    let rails = [bulge(a0, b0), bulge(a1, b1), bulge(a2, b2), bulge(a3, b3)];
    let rail_refs: Vec<&Curve> = rails.iter().collect();
    let opts = RailLoftOptions::new(1e-3);
    let made = loft_rails(&mut k, &sections, &rail_refs, &opts).unwrap();
    assert_eq!(made.bounds.len(), 4, "{:?}", made.bounds);
    for (rail, bound) in rails.iter().zip(&made.bounds) {
        assert!(*bound <= 1e-3, "{:?}", made.bounds);
        let off = sampled_off(&k, &made, rail, 50);
        assert!(off <= 1e-3, "{off}");
    }
}

/// Six rails round a circle-to-circle frustum, each from a point of the lower circle to the
/// point above it on the upper circle (twice the radius): straight rails along the frustum's
/// wall, every guided edge certified and on the skin.
#[test]
fn six_rails_round_a_circle_to_circle_frustum_stay_on_the_skin() {
    let mut k = Kernel::default();
    let circle = |r: f64, z: f64| -> Vec<Point3> {
        (0..=6_u32)
            .map(|i| {
                let a = std::f64::consts::TAU * f64::from(i) / 6.0;
                Point3::new(r * a.cos(), r * a.sin(), z)
            })
            .collect()
    };
    let lower_pts = circle(5.0, 0.0);
    let upper_pts = circle(10.0, 10.0);
    let lower = natural_section(&lower_pts);
    let upper = natural_section(&upper_pts);
    let sections = [
        RailSection {
            origin: Point3::ZERO,
            normal: Vec3::Z,
            curves: lower.iter().collect(),
        },
        RailSection {
            origin: Point3::new(0.0, 0.0, 10.0),
            normal: Vec3::Z,
            curves: upper.iter().collect(),
        },
    ];
    let rails: Vec<Curve> = lower_pts
        .iter()
        .zip(&upper_pts)
        .take(6)
        .map(|(a, b)| line(*a, *b))
        .collect();
    let rail_refs: Vec<&Curve> = rails.iter().collect();
    let opts = RailLoftOptions::new(1e-6);
    let made = loft_rails(&mut k, &sections, &rail_refs, &opts).unwrap();
    assert_eq!(made.bounds.len(), 6, "{:?}", made.bounds);
    for (rail, bound) in rails.iter().zip(&made.bounds) {
        assert!(*bound <= 1e-6, "{:?}", made.bounds);
        let off = sampled_off(&k, &made, rail, 50);
        assert!(off <= 1e-6, "{off}");
    }
}

/// Eleven rails guide a loft between two matching quarter-arc sections, each one cubic span (the
/// lower at radius 5, the upper at radius 10, twice as high), the rails from the arcs' points at
/// the same parameters: straight along the wall, except the middle one, which bulges out between
/// the sections. Each section has as many points as rails, so the warp's solve must hold.
#[test]
fn eleven_rails_on_quarter_arcs_with_one_bulging_stay_on_the_skin() {
    const KAPPA: f64 = 0.552_284_749_8;
    let mut k = Kernel::default();
    // the quarter arc of radius r at height z as one cubic Bezier, and its point at t
    let arc = |r: f64, z: f64| -> [Point3; 4] {
        [
            Point3::new(r, 0.0, z),
            Point3::new(r, KAPPA * r, z),
            Point3::new(KAPPA * r, r, z),
            Point3::new(0.0, r, z),
        ]
    };
    let at = |p: &[Point3; 4], t: f64| {
        let s = 1.0 - t;
        let (a, b, c, d) = (s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t);
        p[0] * a + p[1] * b + p[2] * c + p[3] * d
    };
    let lower_pts = arc(5.0, 0.0);
    let upper_pts = arc(10.0, 10.0);
    let lower = [spline(
        3,
        &lower_pts,
        &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
    )];
    let upper = [spline(
        3,
        &upper_pts,
        &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
    )];
    let sections = [
        RailSection {
            origin: Point3::ZERO,
            normal: Vec3::Z,
            curves: lower.iter().collect(),
        },
        RailSection {
            origin: Point3::new(0.0, 0.0, 10.0),
            normal: Vec3::Z,
            curves: upper.iter().collect(),
        },
    ];
    let rails: Vec<Curve> = (0..=10_u32)
        .map(|i| {
            let t = f64::from(i) / 10.0;
            let (a, b) = (at(&lower_pts, t), at(&upper_pts, t));
            if i == 5 {
                let mid = Point3::new(1.3 * (a.x + b.x) / 2.0, 1.3 * (a.y + b.y) / 2.0, 5.0);
                natural(&[a, mid, b])
            } else {
                line(a, b)
            }
        })
        .collect();
    let rail_refs: Vec<&Curve> = rails.iter().collect();
    let opts = RailLoftOptions::new(1e-3);
    let made = loft_rails(&mut k, &sections, &rail_refs, &opts).unwrap();
    assert_eq!(made.bounds.len(), 11, "{:?}", made.bounds);
    for (rail, bound) in rails.iter().zip(&made.bounds) {
        assert!(*bound <= 1e-3, "{:?}", made.bounds);
        let off = sampled_off(&k, &made, rail, 50);
        assert!(off <= 1e-3, "{off}");
    }
}

/// Sixteen rails, the most a loft takes, guide a loft between two quarter-arc sections, each
/// one cubic span (the lower at radius 5, the upper at radius 10, twice as high): one rail per
/// point of each arc, straight along the wall, except the middle one, which bulges out between
/// the sections. Each section is a single span, so the warp must move sixteen points through
/// only the control points one span has.
#[test]
fn sixteen_rails_on_single_span_arcs_with_one_bulging_stay_on_the_skin() {
    const KAPPA: f64 = 0.552_284_749_8;
    let mut k = Kernel::default();
    let arc = |r: f64, z: f64| -> [Point3; 4] {
        [
            Point3::new(r, 0.0, z),
            Point3::new(r, KAPPA * r, z),
            Point3::new(KAPPA * r, r, z),
            Point3::new(0.0, r, z),
        ]
    };
    let at = |p: &[Point3; 4], t: f64| {
        let s = 1.0 - t;
        let (a, b, c, d) = (s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t);
        p[0] * a + p[1] * b + p[2] * c + p[3] * d
    };
    let lower_pts = arc(5.0, 0.0);
    let upper_pts = arc(10.0, 10.0);
    let lower = [spline(
        3,
        &lower_pts,
        &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
    )];
    let upper = [spline(
        3,
        &upper_pts,
        &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
    )];
    let sections = [
        RailSection {
            origin: Point3::ZERO,
            normal: Vec3::Z,
            curves: lower.iter().collect(),
        },
        RailSection {
            origin: Point3::new(0.0, 0.0, 10.0),
            normal: Vec3::Z,
            curves: upper.iter().collect(),
        },
    ];
    let rails: Vec<Curve> = (0..=15_u32)
        .map(|i| {
            let t = f64::from(i) / 15.0;
            let (a, b) = (at(&lower_pts, t), at(&upper_pts, t));
            if i == 8 {
                let mid = Point3::new(1.3 * (a.x + b.x) / 2.0, 1.3 * (a.y + b.y) / 2.0, 5.0);
                natural(&[a, mid, b])
            } else {
                line(a, b)
            }
        })
        .collect();
    let rail_refs: Vec<&Curve> = rails.iter().collect();
    let opts = RailLoftOptions::new(1e-3);
    let made = loft_rails(&mut k, &sections, &rail_refs, &opts).unwrap();
    assert_eq!(made.bounds.len(), 16, "{:?}", made.bounds);
    for (rail, bound) in rails.iter().zip(&made.bounds) {
        assert!(*bound <= 1e-3, "{:?}", made.bounds);
        let off = sampled_off(&k, &made, rail, 6);
        assert!(off <= 1e-3, "{off}");
    }
}

/// A B-spline curve of `degree` through `points` with `weights` (rational) and the full knots.
fn rational(degree: usize, points: &[Point3], weights: &[f64], knots: &[f64]) -> Curve {
    let mut values: Vec<f64> = Vec::new();
    let mut mults: Vec<usize> = Vec::new();
    for &u in knots {
        if values.last() == Some(&u) {
            *mults.last_mut().unwrap() += 1;
        } else {
            values.push(u);
            mults.push(1);
        }
    }
    Curve::from(
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree,
            control_points: points,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &mults,
            knots: &values,
            knot_type: KnotType::Unspecified,
            weights: Some(weights),
            trim: None,
        })
        .unwrap(),
    )
}

/// The quarter arc of radius 10 from (10, 0) to (0, 10) at height `z`: a rational quadratic.
fn quarter_arc(z: f64) -> Curve {
    rational(
        2,
        &[
            Point3::new(10.0, 0.0, z),
            Point3::new(10.0, 10.0, z),
            Point3::new(0.0, 10.0, z),
        ],
        &[1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0],
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    )
}

/// The chord of `quarter_arc` at height `z` as a non-rational quadratic: its control points
/// are collinear, so it is the same straight line.
fn straight_form(z: f64) -> Curve {
    spline(
        2,
        &[
            Point3::new(10.0, 0.0, z),
            Point3::new(5.0, 5.0, z),
            Point3::new(0.0, 10.0, z),
        ],
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    )
}

/// Three rails from the ends and the middle of the chord at z = 0 to the arc's ends and middle
/// at z = 40.
fn three_rails_chord_to_arc() -> [Curve; 3] {
    let r = 10.0 * std::f64::consts::FRAC_1_SQRT_2;
    [
        line(Point3::new(10.0, 0.0, 0.0), Point3::new(10.0, 0.0, 40.0)),
        line(Point3::new(0.0, 10.0, 0.0), Point3::new(0.0, 10.0, 40.0)),
        line(Point3::new(5.0, 5.0, 0.0), Point3::new(r, r, 40.0)),
    ]
}

/// A line section (its straight form, collinear control points) against an arc section under
/// three rails: the line takes the arc's weights and the loft is built, the rails certified.
#[test]
fn a_line_section_takes_an_arc_sections_weights_under_three_rails() {
    let mut k = Kernel::default();
    let lower = [straight_form(0.0)];
    let upper = [quarter_arc(40.0)];
    let sections = [
        RailSection {
            origin: Point3::ZERO,
            normal: Vec3::Z,
            curves: lower.iter().collect(),
        },
        RailSection {
            origin: Point3::new(0.0, 0.0, 40.0),
            normal: Vec3::Z,
            curves: upper.iter().collect(),
        },
    ];
    let rails = three_rails_chord_to_arc();
    let refs: Vec<&Curve> = rails.iter().collect();
    let made = loft_rails(&mut k, &sections, &refs, &RailLoftOptions::new(1e-6)).unwrap();
    assert!(made.bounds.iter().all(|b| *b <= 1e-6), "{:?}", made.bounds);
}

/// A quadratic whose control points are off a line, against the same arc under three rails: its
/// points are not the arc's, so it is refused as curves that do not pair (it is not given the
/// arc's weights).
#[test]
fn a_bent_curve_against_an_arc_does_not_pair_under_three_rails() {
    let mut k = Kernel::default();
    let lower = [spline(
        2,
        &[
            Point3::new(10.0, 0.0, 0.0),
            Point3::new(10.0, 10.0, 0.0),
            Point3::new(0.0, 10.0, 0.0),
        ],
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    )];
    let upper = [quarter_arc(40.0)];
    let sections = [
        RailSection {
            origin: Point3::ZERO,
            normal: Vec3::Z,
            curves: lower.iter().collect(),
        },
        RailSection {
            origin: Point3::new(0.0, 0.0, 40.0),
            normal: Vec3::Z,
            curves: upper.iter().collect(),
        },
    ];
    let rails = three_rails_chord_to_arc();
    let refs: Vec<&Curve> = rails.iter().collect();
    match loft_rails(&mut k, &sections, &refs, &RailLoftOptions::new(1e-6)) {
        Err(SolidError::Unsupported { reason, .. }) => {
            assert!(reason.contains("do not pair"), "{reason}");
        }
        other => panic!("{other:?}"),
    }
}

/// Three sections whose curves pair within solidcore's `1e-9` relative (an interior knot at 0.5
/// in the middle one, 1e-10 above it in the others) under three rails: refining each curve alone
/// gives the first and last one more control point than the middle one, and the loft then reads
/// a control point the middle section lacks. The column must refine to the same knots, so the
/// loft builds.
#[test]
fn a_column_whose_curves_pair_within_the_tolerance_builds_under_three_rails() {
    let mut k = Kernel::default();
    let bent = |z: f64| {
        [
            Point3::new(10.0, 0.0, z),
            Point3::new(0.0, 10.0, z),
            Point3::new(-10.0, 0.0, z),
        ]
    };
    let first = [spline(1, &bent(0.0), &[0.0, 0.0, 0.5 + 1e-10, 1.0, 1.0])];
    let middle = [spline(1, &bent(20.0), &[0.0, 0.0, 0.5, 1.0, 1.0])];
    let last = [spline(1, &bent(40.0), &[0.0, 0.0, 0.5 + 1e-10, 1.0, 1.0])];
    let sections = [
        RailSection {
            origin: Point3::ZERO,
            normal: Vec3::Z,
            curves: first.iter().collect(),
        },
        RailSection {
            origin: Point3::new(0.0, 0.0, 20.0),
            normal: Vec3::Z,
            curves: middle.iter().collect(),
        },
        RailSection {
            origin: Point3::new(0.0, 0.0, 40.0),
            normal: Vec3::Z,
            curves: last.iter().collect(),
        },
    ];
    let rails = [
        line(Point3::new(10.0, 0.0, 0.0), Point3::new(10.0, 0.0, 40.0)),
        line(Point3::new(0.0, 10.0, 0.0), Point3::new(0.0, 10.0, 40.0)),
        line(Point3::new(-10.0, 0.0, 0.0), Point3::new(-10.0, 0.0, 40.0)),
    ];
    let refs: Vec<&Curve> = rails.iter().collect();
    let made = loft_rails(&mut k, &sections, &refs, &RailLoftOptions::new(1e-6)).unwrap();
    assert!(made.bounds.iter().all(|b| *b <= 1e-6), "{:?}", made.bounds);
}

/// Three square sections, each edge a straight line in every section (the middle section's first
/// edge with weights 1 + 1e-10, within the pairing's tolerance), under three rails up three
/// corners: the columns refine together, so the loft builds.
#[test]
fn a_straight_column_with_a_line_within_the_tolerance_builds_under_three_rails() {
    let mut k = Kernel::default();
    let knots = [0.0, 0.0, 1.0, 1.0];
    let corners = |z: f64| {
        [
            Point3::new(10.0, 10.0, z),
            Point3::new(-10.0, 10.0, z),
            Point3::new(-10.0, -10.0, z),
            Point3::new(10.0, -10.0, z),
        ]
    };
    let edges = |z: f64| {
        let c = corners(z);
        [
            line(c[0], c[1]),
            line(c[1], c[2]),
            line(c[2], c[3]),
            line(c[3], c[0]),
        ]
    };
    let first = edges(0.0);
    let c20 = corners(20.0);
    let middle = [
        rational(1, &[c20[0], c20[1]], &[1.0 + 1e-10, 1.0 + 1e-10], &knots),
        line(c20[1], c20[2]),
        line(c20[2], c20[3]),
        line(c20[3], c20[0]),
    ];
    let last = edges(40.0);
    let sections = [
        RailSection {
            origin: Point3::ZERO,
            normal: Vec3::Z,
            curves: first.iter().collect(),
        },
        RailSection {
            origin: Point3::new(0.0, 0.0, 20.0),
            normal: Vec3::Z,
            curves: middle.iter().collect(),
        },
        RailSection {
            origin: Point3::new(0.0, 0.0, 40.0),
            normal: Vec3::Z,
            curves: last.iter().collect(),
        },
    ];
    let c40 = corners(40.0);
    let rails = [
        line(corners(0.0)[0], c40[0]),
        line(corners(0.0)[1], c40[1]),
        line(corners(0.0)[2], c40[2]),
    ];
    let refs: Vec<&Curve> = rails.iter().collect();
    let made = loft_rails(&mut k, &sections, &refs, &RailLoftOptions::new(1e-6)).unwrap();
    assert!(made.bounds.iter().all(|b| *b <= 1e-6), "{:?}", made.bounds);
}
