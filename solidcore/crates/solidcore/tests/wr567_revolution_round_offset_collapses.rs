//! WR-567 (the fork's tangency-labs/smlib@8b2090f, smlib#64; SolidCraft
//! `shell_of_a_disc_rounded_smaller_than_the_wall`, solidcraft#319): a disc r 15, 5 thick,
//! one rim rounded r 1.5 and shelled 1.6 or 2 inward, open at one end. The round is a
//! fillet's surface of revolution of an arc; moved in past its radius it has no offset,
//! as a torus's, and the floor's and side's offsets meet across it in a circle. Upstream's
//! `SmSurfOfRevolution` never collapses, and its offset builds the arc turned inside out
//! (r 0.1 or 0.5 round its centre): upstream's shell fails with 1001 open at the rounded
//! end's other side, and is 0.85% and 4.2% low open at the rounded end. Without the
//! collapse test the kernel's cavity selection (WR-667) still gives the closed form open
//! at the other end.
//!
//! Expected: the cavity is the cylinder r 15 - t, 5 - t deep; the round takes
//! r^2 (1 - pi/4) from the disc's section, its centroid r (10 - 3 pi) / (3 (4 - pi)) in
//! from the rim, swept round the axis (Pappus).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use core::f64::consts::PI;

use offset_fixture::{planar_faces_at, report, shell};
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::geom::extent1d::Extent1d;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::p1::{BSplineCurveForm, CanonicalData, KnotType};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::surface::surf_of_revolution::SurfOfRevolution;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Brep, Kernel, SolidError};

const RD: f64 = 15.0;
const H: f64 = 5.0;
const R: f64 = 1.5;

/// The disc from z = -5 to 0, its circular rim at `edge_z` rounded r 1.5
/// (`SmApiFilletEdges`, circular cross-section).
fn rounded_disc(k: &mut Kernel, edge_z: f64) -> Brep {
    let mut b = create_cylinder(k, Point3::new(0.0, 0.0, -H), RD, H).unwrap();
    let edges: Vec<_> = b
        .edges()
        .iter()
        .filter(|(_, e)| {
            let c = e.curve.as_ref().unwrap();
            [e.interval.min(), e.interval.mid(), e.interval.max()]
                .iter()
                .all(|&t| (c.evaluate_point(k, t).unwrap().z - edge_z).abs() < 1e-9)
        })
        .map(|(id, _)| id)
        .collect();
    assert_eq!(edges.len(), 1, "the rim at z {edge_z}");
    fillet_edges(k, &mut b, &edges, R, 1, 1, 1.0).unwrap();
    b.refresh_cached_topology();
    b
}

/// The shell's volume: the disc less the round's corner less the cavity.
fn expected(t: f64) -> f64 {
    let corner =
        R * R * (1.0 - PI / 4.0) * 2.0 * PI * (RD - R * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI)));
    PI * RD * RD * H - corner - PI * (RD - t) * (RD - t) * (H - t)
}

/// Shells the disc rounded at `edge_z` by `t`, open at `open_z`; whether the result is
/// an intact manifold solid of the expected volume to 1e-6.
fn shelled(k: &mut Kernel, edge_z: f64, open_z: f64, t: f64) -> bool {
    let b = rounded_disc(k, edge_z);
    let open = planar_faces_at(k, &b, 2, open_z);
    assert_eq!(open.len(), 1, "the end face at z {open_z}");
    let result = shell(k, b, -t, &open);
    report(
        k,
        &format!("WR-567 disc rounded at z {edge_z}, open at z {open_z}, shelled {t}"),
        result,
        Some(expected(t)),
        1e-6,
    )
}

