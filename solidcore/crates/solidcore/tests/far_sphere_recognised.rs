//! A sphere's NURBS form far from the origin is recognised as its sphere, and a form or a label
//! 1e-3 off it is not, at every offset. The half meridian's angles are read off points known to
//! their evaluation's rounding at their own size: a sphere of radius 2.5 at 1e5 reads 180 +
//! 6.7e-10 degrees, past the fixed half-meridian tolerance, and was not recognised.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_section::station::as_b_spline;
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::srf_basic::Surface as Net;
use solidcore::primitive::api_primitives::create_sphere;
use solidcore::surface::implicit::AnalyticSupport;
use solidcore::surface::sphere::Sphere;
use solidcore::topology::brep::p1 as bp1;
use solidcore::Kernel;

const OFFSETS: [f64; 5] = [0.0, 1e3, 1e4, 1e5, 1e6];
const RADIUS: f64 = 2.5;

/// The NURBS form of a sphere of radius 2.5 at `c`, and its label.
fn form_at(k: &mut Kernel, c: Point3) -> (BSplineSurface, AnalyticSupport) {
    let b = create_sphere(k, c, RADIUS).unwrap();
    let f = bp1::faces(&b, None).unwrap()[0];
    let s = b.face(f).unwrap().surface.as_ref().unwrap();
    let form = as_b_spline(k, s).unwrap().unwrap().into_owned();
    (form, AnalyticSupport::of(s).unwrap())
}

#[test]
fn a_far_sphere_is_recognised_on_its_own_numbers() {
    let mut k = Kernel::default();
    for off in OFFSETS {
        let c = Point3::new(off, off, off + 10.0);
        let (form, _) = form_at(&mut k, c);
        let found = Sphere::is_nurb_surface_sphere(&k, &form, 1.0)
            .unwrap()
            .unwrap_or_else(|| panic!("not recognised at {off}"));
        let AnalyticSupport::Sphere { placement, radius } =
            AnalyticSupport::of(&solidcore::surface::surface::Surface::from(found)).unwrap()
        else {
            panic!("not a sphere at {off}");
        };
        assert!((radius - RADIUS).abs() < 1e-9, "{off} {radius}");
        assert!(
            (placement.origin() - c).length() < 1e-9 * (1.0 + off),
            "{off}"
        );
    }
}

#[test]
fn a_form_or_label_1e3_off_is_refused_at_every_offset() {
    let mut k = Kernel::default();
    for off in OFFSETS {
        let c = Point3::new(off, off, off + 10.0);
        let (form, support) = form_at(&mut k, c);
        // a label 1e-3 too big
        let AnalyticSupport::Sphere { placement, radius } = support else {
            panic!("a sphere");
        };
        let off_label = AnalyticSupport::Sphere {
            placement,
            radius: radius + 1e-3,
        };
        assert!(off_label.build(&k, form.copy()).is_err(), "label at {off}");
        // a form with one inner control point moved 1e-3
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
