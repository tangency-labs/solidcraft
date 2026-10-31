//! #2091 (WR-670): with `UpstreamFixes::precise_properties_split_at_surface_knots`
//! (WR-494) each iso-u integral of `SmODEAreaIFEO` runs knot span by knot span, but
//! every piece's last point was evaluated in the knot span above it (upstream's
//! `bVFromLeft = TRUE`). Where the surface's derivatives jump at a v knot (a G2 but
//! not C2 section, merged into one face under `geometric_g2_continuity`), the integrand
//! jumped at each piece's end and the Runge-Kutta steps shrank toward every knot.
//! `UpstreamFixes::precise_properties_span_end_from_below` evaluates the end from
//! the piece's own span.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::mass::integrator::{OdeIntegrator, OdeOptions, StepCounts};
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::math::CPoint;
use solidcore::sweep::api_surfaces::create_skin_primitive;
use solidcore::topology::face::p2::{OdeIsoCurveLength, PrecisePropertyLayout};
use solidcore::topology::Brep;
use solidcore::Kernel;
use std::f64::consts::PI;

const RADIUS: f64 = 5.0;
const HEIGHT: f64 = 10.0;
/// Unequal arcs on unit knot spans: the section's speed jumps at every join (C0 in its
/// parameter), its tangent and curvature do not (G2).
const ARCS_DEG: [f64; 6] = [30.0, 50.0, 70.0, 40.0, 80.0, 90.0];

/// The circle of `RADIUS` at height `z`, as one rational quadratic arc a unit knot span.
fn section(z: f64) -> Vec<Curve> {
    let mut start = 0.0_f64;
    let mut out = Vec::new();
    for (i, deg) in ARCS_DEG.iter().enumerate() {
        let half = deg.to_radians() / 2.0;
        let mid = start + half;
        let end = start + 2.0 * half;
        let w = half.cos();
        let p0 = Vec3::new(z, RADIUS * start.cos(), RADIUS * start.sin());
        let p1 = Vec3::new(z, RADIUS * mid.cos() / w, RADIUS * mid.sin() / w);
        let p2 = Vec3::new(z, RADIUS * end.cos(), RADIUS * end.sin());
        let cpw = [
            CPoint::new(p0.x, p0.y, p0.z, 1.0),
            CPoint::new(p1.x * w, p1.y * w, p1.z * w, w),
            CPoint::new(p2.x, p2.y, p2.z, 1.0),
        ];
        let k0 = i as f64;
        let k1 = k0 + 1.0;
        let knots = [k0, k0, k0, k1, k1, k1];
        out.push(Curve::from(
            BSplineCurve::from_data(3, 2, 5, 2, &cpw, &knots).unwrap(),
        ));
        start = end;
    }
    out
}

fn kernel(span_end_from_below: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.geometric_g2_continuity = true;
    k.fixes.precise_properties_split_at_surface_knots = true;
    k.fixes.precise_properties_span_end_from_below = span_end_from_below;
    k
}

/// The capped prism lofted between the two sections: one side face across all six
/// spans under `geometric_g2_continuity`.
fn prism(k: &mut Kernel) -> Brep {
    let curves: Vec<Curve> = section(0.0).into_iter().chain(section(HEIGHT)).collect();
    let refs: Vec<_> = curves.iter().map(Some).collect();
    let n = ARCS_DEG.len();
    let mut b = create_skin_primitive(k, &[n, n], &refs, 1, true)
        .unwrap()
        .brep;
    b.refresh_cached_topology();
    b
}

fn volume(k: &Kernel, b: &mut Brep, accuracy: f64) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, accuracy, &attrs).unwrap()
}

#[test]
fn flag_is_off_by_default() {
    assert!(
        !Kernel::default()
            .fixes
            .precise_properties_span_end_from_below
    );
}

/// The volume of the prism spanning six knot spans matches π r² h with the flag on, as
/// it does with it off.
#[test]
fn multi_span_prism_volume_matches_closed_form() {
    let exact = PI * RADIUS * RADIUS * HEIGHT;
    for on in [false, true] {
        let mut k = kernel(on);
        let mut b = prism(&mut k);
        assert_eq!(b.face_list().len(), 3, "one side face and two caps");
        let v = volume(&k, &mut b, 1e-8);
        let err = (v - exact).abs() / exact;
        eprintln!("flag {on}: volume {v} vs {exact} ({err:e})");
        assert!(
            err <= 1e-9,
            "flag {on}: volume {v} vs {exact} ({err:e} relative)"
        );
    }
}

