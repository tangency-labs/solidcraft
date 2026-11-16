//! Code generic over a surface type (`SurfaceEvaluate`) reads a lazy analytic
//! surface's natural UV domain as upstream's virtual `GetNaturalUVDomain` does: from
//! the NURB made first. A lazy cone or sphere (constructed without its NURB) answers
//! the generic domain, `SmSurface::EvaluateNormal` (the base body, which reads the
//! domain at a degenerate point such as a pole), `ComputeInwardVector`,
//! `ComputeHigherOrderDerivs` and a tangent field over its natural domain bit for bit
//! as a copy whose NURB was made up front.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::curve::curve::Curve;
use solidcore::curve::tangent_field::{TangentDirType, TangentField};
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::Vec2;
use solidcore::kernel::Kernel;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::knots::Side;
use solidcore::surface::cone::Cone;
use solidcore::surface::sphere::{Sphere, SphereOptions};
use solidcore::surface::surface::p2::SurfaceEvaluate;
use solidcore::surface::surface::p7::{
    base_evaluate_normal, compute_higher_order_derivs, compute_inward_vector,
};
use solidcore::surface::surface::{Surface, SurfaceKind};

/// `s` with its NURB made on the object first (what upstream's surface holds).
fn made(k: &Kernel, s: &Surface) -> Surface {
    let mut kind = s.kind().clone();
    match &mut kind {
        SurfaceKind::Cone(c) => c.make_nurb(k).unwrap(),
        SurfaceKind::Sphere(c) => c.make_nurb(k).unwrap(),
        _ => panic!("not a cone or sphere"),
    }
    Surface::new(kind)
}

/// The generic answers under test, as text (an error is its message). `dom` is the
/// made surface's natural domain.
fn answers(k: &Kernel, s: &Surface, dom: &Extent2d) -> Vec<String> {
    let uv = dom.evaluate(0.3, 0.6);
    let mut out = vec![
        format!("{:?}", SurfaceEvaluate::natural_uv_domain_with_nurb(s, k)),
        format!("{:?}", compute_inward_vector(k, s, uv)),
        // inside the domain: the forward step stays in it (an empty domain would
        // turn the step backwards)
        format!(
            "{:?}",
            compute_higher_order_derivs(k, s, uv, Vec2::new(1.0, 0.0), true, true)
        ),
    ];
    // the base normal inside and on the domain's v limits (a sphere's poles, where the
    // first derivatives degenerate and the body steps into the domain)
    for v in [0.6, 0.0, 1.0] {
        let at = dom.evaluate(0.3, v);
        out.push(format!(
            "{:?}",
            base_evaluate_normal(k, s, at, Side::Left, Side::Left)
        ));
    }
    // a tangent field along a UV line inside the domain, over the surface's natural
    // domain (`uv_domain = None`)
    let (a, b) = (dom.evaluate(0.2, 0.3), dom.evaluate(0.7, 0.8));
    let uv_line = Curve::from(
        solidcore::primitive::api_curves::create_line_segment(
            Point3::new(a.x, a.y, 0.0),
            Point3::new(b.x, b.y, 0.0),
        )
        .unwrap(),
    );
    let field = TangentField::new(
        k,
        &uv_line,
        s,
        TangentDirType::Interpolate,
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        None,
    );
    out.push(match field {
        Ok(f) => {
            let ivl = uv_line.natural_interval();
            let mid = 0.5 * (ivl.min() + ivl.max());
            format!("{:?} {:?}", f.evaluate_point(k, mid), f.evaluate(k, mid, 1))
        }
        Err(e) => format!("Err({e:?})"),
    });
    out
}

fn check(k: &Kernel, name: &str, lazy: &Surface) {
    let reference = made(k, lazy);
    // (the made reference has its NURB: built)
    let dom = reference.natural_uv_domain_if_built();
    assert_ne!(
        lazy.natural_uv_domain_if_built(),
        dom,
        "{name}: the fixture must start without its NURB"
    );
    let want = answers(k, &reference, &dom);
    assert!(
        want.iter().all(|a| !a.starts_with("Err")),
        "{name}: the made surface answers every query: {want:?}"
    );
    assert_eq!(answers(k, lazy, &dom), want, "{name}");
}

#[test]
fn generic_domain_of_a_lazy_analytic_is_its_made_nurbs() {
    let k = Kernel::default();
    let (o, x, y) = (
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    );
    let cone = Cone::new_bounded(o, x, y, 2.0, 1.0, 0.0, 270.0, 3.0, false, false, false);
    check(&k, "cone", &Surface::from(cone));

    // pole to pole, so the base normal meets the degenerate rows
    let sphere = Sphere::new(
        &k,
        o,
        x,
        y,
        Extent2d::from_bounds(0.0, -90.0, 180.0, 90.0),
        2.0,
        SphereOptions {
            swap_uv: false,
            inside_out: false,
            circle_nurb: None,
        },
    )
    .unwrap();
    check(&k, "sphere", &Surface::from(sphere));
}

/// The generic domain of a lazy surface with no NURB yet is not the empty domain the
/// kernel-free accessor answers.
#[test]
fn generic_domain_makes_the_nurb() {
    let k = Kernel::default();
    let cone = Cone::new_bounded(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        2.0,
        2.0,
        0.0,
        180.0,
        1.0,
        false,
        false,
        false,
    );
    let lazy = Surface::from(cone);
    let empty = lazy.natural_uv_domain_if_built();
    let domain = SurfaceEvaluate::natural_uv_domain_with_nurb(&lazy, &k).unwrap();
    assert_ne!(domain, empty);
    assert_eq!(domain, made(&k, &lazy).natural_uv_domain_if_built());
}