/// The round's offset collapses where the shell moves past its radius, and the floor's and
/// side's offsets meet across it in a circle: every case is the closed form, and the shell
/// is valid at every `check_brep` level.
#[test]
fn wr567_round_thinner_than_the_wall_collapses() {
    for (edge_z, open_z) in [(-H, 0.0), (-H, -H), (0.0, -H)] {
        for t in [1.6, 2.0] {
            let mut k = Kernel::default();
            assert!(
                shelled(&mut k, edge_z, open_z, t),
                "rounded at z {edge_z}, open at z {open_z}, shelled {t}"
            );
            let b = rounded_disc(&mut k, edge_z);
            let open = planar_faces_at(&k, &b, 2, open_z);
            let mut result = shell(&mut k, b, -t, &open).unwrap();
            result.refresh_cached_topology();
            for level in [CheckLevel::Level0, CheckLevel::Level1, CheckLevel::Level2] {
                let checked = check_brep(&mut k, &mut result, level).unwrap();
                assert!(
                    checked.is_valid(),
                    "rounded at z {edge_z}, open at z {open_z}, shelled {t}, {level:?}: {:?}",
                    checked.lines()
                );
            }
        }
    }
}

/// A quarter of the circle r 1.5 about (10, 0, 0) in the XZ plane, as 32 exact rational
/// quadratic arcs (knots `j / 32`, interior ones double), with the middle control point of
/// arc 5 (parameters `[5/32, 6/32]`) moved out from the centre by `bulge` of its distance.
/// Arc 5 lies between the arc test's samples at `1/8` and `1/4`, and the circle's three
/// nominating points (`0`, `0.45`, `0.9`) lie on other arcs.
fn round_generatrix(bulge: f64) -> Curve {
    round_generatrix_at(&[5], bulge)
}

/// [`round_generatrix`] with the middle control points of the arcs `arcs` moved out by
/// `bulge` (in by a negative one).
fn round_generatrix_at(arcs: &[usize], bulge: f64) -> Curve {
    const N: usize = 32;
    let half = PI / 2.0 / N as f64 / 2.0;
    let on = |a: f64, s: f64| Point3::new(10.0 + R * s * a.cos(), 0.0, R * s * a.sin());
    let mut control = vec![on(0.0, 1.0)];
    let mut weights = vec![1.0];
    for j in 0..N {
        let a = (2 * j + 1) as f64 * half;
        let out = if arcs.contains(&j) { 1.0 + bulge } else { 1.0 };
        control.push(on(a, out / half.cos()));
        weights.push(half.cos());
        control.push(on(a + half, 1.0));
        weights.push(1.0);
    }
    let knots: Vec<f64> = (0..=N).map(|j| j as f64 / N as f64).collect();
    let mut mult = vec![2; N + 1];
    mult[0] = 3;
    mult[N] = 3;
    Curve::from(
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: 2,
            control_points: &control,
            form: BSplineCurveForm::default(),
            knot_multiplicities: &mult,
            knots: &knots,
            knot_type: KnotType::default(),
            weights: Some(&weights),
            trim: None,
        })
        .unwrap(),
    )
}

/// The round's surface: the generatrix turned about the Z axis.
fn round_surface(k: &Kernel, bulge: f64) -> Surface {
    revolved(k, round_generatrix(bulge))
}

/// `gen` turned about the Z axis.
fn revolved(k: &Kernel, gen: Curve) -> Surface {
    Surface::from(
        SurfOfRevolution::create_canonical(
            k,
            gen,
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            false,
        )
        .unwrap(),
    )
}

/// An exact arc of 32 rational pieces is proved on its circle over the whole generatrix: its
/// offset by twice its radius towards the centre collapses, and the other way does not.
#[test]
fn wr567_an_exact_arc_generatrix_is_proved_and_collapses() {
    let k = Kernel::default();
    let s = round_surface(&k, 0.0);
    let inward = s.offset_collapses(&k, 2.0 * R, 1e-7).unwrap();
    let outward = s.offset_collapses(&k, -2.0 * R, 1e-7).unwrap();
    assert!(
        inward != outward,
        "exactly one side collapses: {inward} {outward}"
    );
    let d = if inward { 2.0 * R } else { -2.0 * R };
    assert!(s.create_offset_surface(&k, d, 1e-7).unwrap().is_none());
    assert!(s.create_offset_surface(&k, -d, 1e-7).unwrap().is_some());
}

