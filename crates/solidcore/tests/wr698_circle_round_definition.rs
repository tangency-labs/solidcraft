//! WR-698 (#2273, the fork's cc08236): the order-1 section, the ball's exact circle, is
//! built by `SmGnSectionFSG::Build` as the rational quadratic arc between the rails, its
//! rows Hermite curves along s, the arc's rows certified span by span (`sm_ArcRows`,
//! `sm_ArcRowTermsOver`), and with `fillet_round_as_definition` the round is the blend of
//! order 1 by its definition on that net.
//!
//! The geometry: a ball of radius 2 at the root of a cylinder of radius 8 (about the z
//! axis) standing on the plane z = 0, along a quarter turn: the centre line the circle of
//! radius 10 at z = 2, the rails the circles of radius 10 at z = 0 and of radius 8 at
//! z = 2, all the same rational quadratic. The faces are NURBS: the plane a bilinear patch
//! over [-20, 20]^2 and the cylinder the quarter circle swept along z. The round is a
//! quarter of the torus of radii 10 and 2. The square corner of two planes (a straight
//! ball, constant rows) is the second case.
//!
//! The values quoted are the fork's (tangency-labs/smlib@c596548, `SMLib` and `SM_API`
//! built locally with the harness flags, `Build` called on the same inputs through a
//! subclass): the certified bounds and spans, and the net's and the definition's points.

#![allow(clippy::unwrap_used, clippy::panic)]

use solidcore::blend::fillet_section::section::{ArcSection, GnSectionFsg};
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::kernel::Kernel;
use solidcore::math::Point2;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use solidcore::nurbs::b_spline_surface::{
    BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData,
};
use solidcore::surface::surface::{Surface, SurfaceKind};

fn kernel(definition: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_gn_section_generator = true;
    k.fixes.fillet_round_as_definition = definition;
    k
}

fn exact_circle() -> GnSectionFsg {
    GnSectionFsg::circle(ArcSection {
        approximate_arcs: false,
        ..ArcSection::default()
    })
}

/// The quarter circle of radius `r` about the z axis at height `z`, from +x to +y, scaled
/// by `a` and moved by `b` in x and y (an affine image, for the uv curve on the plane).
fn quarter(dim: u64, r: f64, z: f64, a: f64, b: f64) -> BSplineCurve {
    let p = |x: f64, y: f64| Point3::new(a * x + b, a * y + b, z);
    let cps = [p(r, 0.0), p(r, r), p(0.0, r)];
    let w = 0.5_f64.sqrt();
    BSplineCurve::create_canonical(&CanonicalData {
        dim,
        degree: 2,
        control_points: &cps,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &[3, 3],
        knots: &[0.0, 1.0],
        knot_type: KnotType::Unspecified,
        weights: Some(&[1.0, w, 1.0]),
        trim: None,
    })
    .unwrap()
}

fn line(dim: u64, a: [f64; 3], b: [f64; 3]) -> BSplineCurve {
    BSplineCurve::create_line_segment(
        dim,
        Point3::new(a[0], a[1], a[2]),
        Point3::new(b[0], b[1], b[2]),
    )
    .unwrap()
}

fn surface(du: usize, dv: usize, cps: &[Point3], weights: Option<&[f64]>) -> Surface {
    Surface::from(
        BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: du,
            v_degree: dv,
            control_points: cps,
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &[du + 1, du + 1],
            v_multiplicities: &[dv + 1, dv + 1],
            u_knots: &[0.0, 1.0],
            v_knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights,
            uv_domain: None,
        })
        .unwrap(),
    )
}

/// The plane z = 0 over [-20, 20]^2, u along x and v along y.
fn plate() -> Surface {
    let c = 20.0;
    surface(
        1,
        1,
        &[
            Point3::new(-c, -c, 0.0),
            Point3::new(-c, c, 0.0),
            Point3::new(c, -c, 0.0),
            Point3::new(c, c, 0.0),
        ],
        None,
    )
}

/// The cylinder of radius 8 about the z axis, a quarter turn (u) by 10 high (v).
fn cylinder() -> Surface {
    let w = 0.5_f64.sqrt();
    let mut cps = Vec::new();
    let mut ws = Vec::new();
    for (p, wi) in [(8.0, 0.0), (8.0, 8.0), (0.0, 8.0)]
        .into_iter()
        .zip([1.0, w, 1.0])
    {
        for z in [0.0, 10.0] {
            cps.push(Point3::new(p.0, p.1, z));
            ws.push(wi);
        }
    }
    surface(2, 1, &cps, Some(&ws))
}