/// The side face's B-spline surface and its v knots (ends included).
fn side_surface(b: &Brep) -> (solidcore::surface::surface::Surface, Vec<f64>) {
    for &f in b.face_list() {
        let s = b.face(f).unwrap().surface.clone().unwrap();
        let k = Kernel::default();
        let knots = solidcore::topology::face::p2::surface_knots_inside(
            &k,
            &s,
            solidcore::nurbs::b_spline_surface::p1::SurfParamType::V,
            f64::MIN,
            f64::MAX,
        )
        .unwrap();
        if knots.len() == ARCS_DEG.len() + 1 {
            return (s, knots);
        }
    }
    panic!("no multi-span face");
}

/// One iso-u piece from a v knot to the next, as `SmODEAreaIFEO` integrates it.
fn piece_steps(eval: &OdeIsoCurveLength<'_>, a: f64, b: f64, n: usize) -> (Vec<f64>, StepCounts) {
    let mut y = vec![0.0; n];
    let scale = vec![1.0; n];
    let mut scratch = vec![0.0; n + 1];
    let counts = OdeIntegrator::default()
        .integrate(
            &mut y,
            a,
            b,
            OdeOptions {
                accuracy: 1e-9,
                initial_step: (b - a) / 2.0,
                minimum_step: 0.0,
                scale: Some(&scale),
            },
            |t, _, dy| {
                eval.evaluate(t, &mut scratch)?;
                for (d, v) in dy.iter_mut().zip(scratch.iter().skip(1)) {
                    *d = *v;
                }
                Ok(())
            },
        )
        .unwrap();
    (y, counts)
}

/// At a piece's end knot the fixed integrand is the limit from inside the piece, and
/// the piece takes fewer steps; upstream's takes the next span's value there.
#[test]
fn piece_end_is_evaluated_in_its_own_span() {
    let mut k = kernel(true);
    let b = prism(&mut k);
    let (surface, knots) = side_surface(&b);
    let flags = solidcore::topology::face::p2::PPF_AREA;
    let layout = PrecisePropertyLayout::new(flags);
    let n = layout.num_components;
    let (lo, hi) = (knots[0], knots[1]);
    let u = 0.5;
    let upstream = OdeIsoCurveLength::new(&k, &surface, u, Vec3::ZERO, 0.0, &layout);
    let fixed = upstream.with_v_end_from_below(hi);

    let mut at_end_up = vec![0.0; n + 1];
    let mut at_end_fix = vec![0.0; n + 1];
    let mut inside = vec![0.0; n + 1];
    upstream.evaluate(hi, &mut at_end_up).unwrap();
    fixed.evaluate(hi, &mut at_end_fix).unwrap();
    upstream.evaluate(hi - 1e-9, &mut inside).unwrap();
    let area = |d: &[f64]| d[1];
    assert!(
        (area(&at_end_fix) - area(&inside)).abs() <= 1e-6 * area(&inside),
        "fixed end {} vs inside {}",
        area(&at_end_fix),
        area(&inside)
    );
    assert!(
        (area(&at_end_up) - area(&inside)).abs() > 1e-3 * area(&inside),
        "upstream end {} should be the next span's, not {}",
        area(&at_end_up),
        area(&inside)
    );

    let (y_up, c_up) = piece_steps(&upstream, lo, hi, n);
    let (y_fix, c_fix) = piece_steps(&fixed, lo, hi, n);
    eprintln!("steps upstream {c_up:?} fixed {c_fix:?}; {y_up:?} vs {y_fix:?}");
    assert!(
        c_fix.good + c_fix.reduced < c_up.good + c_up.reduced,
        "fixed {c_fix:?} vs upstream {c_up:?}"
    );
    assert!((y_fix[0] - y_up[0]).abs() <= 1e-8 * y_fix[0].abs());
}