/// A circle generatrix is its own arc by definition, at any tolerance: turned about Z, its
/// offset by twice its radius towards the centre collapses even at a tolerance below its
/// NURB's rounding (`1e-15`; the goldens' `surface.offset_surface` gen1247 asks this of a
/// circle turned many times round). Bounding the NURB instead refused it as unproved.
#[test]
fn wr567_a_circle_generatrix_collapses_at_any_tolerance() {
    let k = Kernel::default();
    let circle = Ellipse::new_circle(
        &k,
        Point3::new(10.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        Extent1d::from_min_max(0.0, 90.0),
        R,
        &EllipseOptions::default(),
    )
    .unwrap();
    let s = Surface::from(
        SurfOfRevolution::create_canonical(
            &k,
            Curve::from(circle),
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            false,
        )
        .unwrap(),
    );
    for tol in [1e-7, 1e-15] {
        let inward = s.offset_collapses(&k, 2.0 * R, tol).unwrap();
        let outward = s.offset_collapses(&k, -2.0 * R, tol).unwrap();
        assert!(inward != outward, "{tol}: {inward} {outward}");
        let d = if inward { 2.0 * R } else { -2.0 * R };
        assert!(
            s.create_offset_surface(&k, d, tol).unwrap().is_none(),
            "{tol}"
        );
    }
}

/// solidcraft#17790's review: a generatrix that is the circle at every one of the arc
/// test's nine samples but bulges about 1% of its radius out between two of them is no arc.
/// The sampled test called it one, so its offset "collapsed" and the face was dropped; the
/// bound over the whole generatrix finds the bulge, and the offset is not reported collapsed
/// on either side.
#[test]
fn wr567_a_generatrix_bulging_between_the_samples_is_no_arc() {
    let k = Kernel::default();
    let exact = round_surface(&k, 0.0);
    let d = if exact.offset_collapses(&k, 2.0 * R, 1e-7).unwrap() {
        2.0 * R
    } else {
        -2.0 * R
    };
    let s = round_surface(&k, 0.02);
    // the arc test alone still nominates it: every sample is on the circle
    let SurfaceKind::SurfOfRevolution(rev) = s.kind() else {
        panic!("a surface of revolution");
    };
    let gen = rev.gen_curve().unwrap();
    let (_, radius, _, _) = gen.is_arc(&k, 9, 1e-7).unwrap().unwrap();
    assert!((radius - R).abs() < 1e-9, "{radius}");
    // the bulge is real: its middle is off the circle by far more than the tolerance
    let p = gen.evaluate_point(&k, 5.5 / 32.0).unwrap();
    let off = (Point3::new(10.0, 0.0, 0.0).distance(p) - R).abs();
    assert!(off > 1e-3, "{off}");
    for dd in [d, -d] {
        assert!(!s.offset_collapses(&k, dd, 1e-7).unwrap(), "{dd}");
        // the face is never dropped: the offset is asked of the surface (this generatrix,
        // kinked where its pieces meet, refuses a simple offset), never `Ok(None)`
        assert!(
            !matches!(s.create_offset_surface(&k, dd, 1e-7), Ok(None)),
            "{dd}"
        );
    }
}

/// The side `d` of the exact round that collapses at twice its radius: `R` or `-R`.
fn collapsing_side(k: &Kernel) -> f64 {
    if round_surface(k, 0.0)
        .offset_collapses(k, 2.0 * R, 1e-7)
        .unwrap()
    {
        R
    } else {
        -R
    }
}

/// solidcraft#17907's review: arcs 15 and 16 (the two either side of the middle of the
/// generatrix, where the side is read) bulged by 5e-8 of the radius stay within 1e-7 of the
/// circle, so the generatrix is proved on it, but their curvature there is off by about
/// 1e-4 of the circle's. An offset of `r (1 -+ 4e-5)` towards the centre leaves the circle a
/// radius of `+-6e-5`, far from the tolerance: the curvature read at one point decided it
/// the other way (a face dropped, or a real collapse missed). The answer now follows the
/// circle, or is refused where the curvature read there disagrees; never the other way.
/// Each bulge has one offset decided and one refused (pinned, so refusing all fails).
#[test]
fn wr567_a_near_arc_at_its_radius_follows_the_circle() {
    let k = Kernel::default();
    let side = collapsing_side(&k);
    // (outward bulges bend harder at the middle: the offset just short of the radius is
    // refused there, and the one past it decided; inward ones the other way round)
    for (bulge, short_decided) in [(5e-8, false), (-5e-8, true)] {
        let s = revolved(&k, round_generatrix_at(&[15, 16], bulge));
        for (scale, collapses, decided) in [
            (1.0 - 4e-5, false, short_decided),
            (1.0 + 4e-5, true, !short_decided),
        ] {
            let d = side * scale;
            let r = s.offset_collapses(&k, d, 1e-7);
            if decided {
                assert_eq!(r.unwrap(), collapses, "bulge {bulge}, d {d}");
            } else {
                assert!(
                    matches!(r, Err(SolidError::Unsupported { .. })),
                    "bulge {bulge}, d {d}: {r:?}"
                );
            }
            if !collapses {
                assert!(
                    !matches!(s.create_offset_surface(&k, d, 1e-7), Ok(None)),
                    "bulge {bulge}, d {d}"
                );
            }
        }
    }
}

/// solidcraft#17907's second review: as [`wr567_a_near_arc_at_its_radius_follows_the_circle`]
/// with arc 5 bulged instead, away from the middle of the generatrix where the side is
/// read. The curvature read there is the circle's, so one sample agreed with the circle
/// and the collapse was decided from its radius alone: the inward bulge (radius of
/// curvature from `r` up to about `r (1 + 8.3e-5)` on arc 5) at `r (1 + 4e-5)` was
/// reported collapsed and its face dropped, though the offset passes the centres of
/// curvature only off arc 5. The collapse is now proved over the whole generatrix: the
/// outcomes are those of arcs 15 and 16 (a radius of curvature from about
/// `r (1 - 8.3e-5)` to `r` outward, `r` to `r (1 + 8.3e-5)` inward), each bulge with one
/// offset decided and one refused (pinned, so refusing all fails).
#[test]
fn wr567_a_near_arc_off_the_middle_follows_its_curvature() {
    let k = Kernel::default();
    let side = collapsing_side(&k);
    for (bulge, short_decided) in [(5e-8, false), (-5e-8, true)] {
        let s = revolved(&k, round_generatrix_at(&[5], bulge));
        for (scale, collapses, decided) in [
            (1.0 - 4e-5, false, short_decided),
            (1.0 + 4e-5, true, !short_decided),
        ] {
            let d = side * scale;
            let r = s.offset_collapses(&k, d, 1e-7);
            if decided {
                assert_eq!(r.unwrap(), collapses, "bulge {bulge}, d {d}");
            } else {
                assert!(
                    matches!(r, Err(SolidError::Unsupported { .. })),
                    "bulge {bulge}, d {d}: {r:?}"
                );
                assert!(
                    !matches!(s.create_offset_surface(&k, d, 1e-7), Ok(None)),
                    "bulge {bulge}, d {d}"
                );
            }
        }
        // far from the radius the bulge decides nothing: the collapse at twice the radius
        // is still found, and the offset at half of it still built
        assert!(s.offset_collapses(&k, 2.0 * side, 1e-7).unwrap(), "{bulge}");
        assert!(
            !s.offset_collapses(&k, 0.5 * side, 1e-7).unwrap(),
            "{bulge}"
        );
        assert!(s
            .create_offset_surface(&k, 2.0 * side, 1e-7)
            .unwrap()
            .is_none());
    }
}

/// The tilted circle of [`wr567_a_tilted_circle_follows_its_profile`]: r 1.5 about
/// (10, 0, 0) with axes X and `(0, 1/2, sqrt 3 / 2)`, from 30 to 90 degrees.
fn tilted_circle(k: &Kernel) -> Ellipse {
    Ellipse::new_circle(
        k,
        Point3::new(10.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 0.5, 3.0_f64.sqrt() / 2.0),
        Extent1d::from_min_max(30.0, 90.0),
        R,
        &EllipseOptions::default(),
    )
    .unwrap()
}

/// The smallest and largest radius of curvature of the profile `(sqrt(x^2 + y^2), z)` of
/// `gen` (about Z) over `[t0, t1]`, by central differences at `n` steps (a measurement for
/// the test's margins, independent of the certificate).
fn profile_radius_range(k: &Kernel, gen: &Curve, t0: f64, t1: f64, n: usize) -> (f64, f64) {
    let profile = |t: f64| {
        let p = gen.evaluate_point(k, t).unwrap();
        (p.x.hypot(p.y), p.z)
    };
    let h = (t1 - t0) / n as f64 * 1e-2;
    let (mut lo, mut hi) = (f64::INFINITY, 0.0_f64);
    for i in 1..n {
        let t = t0 + (t1 - t0) * i as f64 / n as f64;
        let (a, b, c) = (profile(t - h), profile(t), profile(t + h));
        let (r1, z1) = ((c.0 - a.0) / (2.0 * h), (c.1 - a.1) / (2.0 * h));
        let (r2, z2) = (
            (c.0 - 2.0 * b.0 + a.0) / (h * h),
            (c.1 - 2.0 * b.1 + a.1) / (h * h),
        );
        let rho = (r1 * r1 + z1 * z1).powf(1.5) / (r1 * z2 - z1 * r2).abs();
        lo = lo.min(rho);
        hi = hi.max(rho);
    }
    (lo, hi)
}

/// solidcraft#17907's second review (the reviewer's counterexample) and solidcraft#18018's
/// review: the circle of [`tilted_circle`] turned about Z. Its plane holds no line through
/// the axis, so the generatrix is not the surface's profile and its radius (1.5) is not
/// where the offset folds: the profile's radius of curvature runs over a range inside
/// `(1.1, 2)` that holds `r` (MEASURED below, to the test's margins). The circle
/// shortcut decided `2 r` and `r` towards the centre from the radius (the face dropped at `r`, where the
/// profile's radius straddles it). The collapse is now decided on the profile: offsets
/// away from its centres of curvature, and towards them below its smallest radius, are
/// built (as main built them); towards them past its largest radius it collapses (as main
/// found); at `r` towards them it is refused, never dropped. A zero offset stays `false`
/// (its offset is the surface itself).
#[test]
fn wr567_a_tilted_circle_follows_its_profile() {
    let k = Kernel::default();
    let gen = Curve::from(tilted_circle(&k));
    let domain = gen.natural_interval();
    let (lo, hi) = profile_radius_range(&k, &gen, domain.min(), domain.max(), 600);
    // the margins below: 1 below the smallest radius, `r` inside the range, 3 above it
    assert!(lo > 1.1 && lo < R && hi > R && hi < 2.0, "{lo} {hi}");
    let s = revolved(&k, gen);
    // the side towards the profile's centres of curvature collapses past its largest radius
    let inward = s.offset_collapses(&k, 3.0, 1e-7).unwrap();
    let outward = s.offset_collapses(&k, -3.0, 1e-7).unwrap();
    assert!(
        inward != outward,
        "exactly one side collapses: {inward} {outward}"
    );
    let towards = if inward { 1.0 } else { -1.0 };
    // (the reviewer measured main: `Ok(true)` at -3, so the centres are on the negative side)
    assert_eq!(towards, -1.0);
    assert!(s
        .create_offset_surface(&k, 3.0 * towards, 1e-7)
        .unwrap()
        .is_none());
    // below the smallest radius on the towards side, and anything on the other side: built
    for d in [1e-9, 0.1, 0.5, 1.0] {
        for dd in [d * towards, -d * towards] {
            assert!(!s.offset_collapses(&k, dd, 1e-7).unwrap(), "{dd}");
            assert!(
                s.create_offset_surface(&k, dd, 1e-7).unwrap().is_some(),
                "{dd}"
            );
        }
    }
    for d in [R, 3.0] {
        assert!(!s.offset_collapses(&k, -d * towards, 1e-7).unwrap(), "{d}");
    }
    // at `r` towards the centres the profile's radius is on both sides of the offset
    let r = s.offset_collapses(&k, R * towards, 1e-7);
    assert!(matches!(r, Err(SolidError::Unsupported { .. })), "{r:?}");
    assert!(!matches!(
        s.create_offset_surface(&k, R * towards, 1e-7),
        Ok(None)
    ));
    // a zero offset is the surface itself: no point passes its centre of curvature
    // (`surface.offset_surface` gen1943 offsets a tilted circle's revolution by 0)
    assert!(!s.offset_collapses(&k, 0.0, 1e-7).unwrap());
    assert!(s.create_offset_surface(&k, 0.0, 1e-7).unwrap().is_some());
}

/// solidcraft#18018's review (B1): the circle r 1 about (0.5, 0, 0) in the XZ plane from 90
/// to 140 degrees crosses the Z axis at 120 degrees. Turned about Z, the surface's normal
/// flips there (`N ~ x (-z', 0, x')`) while the generatrix bends the same way, so past the
/// axis the offset moves away from the centres of curvature: one side read at the middle
/// (115 degrees, `x > 0`) reported the offset by 1, 2 or 3 towards the centre collapsed
/// and the face was dropped, though the lobe past the axis offsets to a radius of `1 + d`.
/// The generatrix's signed distance from the axis must keep one sign: it does not, so every
/// offset is refused, never dropped. The arc from 60 to 110 degrees keeps off the axis and
/// is decided.
#[test]
fn wr567_a_generatrix_crossing_the_axis_is_refused() {
    let k = Kernel::default();
    let arc = |from: f64, to: f64| {
        Curve::from(
            Ellipse::new_circle(
                &k,
                Point3::new(0.5, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
                Extent1d::from_min_max(from, to),
                1.0,
                &EllipseOptions::default(),
            )
            .unwrap(),
        )
    };
    let crossing = revolved(&k, arc(90.0, 140.0));
    for d in [-3.0, -2.0, -1.0, 1.0, 2.0, 3.0] {
        let r = crossing.offset_collapses(&k, d, 1e-7);
        assert!(
            matches!(r, Err(SolidError::Unsupported { .. })),
            "{d}: {r:?}"
        );
        assert!(
            !matches!(crossing.create_offset_surface(&k, d, 1e-7), Ok(None)),
            "{d}"
        );
    }
    let clear = revolved(&k, arc(60.0, 110.0));
    let inward = clear.offset_collapses(&k, 2.0, 1e-7).unwrap();
    let outward = clear.offset_collapses(&k, -2.0, 1e-7).unwrap();
    assert!(
        inward != outward,
        "exactly one side collapses: {inward} {outward}"
    );
    let towards = if inward { 1.0 } else { -1.0 };
    assert!(!clear.offset_collapses(&k, 0.5 * towards, 1e-7).unwrap());
    assert!(!clear.offset_collapses(&k, -2.0 * towards, 1e-7).unwrap());
}

/// A generatrix whose bulge comes exactly to the tolerance (its peak deviation from the
/// circle its samples nominate) is neither proved on the circle nor off it: the collapse
/// test and the offset are refused with `Unsupported`, the face is never dropped.
#[test]
fn wr567_a_generatrix_at_the_tolerance_edge_is_refused() {
    let k = Kernel::default();
    let s = round_surface(&k, 1e-6);
    let SurfaceKind::SurfOfRevolution(rev) = s.kind() else {
        panic!("a surface of revolution");
    };
    let gen = rev.gen_curve().unwrap();
    let (frame, radius, _, _) = gen.is_arc(&k, 9, 1e-7).unwrap().unwrap();
    // the peak, at the bulged arc's middle by symmetry
    let p = gen.evaluate_point(&k, 5.5 / 32.0).unwrap();
    let tol = (frame.origin().distance(p) - radius).abs();
    assert!(tol > 1e-7 && tol < 1e-5, "{tol}");
    for d in [2.0 * R, -2.0 * R] {
        assert!(
            matches!(
                s.offset_collapses(&k, d, tol),
                Err(SolidError::Unsupported { .. })
            ),
            "{d}: {:?}",
            s.offset_collapses(&k, d, tol)
        );
        assert!(
            matches!(
                s.create_offset_surface(&k, d, tol),
                Err(SolidError::Unsupported { .. })
            ),
            "{d}"
        );
    }
}

/// solidcraft#17907's second review: a circle in a plane square to the axis, turned about
/// it, sweeps a flat annulus. Its normal is the axis, so the generatrix does not bend along
/// it (`N . k = 0`) and no side can be read: the collapse test is refused, and no offset by
/// twice the radius either way is dropped. Taking `0` as the side `+1` reported one of them
/// collapsed (main built both).
#[test]
fn wr567_a_circle_with_no_side_is_refused() {
    let k = Kernel::default();
    let circle = Ellipse::new_circle(
        &k,
        Point3::new(10.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Extent1d::from_min_max(0.0, 90.0),
        R,
        &EllipseOptions::default(),
    )
    .unwrap();
    let s = revolved(&k, Curve::from(circle));
    for d in [2.0 * R, -2.0 * R] {
        let r = s.offset_collapses(&k, d, 1e-7);
        assert!(
            matches!(r, Err(SolidError::Unsupported { .. })),
            "{d}: {r:?}"
        );
        assert!(
            !matches!(s.create_offset_surface(&k, d, 1e-7), Ok(None)),
            "{d}"
        );
    }
}

/// The circle r 1 about `(cx, 0, 0)` in the XZ plane, from `from` to `to` degrees.
fn xz_arc(k: &Kernel, cx: f64, from: f64, to: f64) -> Curve {
    Curve::from(
        Ellipse::new_circle(
            k,
            Point3::new(cx, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Extent1d::from_min_max(from, to),
            1.0,
            &EllipseOptions::default(),
        )
        .unwrap(),
    )
}

/// solidcraft#18018's second review (R1): the arc of
/// [`wr567_a_generatrix_crossing_the_axis_is_refused`] from 90 degrees to just past the Z
/// axis (120 degrees and `1e-9` or `1e-6` more: its end `1.5e-11` or `1.5e-8` past the axis, a
/// pole snapped to the axis with import noise). The part past the axis is within the
/// tolerance `1e-7` of it, so the generatrix keeps to its side of the axis to the tolerance
/// and its offset is decided as main decides it: `-3` collapses, `3` and `+-0.5` do not.
/// Allowing only the rounding past the axis refused all four. An end `1.5e-6` past the axis
/// (past the tolerance) is still refused.
#[test]
fn wr567_an_arc_ending_past_the_axis_within_the_tolerance_is_decided() {
    let k = Kernel::default();
    for past in [1e-9, 1e-6] {
        let gen = xz_arc(&k, 0.5, 90.0, 120.0 + past);
        let end = gen.evaluate_point(&k, 120.0 + past).unwrap();
        assert!(end.x < 0.0 && end.x > -1e-7, "{past}: {end:?}");
        let s = revolved(&k, gen);
        assert!(s.offset_collapses(&k, -3.0, 1e-7).unwrap(), "{past}");
        assert!(s.create_offset_surface(&k, -3.0, 1e-7).unwrap().is_none());
        for d in [3.0, 0.5, -0.5] {
            assert!(!s.offset_collapses(&k, d, 1e-7).unwrap(), "{past}: {d}");
        }
    }
    let gen = xz_arc(&k, 0.5, 90.0, 120.0 + 1e-4);
    assert!(gen.evaluate_point(&k, 120.0 + 1e-4).unwrap().x < -1e-6);
    let s = revolved(&k, gen);
    for d in [-3.0, 3.0, 0.5, -0.5] {
        let r = s.offset_collapses(&k, d, 1e-7);
        assert!(
            matches!(r, Err(SolidError::Unsupported { .. })),
            "{d}: {r:?}"
        );
    }
}

/// solidcraft#18018's second review (R1): the horn torus, the full circle r 1 about
/// (1, 0, 0) in the XZ plane turned about Z. It touches the axis at 180 degrees (its tangent
/// along the axis there) and keeps to one side of it everywhere else, so its normal does not
/// flip, and every centre of curvature is the circle's centre: the offset by 2 towards the
/// centres collapses, the one by 2 away and `+-0.5` do not (as main decides it). The middle of
/// the generatrix, where the side was read, is the point on the axis: the side is read
/// where the generatrix keeps off the axis.
#[test]
fn wr567_a_horn_torus_is_decided() {
    let k = Kernel::default();
    let s = revolved(&k, xz_arc(&k, 1.0, 0.0, 360.0));
    assert!(s.offset_collapses(&k, -2.0, 1e-7).unwrap());
    assert!(s.create_offset_surface(&k, -2.0, 1e-7).unwrap().is_none());
    for d in [2.0, 0.5, -0.5] {
        assert!(!s.offset_collapses(&k, d, 1e-7).unwrap(), "{d}");
    }
}
