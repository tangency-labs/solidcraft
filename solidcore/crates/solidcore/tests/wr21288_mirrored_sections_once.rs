//! WR-21288: a mirrored round's approximated sections are mirrored once.
//!
//! The round of radius 2 between the planes z = 0 and x = 5 (two bilinear sheets, 10 by
//! 10), mirrored and complemented, is the 270 degree arc of radius 2 about x = 5, z = 0:
//! the centre x = 3, z = 2 mirrored about the chord between the touching lines. The rails
//! run past the sheets (y from -5 to 15), and their ends, extended from the base, sit an
//! ulp off the others: the end sections' sweep rounds up and their approximated arcs take
//! one knot more, so the sections take a second pass. Upstream mirrors there every
//! section again, so the end sections, which the first pass had mirrored and the second
//! keeps, went back to the circle about x = 3, z = 2, and the skin left the cylinder by up
//! to 13.8.

use solidcore::blend::fillet_executive::api::{surface_surface_fillet, FilletSurfaceArg};
use solidcore::math::{Point2, Vec3};
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p1;
use solidcore::Kernel;

fn sheet(c: [[f64; 3]; 4]) -> Surface {
    let [a, b, c, d] = c.map(Vec3::from);
    Surface::from(BSplineSurface::create_bilinear_surface(a, b, c, d).unwrap())
}

#[test]
fn wr21288_mirrored_complement_round_is_its_cylinder() {
    let mut k = Kernel::default();
    let first = sheet([[0., 0., 0.], [10., 0., 0.], [0., 10., 0.], [10., 10., 0.]]);
    let second = sheet([[5., 0., -5.], [5., 0., 5.], [5., 10., -5.], [5., 10., 5.]]);
    let tolerance = 0.01;
    let body = surface_surface_fillet(
        &mut k,
        FilletSurfaceArg::Free(&first),
        FilletSurfaceArg::Free(&second),
        2.0,
        2.0,
        tolerance,
        1,
        true,
        true,
    )
    .unwrap()
    .expect("a result body");
    let faces = p1::faces(&body, None).unwrap();
    assert_eq!(faces.len(), 1);
    let surface = body.face(faces[0]).unwrap().surface.clone().unwrap();
    let d = surface.natural_uv_domain_with_nurb(&k).unwrap();
    let (u, v) = (d.u_interval(), d.v_interval());
    let mut worst: f64 = 0.0;
    for i in 0..=16 {
        for j in 0..=16 {
            let uv = Point2::new(
                u.min() + (u.max() - u.min()) * f64::from(i) / 16.0,
                v.min() + (v.max() - v.min()) * f64::from(j) / 16.0,
            );
            let p = surface.evaluate_point(&k, uv).unwrap();
            let rho = ((p.x - 5.0).powi(2) + p.z.powi(2)).sqrt();
            worst = worst.max((rho - 2.0).abs());
            if j == 8 {
                // the section's middle is the 270 degree arc's far point
                let far = ((p.x - (5.0 + 2f64.sqrt())).powi(2) + (p.z + 2f64.sqrt()).powi(2)).sqrt();
                assert!(far <= tolerance, "middle of the section at {uv:?}: {far:e} off");
            }
        }
    }
    assert!(worst <= tolerance, "{worst:e} off the cylinder about x = 5, z = 0");
}