/// The root round: (surface, bound, spans).
fn root_round(k: &Kernel) -> (Surface, f64, usize) {
    let (f1, f2) = (plate(), cylinder());
    let center = quarter(3, 10.0, 2.0, 1.0, 0.0);
    let mut rail1 = quarter(3, 10.0, 0.0, 1.0, 0.0);
    let mut rail2 = quarter(3, 8.0, 2.0, 1.0, 0.0);
    // on the plate (u, v) = ((x + 20) / 40, (y + 20) / 40); on the cylinder (t, 0.2)
    let mut uv1 = quarter(2, 10.0, 0.0, 1.0 / 40.0, 0.5);
    let mut uv2 = line(2, [0.0, 0.2, 0.0], [1.0, 0.2, 0.0]);
    let mut g = exact_circle();
    let s = g
        .build_with_radius(
            k,
            &center,
            [&mut rail1, &mut rail2],
            1e-3,
            [&f1, &f2],
            [&mut uv1, &mut uv2],
            false,
            2.0,
        )
        .unwrap();
    (s, g.certified_bound, g.certified_spans)
}

/// The square corner: z = 0 and x = 0, the ball of radius 3 along y over [0, 7].
fn corner_round(k: &Kernel) -> (Surface, f64, usize) {
    let plane = |x: Vec3, y: Vec3| {
        Surface::from(solidcore::surface::plane::Plane::new(
            Vec3::ZERO,
            x,
            y,
            solidcore::math::Vec2::new(1.0, 1.0),
            &solidcore::geom::extent2d::Extent2d::from_bounds(-10.0, -10.0, 10.0, 10.0),
        ))
    };
    let (r, len) = (3.0, 7.0);
    let f1 = plane(Vec3::X, Vec3::Y);
    let f2 = plane(Vec3::Y, Vec3::Z);
    let center = line(3, [r, 0.0, r], [r, len, r]);
    let mut rail1 = line(3, [r, 0.0, 0.0], [r, len, 0.0]);
    let mut rail2 = line(3, [0.0, 0.0, r], [0.0, len, r]);
    let mut uv1 = line(2, [r, 0.0, 0.0], [r, len, 0.0]);
    let mut uv2 = line(2, [0.0, r, 0.0], [len, r, 0.0]);
    let mut g = exact_circle();
    let s = g
        .build_with_radius(
            k,
            &center,
            [&mut rail1, &mut rail2],
            1e-6,
            [&f1, &f2],
            [&mut uv1, &mut uv2],
            false,
            0.0,
        )
        .unwrap();
    (s, g.certified_bound, g.certified_spans)
}

fn net_of(s: &Surface) -> &BSplineSurface {
    match s.kind() {
        SurfaceKind::BSpline(b) => b,
        SurfaceKind::ProceduralBlend(pb) => pb.net(),
        other => panic!("not a net: {other:?}"),
    }
}

const US: [f64; 3] = [0.0, 0.37, 1.0];

/// Prints the case as the fork driver does (`DUMP=1`).
fn dump(name: &str, k: &Kernel, s: &Surface, bound: f64, spans: usize) {
    println!("case {name} bound {bound:.17e} spans {spans}");
    for &u in &US {
        for j in 0..=8 {
            let v = f64::from(j) / 8.0;
            let p = net_of(s).evaluate_point(Point2::new(u, v)).unwrap();
            let q = s.evaluate_point(k, Point2::new(u, v)).unwrap();
            println!(
                "pt {u} {v} {:.17e} {:.17e} {:.17e} def {:.17e} {:.17e} {:.17e}",
                p.x, p.y, p.z, q.x, q.y, q.z
            );
        }
    }
}

#[test]
fn dump_for_the_fork() {
    if std::env::var("DUMP").is_err() {
        return;
    }
    let k = kernel(true);
    let (s, b, n) = root_round(&k);
    dump("root", &k, &s, b, n);
    let (s, b, n) = corner_round(&k);
    dump("corner", &k, &s, b, n);
}

fn assert_pt(got: Point3, want: [f64; 3], what: &str) {
    assert_eq!([got.x, got.y, got.z], want, "{what}");
}

