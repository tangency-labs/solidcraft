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
    // no rail, three rails
    invalid(
        loft_rails(&mut k, &sections, &[], &RailLoftOptions::new(1e-6)),
        "a loft takes one or two guide rails",
    );
    invalid(
        loft_rails(
            &mut k,
            &sections,
            &[&rail, &rail, &rail],
            &RailLoftOptions::new(1e-6),
        ),
        "a loft takes one or two guide rails",
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
}
