//! Lazy analytic surfaces (a cone or cylinder, sphere, torus, surface of revolution or
//! surface of extrusion constructed without its NURB, as the B-rep reader's recogniser
//! leaves them) answer every query as if the NURB had been made first: upstream's
//! analytic surfaces always hold `m_pNurb`, and its `const` queries reach it through
//! `GetOrCreateGwNurbPointer`. Each kind is checked against a copy whose NURB was made
//! up front, bit for bit: evaluation, the area integrand's first derivatives, the
//! normal, an iso curve, the bounding box, the API evaluations' domain check, a
//! clamped `SmIsoCurve` and its UV line, and an offset's extension of the surface.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::curve::curve::Curve;
use solidcore::curve::iso_curve::IsoCurve;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::Point2;
use solidcore::kernel::Kernel;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_surface::SurfParamType;
use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use solidcore::nurbs::knots::Side;
use solidcore::surface::cone::Cone;
use solidcore::surface::offset_surface::{OffsetSurface, SS_UNKNOWN};
use solidcore::surface::sphere::{Sphere, SphereOptions};
use solidcore::surface::surf_of_extrusion::SurfOfExtrusion;
use solidcore::surface::surf_of_revolution::SurfOfRevolution;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::surface::torus::{Torus, TorusOptions};

/// Whether the analytic kind of `s` has no NURB yet.
fn is_lazy(s: &Surface) -> bool {
    match s.kind() {
        SurfaceKind::Cone(c) => c.rev().base().gw_nurb().is_none(),
        SurfaceKind::Sphere(c) => c.base().base().gw_nurb().is_none(),
        SurfaceKind::Torus(c) => c.base().base().gw_nurb().is_none(),
        SurfaceKind::SurfOfRevolution(c) => c.base().gw_nurb().is_none(),
        SurfaceKind::SurfOfExtrusion(c) => c.base().gw_nurb().is_none(),
        _ => false,
    }
}

/// `s` with its NURB made on the object first (what upstream's surface holds).
fn made(k: &Kernel, s: &Surface) -> Surface {
    let mut kind = match s.kind() {
        // an exact copy (no copy-constructor knot fix-up): upstream makes the NURB on
        // the object itself
        SurfaceKind::SurfOfExtrusion(e) => SurfaceKind::SurfOfExtrusion(Box::new(e.clone_exact())),
        other => other.clone(),
    };
    match &mut kind {
        SurfaceKind::Cone(c) => c.make_nurb(k).unwrap(),
        SurfaceKind::Sphere(c) => c.make_nurb(k).unwrap(),
        SurfaceKind::Torus(c) => c.make_nurb(k).unwrap(),
        SurfaceKind::SurfOfRevolution(c) => c.make_nurb(k).unwrap(),
        SurfaceKind::SurfOfExtrusion(c) => c.make_nurb(k).unwrap(),
        _ => panic!("not an analytic kind"),
    }
    let s = Surface::new(kind);
    assert!(!is_lazy(&s));
    s
}

/// The answers of the queries under test, as bits (an error is its message).
/// `dom` is the made surface's natural domain (`GetNaturalUVDomain` makes the NURB
/// upstream; the port's kernel-free `natural_uv_domain` cannot).
fn answers(k: &Kernel, s: &Surface, dom: &Extent2d, uv: Point2) -> Vec<String> {
    let mut out = Vec::new();
    let flags = EvaluateFlags::default();
    out.push(format!("{:?}", s.evaluate(k, uv, 1, 1, flags)));
    out.push(format!("{:?}", s.evaluate_point(k, uv)));
    let mut d = [[Vec3::ZERO; 2]; 2];
    let first = s.evaluate_simple_first_into(k, uv, flags, &mut d);
    out.push(format!("{first:?} {d:?}"));
    out.push(format!(
        "{:?}",
        s.evaluate_normal(k, uv, Side::Left, Side::Left)
    ));
    for param in [SurfParamType::U, SurfParamType::V] {
        let iso_value = if param == SurfParamType::U {
            uv.x
        } else {
            uv.y
        };
        let iso = s.create_iso_parametric_curve(k, param, iso_value, 1e-3, None, false);
        out.push(match iso {
            Ok(iso) => {
                let ivl = iso.curve.natural_interval();
                let mid = 0.5 * (ivl.min() + ivl.max());
                format!(
                    "{:?} {:?}",
                    iso.curve.evaluate_point(k, mid),
                    iso.max_gap_3d
                )
            }
            Err(e) => format!("Err({e:?})"),
        });
    }
    out.push(format!(
        "{:?}",
        s.calculate_bounding_box(k, dom, [true, false, false], false)
    ));
    // SmApiEvaluateSurfacePoint checks `uv` against GetNaturalUVDomain
    out.push(format!(
        "{:?}",
        solidcore::query::brep::evaluate_surface_point(k, Some(s), uv)
    ));
    // SmIsoCurve(.., bClampInput = TRUE) clamps to GetNaturalUVDomain, and
    // CreateUVIsoLine spans it
    let past = dom.max().x + 0.5 * dom.size().x;
    out.push(
        match IsoCurve::from_surface(k, s.clone(), SurfParamType::U, past, true, true) {
            Ok(iso) => {
                let line = iso
                    .create_uv_iso_line_with_nurb(k)
                    .map(|l| l.natural_interval());
                format!("{:?} {line:?}", iso.iso_parameter())
            }
            Err(e) => format!("Err({e:?})"),
        },
    );
    out
}

