//! A sphere's NURBS form is recognised as its sphere far from the origin, at any radius and
//! orientation, and the half-meridian test allows the angles' rounding at the points' size,
//! no more (#3131). A sphere of radius 2.5 at 1e5 reads 180 + 6.7e-10 degrees, past the fixed
//! half-meridian tolerance, and was not recognised.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_section::station::as_b_spline;
use solidcore::curve::curve::Curve;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::{BSplineCurve, NurbCircleParam};
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::gamma;
use solidcore::nurbs::srf_basic::Surface as Net;
use solidcore::primitive::api_primitives::create_sphere;
use solidcore::surface::implicit::AnalyticSupport;
use solidcore::surface::sphere::Sphere;
use solidcore::surface::surf_of_revolution::SurfOfRevolution;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p1 as bp1;
use solidcore::Kernel;

/// The allowance the recogniser adds, as a position (#15039): `S 1.75 (γ_K + γ_33) |P| +
/// 3.5 γ_4 |P| + γ_31 π r`, widened by `1e-3` and `γ_16`, `|P| = 1 + |corners| + 2 r`,
/// `K = 16 (2 + 2 + 2)` for a form of degrees (2, 2), and `S = 6.02`, the end angle's
/// conditioning for a half turn's fit (written out so that a change to the allowance fails;
/// the recogniser reads `S` off each fit, 5.98 to 6.02 for these forms). In degrees it is
/// this over `r`, turned to degrees.
fn allowance_position(form: &BSplineSurface, r: f64) -> f64 {
    let d = form.natural_uv_domain();
    let corners = form
        .evaluate_point(d.min())
        .unwrap()
        .max_dimension()
        .max(form.evaluate_point(d.max()).unwrap().max_dimension());
    let size = 1.0 + corners + 2.0 * r;
    let position = 6.02 * 1.75 * (gamma(16 * 6) + gamma(33)) * size + 3.5 * gamma(4) * size;
    (position + gamma(31) * std::f64::consts::PI * r) * 1.001 * (1.0 + gamma(16))
}

/// The NURBS form of a sphere of radius `r` at `c`, turned by `turn` degrees about the axis
/// (1, 1, 1) when it is not 0, and its label.
fn form_at(k: &mut Kernel, c: Point3, r: f64, turn: f64) -> (BSplineSurface, AnalyticSupport) {
    let b = create_sphere(k, Point3::new(0.0, 0.0, 0.0), r).unwrap();
    let f = bp1::faces(&b, None).unwrap()[0];
    let mut s = b
        .face(f)
        .unwrap()
        .surface
        .as_ref()
        .unwrap()
        .copy(k)
        .unwrap();
    // turned about (1, 1, 1) by `turn`, then moved to `c`
    let a = turn.to_radians();
    let n = Vec3::new(1.0, 1.0, 1.0) * (1.0 / 3.0_f64.sqrt());
    let rot = |v: Vec3| v * a.cos() + n.cross(v) * a.sin() + n * (n.dot(v) * (1.0 - a.cos()));
    let mut place = Axis2Placement::default();
    place
        .set_canonical(
            c,
            rot(Vec3::new(1.0, 0.0, 0.0)),
            rot(Vec3::new(0.0, 1.0, 0.0)),
        )
        .unwrap();
    s.transform(k, &place, None).unwrap();
    let form = as_b_spline(k, &s).unwrap().unwrap().into_owned();
    (form, AnalyticSupport::of(&s).unwrap())
}

/// Recognised at radii 1e-3 to 1e3, 0 to 1e7 from the origin (at most 1e9 radii out), on the
/// axes and turned off them, with its radius and centre within the allowance as a position.
#[test]
fn a_far_sphere_is_recognised_on_its_own_numbers() {
    let mut k = Kernel::default();
    for r in [1e-3, 1.0, 2.5, 1e3] {
        for off in [0.0, 1e3, 1e5, 1e6, 1e7] {
            // a sphere of radius 1e-3 at 1e7, ten orders apart: upstream's iso-circle test
            // fails with a status there (1001), before any angle is read; unchanged here
            if off > 1e9 * r {
                continue;
            }
            for turn in [0.0, 37.0] {
                let c = Point3::new(off, off, off + 10.0);
                let (form, _) = form_at(&mut k, c, r, turn);
                let found = Sphere::is_nurb_surface_sphere(&k, &form, 1.0)
                    .unwrap()
                    .unwrap_or_else(|| panic!("not recognised: r {r} at {off}, turned {turn}"));
                let AnalyticSupport::Sphere { placement, radius } =
                    AnalyticSupport::of(&Surface::from(found)).unwrap()
                else {
                    panic!("not a sphere");
                };
                let tol = allowance_position(&form, r);
                assert!((radius - r).abs() <= tol, "r {r} at {off}: radius {radius}");
                assert!(
                    (placement.origin() - c).length() <= tol,
                    "r {r} at {off}: centre {:e} past {tol:e}",
                    (placement.origin() - c).length()
                );
            }
        }
    }
}

