//! A cylinder wall of a short arc far from the origin, extended 500 and 1500 mm along its
//! rulings and round its axis. The iso that certifies the turn is interpolated between rulings
//! up to 3 m long: a weight ratio per row, differing by the continuation's rounding, took it out
//! of its plane by 1.8e-10, and the certificate refused the extension as not exact. Every point
//! of the result stays on the cylinder, radius 80 about its axis.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::boolean_intersection;
use solidcore::curve::curve::Curve;
use solidcore::kernel::UpstreamFixes;
use solidcore::math::{Point2, Point3, Vec3};
use solidcore::primitive::api_curves as curves;
use solidcore::primitive::api_primitives::create_box;
use solidcore::surface::cone::p1::Cone;
use solidcore::surface::extend::Continuity;
use solidcore::surface::implicit::AnalyticSupport;
use solidcore::surface::side::BoxSide;
use solidcore::surface::surface::Surface;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p1 as bp1;
use solidcore::{Brep, Kernel};

const CENTRE: (f64, f64) = (-90.0, -280.0);
const RADIUS: f64 = 80.0;

fn p(x: f64, y: f64) -> Point3 {
    Point3::new(x, y, 0.0)
}

fn line(a: Point3, b: Point3) -> Curve {
    Curve::from(curves::create_line_segment(a, b).unwrap())
}

/// A ring sector outside the arc from 90 to 99 degrees, swept 94 up, its ends cut off by a
/// box: the cut wall carries its cylinder, with SolidCraft's fix list.
fn wedge(k: &mut Kernel) -> Brep {
    let c = p(CENTRE.0, CENTRE.1);
    let at = |r: f64, deg: f64| {
        let a = f64::to_radians(deg);
        p(CENTRE.0 + r * a.cos(), CENTRE.1 + r * a.sin())
    };
    let profile = [
        Curve::from(curves::create_arc(c, RADIUS, 90.0, 99.0).unwrap()),
        line(at(RADIUS, 99.0), at(120.0, 99.0)),
        line(at(120.0, 99.0), at(120.0, 90.0)),
        line(at(120.0, 90.0), at(RADIUS, 90.0)),
    ];
    let refs: Vec<Option<&Curve>> = profile.iter().map(Some).collect();
    let prism = create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 94.0, true).unwrap();
    let block = create_box(k, Point3::new(-200.0, -300.0, 1.0), 300.0, 300.0, 92.0).unwrap();
    boolean_intersection(k, prism, block).unwrap()
}

#[test]
fn a_short_arc_extends_along_long_rulings_on_its_cylinder() {
    let names: Vec<String> = serde_json::from_str(include_str!("kernel_cases/fixes.json")).unwrap();
    let mut f = UpstreamFixes::default();
    for name in &names {
        if let Some(flag) = f.flag_mut(name) {
            *flag = true;
        }
    }
    let mut k = Kernel {
        fixes: f,
        ..Kernel::default()
    };
    let b = wedge(&mut k);
    let wall = bp1::faces(&b, None)
        .unwrap()
        .into_iter()
        .map(|f| b.face(f).unwrap().surface.as_ref().unwrap())
        .find(|s| {
            matches!(
                AnalyticSupport::of(s),
                Some(AnalyticSupport::Cylinder { radius, .. }) if (radius - RADIUS).abs() < 1e-6
            )
        })
        .expect("the wall is a cylinder");
    // the rulings run along u, so they are extended first and the turn's iso is taken between them
    let dom = wall.natural_uv_domain_with_nurb(&k).unwrap();
    let x0 = wall
        .evaluate_point(&k, Point2::new(dom.min().x, dom.min().y))
        .unwrap();
    let x1 = wall
        .evaluate_point(&k, Point2::new(dom.max().x, dom.min().y))
        .unwrap();
    assert!((x0.z - x1.z).abs() > 90.0, "{x0:?} {x1:?}");
    for d in [500.0, 1500.0] {
        let grown = wall
            .extended_sides_keeping_parameters(&k, &BoxSide::ALL, Continuity::Ruled, d)
            .unwrap()
            .surface;
        let dom = grown.natural_uv_domain_with_nurb(&k).unwrap();
        let (u, v) = (dom.u_interval(), dom.v_interval());
        for i in 0..=8 {
            for j in 0..=8 {
                let uv = Point2::new(
                    u.min() + u.length() * f64::from(i) / 8.0,
                    v.min() + v.length() * f64::from(j) / 8.0,
                );
                let x = grown.evaluate_point(&k, uv).unwrap();
                let r = (x.x - CENTRE.0).hypot(x.y - CENTRE.1);
                assert!((r - RADIUS).abs() < 1e-9, "{d} {i} {j} {r}");
            }
        }
    }
}

/// A cylinder of radius 1, 60 degrees round and 1 high at the origin, extended along its
/// rulings and round its axis by up to 1e5: the frame's size stays the source's (about 1)
/// while the rows grow to the extension, so this is where a limit at the iso's own size
/// could fall under its rounding. Every point stays on the cylinder.
#[test]
fn a_small_arc_extends_far_beyond_its_size_on_its_cylinder() {
    let k = Kernel::default();
    let wall = Surface::from(Cone::new_cylinder_bounded(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        1.0,
        0.0,
        60.0,
        1.0,
        false,
        false,
        false,
    ));
    for d in [1e2, 1e3, 1e4, 1e5] {
        let grown = wall
            .extended_sides_keeping_parameters(&k, &BoxSide::ALL, Continuity::Ruled, d)
            .unwrap_or_else(|e| panic!("{d}: {e}"))
            .surface;
        let dom = grown.natural_uv_domain_with_nurb(&k).unwrap();
        let (u, v) = (dom.u_interval(), dom.v_interval());
        for i in 0..=8 {
            for j in 0..=8 {
                let uv = Point2::new(
                    u.min() + u.length() * f64::from(i) / 8.0,
                    v.min() + v.length() * f64::from(j) / 8.0,
                );
                let x = grown.evaluate_point(&k, uv).unwrap();
                let r = x.x.hypot(x.y);
                assert!(
                    (r - 1.0).abs() < 1e-9 * (1.0 + x.z.abs()),
                    "{d} {i} {j} {r}"
                );
            }
        }
    }
}