fn check(k: &Kernel, name: &str, lazy: &Surface) {
    assert!(
        is_lazy(lazy),
        "{name}: the fixture must start without its NURB"
    );
    let reference = made(k, lazy);
    // (the made reference has its NURB: built)
    let dom = reference.natural_uv_domain_if_built();
    let uv = dom.evaluate(0.3, 0.6);
    let want = answers(k, &reference, &dom, uv);
    assert!(
        want.iter().all(|a| !a.starts_with("Err")),
        "{name}: the made surface answers every query: {want:?}"
    );
    // twice: the first query makes the NURB, the second reads the kept one
    for round in 0..2 {
        assert_eq!(answers(k, lazy, &dom, uv), want, "{name}, round {round}");
    }
}

#[test]
fn lazy_analytic_surfaces_answer_as_if_their_nurb_were_made() {
    let k = Kernel::default();
    let (o, x, y) = (
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    );

    let cone = Cone::new_bounded(o, x, y, 2.0, 1.0, 0.0, 270.0, 3.0, false, false, false);
    check(&k, "cone", &Surface::from(cone));
    let cylinder = Cone::new_bounded(o, x, y, 2.0, 2.0, 0.0, 270.0, 3.0, false, false, false);
    check(&k, "cylinder", &Surface::from(cylinder));

    let sphere_opts = SphereOptions {
        swap_uv: false,
        inside_out: false,
        circle_nurb: None,
    };
    let sphere = Sphere::new(
        &k,
        o,
        x,
        y,
        Extent2d::from_bounds(0.0, -45.0, 180.0, 45.0),
        2.0,
        sphere_opts,
    )
    .unwrap();
    check(&k, "sphere", &Surface::from(sphere));

    let torus_opts = TorusOptions {
        swap_uv: false,
        inside_out: false,
        circle_nurb: None,
    };
    let torus = Torus::new(
        &k,
        o,
        x,
        y,
        Extent2d::from_bounds(0.0, 0.0, 180.0, 180.0),
        3.0,
        1.0,
        torus_opts,
    )
    .unwrap();
    check(&k, "torus", &Surface::from(torus));

    let line = Curve::from(
        solidcore::primitive::api_curves::create_line_segment(
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(2.5, 0.0, 2.0),
        )
        .unwrap(),
    );
    let ivl = line.natural_interval();
    let revolution = SurfOfRevolution::new(
        &k,
        line,
        o,
        x,
        y,
        Extent2d::from_bounds(0.0, ivl.min(), 180.0, ivl.max()),
        false,
    )
    .unwrap();
    check(&k, "surface of revolution", &Surface::from(revolution));

    let generator = Curve::from(
        solidcore::primitive::api_curves::create_line_segment(
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(3.0, 1.0, 0.0),
        )
        .unwrap(),
    );
    let extrusion = SurfOfExtrusion::new(
        &k,
        generator,
        o,
        Vec3::new(0.0, 0.0, 2.0),
        Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
        false,
    )
    .unwrap();
    check(&k, "surface of extrusion", &Surface::from(extrusion));
}

/// An offset that extends its lazy base past the base domain (`sm_ExtendSurface`, which
/// reads the base's NURB through `GetOrCreateGwNurbPointer`) extends it as it extends
/// the made base.
#[test]
fn an_offset_extends_a_lazy_base_as_a_made_one() {
    let k = Kernel::default();
    let (o, x, y) = (
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    );
    let cylinder = Cone::new_bounded(o, x, y, 2.0, 2.0, 0.0, 270.0, 3.0, false, false, false);
    let lazy = Surface::from(cylinder);
    assert!(is_lazy(&lazy), "the fixture must start without its NURB");
    let reference = made(&k, &lazy);
    // (the made reference has its NURB: built)
    let dom = reference.natural_uv_domain_if_built();
    // past the high v side, inside u
    let uv = Point2::new(dom.evaluate(0.5, 0.5).x, dom.max().y + 0.25 * dom.size().y);
    let extended = |base: &Surface| {
        let mut off = OffsetSurface::new(&k, 0.25, base.clone(), true, SS_UNKNOWN).unwrap();
        off.set_allow_extension(true);
        let point = off.evaluate_point_extending(&k, uv);
        format!("{point:?} {:?}", off.natural_uv_domain_with_nurb(&k))
    };
    let want = extended(&reference);
    assert!(!want.starts_with("Err"), "the made base extends: {want}");
    assert_eq!(extended(&lazy), want);
}