/// A half meridian of `180 + extra` degrees, radius `r`, at `c`: a circle arc from -90 to
/// `90 + extra` degrees in the plane y = c.y, turned a quarter turn about the vertical.
fn long_meridian(k: &Kernel, c: Point3, r: f64, extra: f64) -> BSplineSurface {
    let mut frame = Axis2Placement::default();
    frame
        .set_canonical(c, Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0))
        .unwrap();
    let generator = BSplineCurve::create_circle_segment(
        3,
        &frame,
        r,
        -90.0,
        90.0 + extra,
        NurbCircleParam::Quadratic,
    )
    .unwrap();
    let mut sor = SurfOfRevolution::new(
        k,
        Curve::from(generator),
        c,
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Extent2d::from_bounds(0.0, -90.0, 90.0, 90.0 + extra),
        false,
    )
    .unwrap();
    sor.make_nurb(k).unwrap();
    as_b_spline(k, &Surface::from(sor))
        .unwrap()
        .unwrap()
        .into_owned()
}

/// The half meridian is allowed `1e-12` degrees (upstream) plus the angles' rounding at the
/// points' size. A meridian `d` past 180 degrees also folds the generator's STEP interval
/// against the sphere's latitudes by `2 d` (a point past the pole reads latitude `90 - d`),
/// held to the same allowance. So a meridian 0.4 of the allowance past is recognised (both
/// tests pass; halve the allowance and the fold, 0.8 of it, is refused) and one twice the
/// allowance past is refused, at the origin and at 1e5. A doubled allowance is caught by
/// `sphere.rs`'s own unit test of `meridian_rounding` against its closed form.
#[test]
fn the_half_meridian_is_allowed_its_rounding_and_no_more() {
    let k = Kernel::default();
    let r = 2.5;
    for off in [0.0, 1e5] {
        let c = Point3::new(off, off, off + 10.0);
        let exact = long_meridian(&k, c, r, 0.0);
        let allowed = 1e-12 + (allowance_position(&exact, r) / r).to_degrees();
        let over = long_meridian(&k, c, r, 2.0 * allowed);
        assert!(
            Sphere::is_nurb_surface_sphere(&k, &over, 1.0)
                .unwrap()
                .is_none(),
            "at {off}: 180 + {:e} degrees recognised",
            2.0 * allowed
        );
        let under = long_meridian(&k, c, r, 0.4 * allowed);
        assert!(
            Sphere::is_nurb_surface_sphere(&k, &under, 1.0)
                .unwrap()
                .is_some(),
            "at {off}: 180 + {:e} degrees refused",
            0.4 * allowed
        );
    }
}

/// A form or a label 1e-3 off the sphere is refused at every offset to 1e6: the label by
/// `lies_on` (`AnalyticSupport::build`), the form by the iso-circle tests. Neither reaches
/// the angle allowance; they show the allowance did not open what those tests close. At 1e7
/// the iso-circle tests' own tolerance, `100 SM_EFF_ZERO (1 + |corners|)` (about 2e-3),
/// admits a 1e-3 move: upstream's, unchanged here.
#[test]
fn a_form_or_label_1e3_off_is_refused_to_1e6() {
    let mut k = Kernel::default();
    for off in [0.0, 1e3, 1e4, 1e5, 1e6] {
        let c = Point3::new(off, off, off + 10.0);
        let (form, support) = form_at(&mut k, c, 2.5, 0.0);
        let AnalyticSupport::Sphere { placement, radius } = support else {
            panic!("a sphere");
        };
        let off_label = AnalyticSupport::Sphere {
            placement,
            radius: radius + 1e-3,
        };
        assert!(off_label.build(&k, form.copy()).is_err(), "label at {off}");
        let net = form.gw_nurb().unwrap();
        let mut cpts = net.cpts();
        let (i, j) = (cpts.len() / 2, cpts[0].len() / 2);
        let w = cpts[i][j].w;
        cpts[i][j].x += 1e-3
            * if w == solidcore::nurbs::math::NOW {
                1.0
            } else {
                w
            };
        let (p, q) = net.degrees();
        let moved = Net::new(cpts, p, q, net.u_knots().to_vec(), net.v_knots().to_vec()).unwrap();
        let moved = BSplineSurface::from_gw_nurb(&moved);
        assert!(
            Sphere::is_nurb_surface_sphere(&k, &moved, 1.0)
                .unwrap()
                .is_none(),
            "form at {off}"
        );
    }
}
