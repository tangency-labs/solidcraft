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
use solidcore::{Brep, Kernel};

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
    const N: usize = 32;
    let half = PI / 2.0 / N as f64 / 2.0;
    let on = |a: f64, s: f64| Point3::new(10.0 + R * s * a.cos(), 0.0, R * s * a.sin());
    let mut control = vec![on(0.0, 1.0)];
    let mut weights = vec![1.0];
    for j in 0..N {
        let a = (2 * j + 1) as f64 * half;
        let out = if j == 5 { 1.0 + bulge } else { 1.0 };
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
    Surface::from(
        SurfOfRevolution::create_canonical(
            k,
            round_generatrix(bulge),
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