/// The fork's certificate and points, bit for bit (436 values in all: the bounds, the
/// spans, and the net's and the definition's points at u in {0, 0.37, 1}, v in 0..=8/8,
/// compared by `dump_for_the_fork` against the fork driver's output).
#[test]
fn the_root_round_is_the_fork_s_order_1_definition() {
    let k = kernel(true);
    let (s, bound, spans) = root_round(&k);
    assert!(matches!(s.kind(), SurfaceKind::ProceduralBlend(_)));
    assert_eq!(spans, 16);
    assert_eq!(bound, 9.034_561_390_531_112_47e-5);
    let net = net_of(&s);
    assert_pt(
        net.evaluate_point(Point2::new(0.37, 0.5)).unwrap(),
        [
            7.224_514_355_152_565_24,
            4.639_192_921_248_099_57,
            5.857_864_376_269_049_66e-1,
        ],
        "net (0.37, 0.5)",
    );
    assert_pt(
        s.evaluate_point(&k, Point2::new(0.37, 0.5)).unwrap(),
        [
            7.224_514_988_698_906_85,
            4.639_193_025_796_938_49,
            5.857_864_376_269_049_66e-1,
        ],
        "definition (0.37, 0.5)",
    );
    assert_pt(
        s.evaluate_point(&k, Point2::new(1.0, 0.25)).unwrap(),
        [0.0, 9.263_810_580_876_253_20, 1.404_233_978_751_393_83e-1],
        "definition (1, 0.25)",
    );
    // the definition is the torus of radii 10 and 2, the net within the certified bound
    for i in 0..=10 {
        for j in 0..=10 {
            let uv = Point2::new(f64::from(i) / 10.0, f64::from(j) / 10.0);
            let q = s.evaluate_point(&k, uv).unwrap();
            let rho = (q.x * q.x + q.y * q.y).sqrt();
            let off = ((rho - 10.0) * (rho - 10.0) + (q.z - 2.0) * (q.z - 2.0)).sqrt();
            assert!((off - 2.0).abs() < 1e-12, "{uv:?}: {off}");
            let p = net.evaluate_point(uv).unwrap();
            assert!(p.distance(q) <= bound, "{uv:?}: {}", p.distance(q));
        }
    }
}

/// Between two planes the arc's rows are constant along s: one span, the bound at
/// rounding, the net the quarter circle of radius 3 about (3, y, 3) (the fork's values).
#[test]
fn the_square_corner_s_circle_is_one_span() {
    let k = kernel(true);
    let (s, bound, spans) = corner_round(&k);
    assert_eq!(spans, 1);
    assert_eq!(bound, 7.869_627_889_322_486_91e-25);
    assert_pt(
        net_of(&s).evaluate_point(Point2::new(1.0, 0.25)).unwrap(),
        [
            1.895_715_871_314_381_57,
            6.999_999_999_999_997_34,
            2.106_350_968_127_090_61e-1,
        ],
        "net (1, 0.25)",
    );
    assert_pt(
        s.evaluate_point(&k, Point2::new(0.37, 0.5)).unwrap(),
        [
            8.786_796_564_403_573_93e-1,
            2.590_000_000_000_000_75,
            8.786_796_564_403_573_93e-1,
        ],
        "definition (0.37, 0.5)",
    );
}

/// Without `fillet_round_as_definition` the same build returns its net as a B-spline.
#[test]
fn flag_off_returns_the_net() {
    let k = kernel(false);
    let (s, bound, spans) = root_round(&k);
    assert!(matches!(s.kind(), SurfaceKind::BSpline(_)));
    assert_eq!((spans, bound), (16, 9.034_561_390_531_112_47e-5));
}

/// An approximated, mirrored or larger arc is not the order-1 section `Build` takes.
#[test]
fn only_the_exact_circle_is_built() {
    let k = kernel(true);
    let (f1, f2) = (plate(), cylinder());
    for arcs in [
        ArcSection::default(),
        ArcSection {
            approximate_arcs: false,
            mirror: true,
            ..ArcSection::default()
        },
        ArcSection {
            approximate_arcs: false,
            complement: true,
            ..ArcSection::default()
        },
    ] {
        let center = quarter(3, 10.0, 2.0, 1.0, 0.0);
        let mut rail1 = quarter(3, 10.0, 0.0, 1.0, 0.0);
        let mut rail2 = quarter(3, 8.0, 2.0, 1.0, 0.0);
        let mut uv1 = quarter(2, 10.0, 0.0, 1.0 / 40.0, 0.5);
        let mut uv2 = line(2, [0.0, 0.2, 0.0], [1.0, 0.2, 0.0]);
        let r = GnSectionFsg::circle(arcs).build_with_radius(
            &k,
            &center,
            [&mut rail1, &mut rail2],
            1e-3,
            [&f1, &f2],
            [&mut uv1, &mut uv2],
            false,
            2.0,
        );
        assert!(
            matches!(r, Err(solidcore::SolidError::InvalidInput { .. })),
            "{arcs:?}: {r:?}"
        );
    }
}
