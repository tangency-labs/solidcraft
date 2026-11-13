//! WR-698 (#2273, the fork's cc08236, 308cc40 and c596548): the circle generator builds a
//! constant round between NURBS faces as its definition (`SmGnSectionFSG::CircleSurface`
//! calling `Build` of order 1 with the solver's radius, the rails and uv curves replaced),
//! unless the rails are not the ball's feet, the circle is not exact, or the circles sweep
//! a cylinder or revolve a torus; then the round is built as before.
//!
//! The geometry: a ball of radius 2 between the plane y = 0 (side -1) and a biquadratic
//! bump over z = 0 (its centre control point 0.6 high), along the line (0, 2, 2) ->
//! (10, 2, 2) (a cubic, so that it is not a degree-1 line). The rails are polylines
//! through 41 points of the order-1 blend by its definition on those faces (the feet in
//! the planes x = const), and the uv curves the polylines of their parameters. A solver's
//! rails carry its own error; these are the feet at their knots.
//!
//! The values quoted are the fork's (tangency-labs/smlib@c596548, `SMLib` and `SM_API`
//! built locally with the harness flags): the same inputs built the same way through
//! `SmProceduralBlend::Create` and `EvaluatePoint`, then `Build(..., TRUE, rpSurface, 2.0,
//! &bNotTheFeet)` through a subclass.
//!
//! The certified bounds are solidcore's own since #2559: its intervals take knot
//! differences, factorials and radii outward-rounded, where the fork rounds them to
//! nearest first, so a bound is a few ulps of the part above the fork's. The spans and
//! points are the fork's, bit for bit.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_lossless
)]

use solidcore::blend::fillet_section::section::{ArcSection, GnSectionFsg};
use solidcore::blend::fillet_solver::p1::{FilletSolverType, FilletSurfaceGenerator};
use solidcore::blend::fillet_standard_solver::p2::CircleBall;
use solidcore::geom::vector3d::Point3;
use solidcore::kernel::Kernel;
use solidcore::math::Point2;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use solidcore::nurbs::b_spline_surface::{
    BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData,
};
use solidcore::surface::offset_surface::OffsetSurface;
use solidcore::surface::procedural_blend::{BlendDefinitionInput, ProceduralBlend};
use solidcore::surface::surface::{Surface, SurfaceKind};

const L: f64 = 10.0;
const R: f64 = 2.0;
const N: usize = 40;

fn kernel(definition: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_round_as_definition = definition;
    k
}

fn plane_a() -> Surface {
    Surface::from(
        BSplineSurface::create_bilinear_surface(
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(L, 0.0, 0.0),
            Point3::new(0.0, 0.0, L),
            Point3::new(L, 0.0, L),
        )
        .unwrap(),
    )
}

fn bump() -> Surface {
    let mut cps = Vec::new();
    for i in 0..3 {
        for j in 0..3 {
            let z = if i == 1 && j == 1 { 0.6 } else { 0.0 };
            cps.push(Point3::new(
                f64::from(i) * L / 2.0,
                f64::from(j) * L / 2.0,
                z,
            ));
        }
    }
    Surface::from(
        BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 2,
            v_degree: 2,
            control_points: &cps,
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &[3, 3],
            v_multiplicities: &[3, 3],
            u_knots: &[0.0, 1.0],
            v_knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: None,
            uv_domain: None,
        })
        .unwrap(),
    )
}

fn polyline(dim: u64, pts: &[Point3]) -> BSplineCurve {
    let knots: Vec<f64> = (0..=N).map(|i| i as f64 / N as f64).collect();
    let mut mult = vec![1; N + 1];
    mult[0] = 2;
    mult[N] = 2;
    BSplineCurve::create_canonical(&CanonicalData {
        dim,
        degree: 1,
        control_points: pts,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &mult,
        knots: &knots,
        knot_type: KnotType::Unspecified,
        weights: None,
        trim: None,
    })
    .unwrap()
}

/// The centre line: the leader as a cubic.
fn centre() -> BSplineCurve {
    let cps: Vec<Point3> = (0..4)
        .map(|i| Point3::new(f64::from(i) * L / 3.0, R, R))
        .collect();
    BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: 3,
        control_points: &cps,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &[4, 4],
        knots: &[0.0, 1.0],
        knot_type: KnotType::Unspecified,
        weights: None,
        trim: None,
    })
    .unwrap()
}

/// The rails and uv curves: the order-1 definition's feet at 41 stations.
fn inputs(k: &Kernel) -> [BSplineCurve; 4] {
    let (a, b) = (plane_a(), bump());
    let net = BSplineSurface::create_bilinear_surface(
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
        Point3::new(1.0, 1.0, 0.0),
    )
    .unwrap();
    let leader =
        BSplineCurve::create_line_segment(3, Point3::new(0.0, R, R), Point3::new(L, R, R)).unwrap();
    let seed = BSplineCurve::create_line_segment(
        3,
        Point3::new(0.0, R / L, 0.0),
        Point3::new(1.0, R / L, 0.0),
    )
    .unwrap();
    let input = BlendDefinitionInput {
        supports: [&a, &b],
        sides: [-1.0, 1.0],
        offsets: [R, R],
        law: None,
        order: 1,
        thumbweight: 0.0,
        leader: &leader,
        seeds: [&seed, &seed],
        bound: 1e-3,
        chamfer: None,
    };
    let pb = Surface::from(
        ProceduralBlend::create(k, &net, &input, |n, w| {
            GnSectionFsg::is_valid(n as usize, w)
        })
        .unwrap(),
    );
    let (mut r1, mut r2, mut w1, mut w2) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for i in 0..=N {
        let u = i as f64 / N as f64;
        let p = pb.evaluate_point(k, Point2::new(u, 0.0)).unwrap();
        let q = pb.evaluate_point(k, Point2::new(u, 1.0)).unwrap();
        r1.push(p);
        r2.push(q);
        w1.push(Point3::new(p.x / L, p.z / L, 0.0));
        w2.push(Point3::new(q.x / L, q.y / L, 0.0));
    }
    [
        polyline(3, &r1),
        polyline(3, &r2),
        polyline(2, &w1),
        polyline(2, &w2),
    ]
}

fn offsets(k: &Kernel) -> [Surface; 2] {
    [
        Surface::from(OffsetSurface::new(k, -R, plane_a(), true, 0).unwrap()),
        Surface::from(OffsetSurface::new(k, R, bump(), true, 0).unwrap()),
    ]
}

/// The circle generator's round, as the solver hands it its curves (`create_fillet_surface`).
fn generator_round(
    k: &Kernel,
    generator: &mut FilletSurfaceGenerator,
    radii: [f64; 2],
) -> (Surface, BSplineCurve) {
    let [mut r1, mut r2, mut w1, mut w2] = inputs(k);
    let mut c = centre();
    let [o1, o2] = offsets(k);
    let s = generator
        .create_surface_replacing_uv(
            k,
            1e-3,
            FilletSolverType::ConstRadius,
            Some(&mut c),
            &mut r1,
            &mut r2,
            &[],
            [&o1, &o2],
            [&mut w1, &mut w2],
            Some(&CircleBall {
                radii: Some(radii),
                chord: None,
                chamfer: None,
                ang_tol_rad: 0.1,
                law: None,
            }),
        )
        .unwrap()
        .0;
    (s, r1)
}

fn exact() -> FilletSurfaceGenerator {
    FilletSurfaceGenerator::circular(false, 0.05, false, false)
}

/// `Build` as the gate calls it, with a radius: (surface, bound, spans).
fn build(k: &Kernel, radius: f64) -> (Option<Surface>, f64, usize, BSplineCurve) {
    let [mut r1, mut r2, mut w1, mut w2] = inputs(k);
    let mut c = centre();
    let (a, b) = (plane_a(), bump());
    let mut g = GnSectionFsg::circle(ArcSection {
        approximate_arcs: false,
        ..ArcSection::default()
    });
    let s = g
        .build_given_ball(
            k,
            &mut c,
            [&mut r1, &mut r2],
            1e-3,
            [&a, &b],
            [&mut w1, &mut w2],
            radius,
            None,
        )
        .unwrap();
    (s, g.certified_bound, g.certified_spans, r1)
}

fn net_of(s: &Surface) -> &BSplineSurface {
    match s.kind() {
        SurfaceKind::BSpline(b) => b,
        SurfaceKind::ProceduralBlend(pb) => pb.net(),
        other => panic!("not a net: {other:?}"),
    }
}

/// Prints the case as the fork driver does (`DUMP=1`).
#[test]
fn dump_for_the_fork() {
    if std::env::var("DUMP").is_err() {
        return;
    }
    let k = kernel(true);
    let [r1, r2, w1, w2] = inputs(&k);
    for (name, c) in [("r1", &r1), ("r2", &r2), ("w1", &w1), ("w2", &w2)] {
        for i in [0, 7, 20, 33, 40] {
            let p = c.evaluate_point(i as f64 / N as f64).unwrap();
            println!("in {name} {i} {:.17e} {:.17e} {:.17e}", p.x, p.y, p.z);
        }
    }
    for radius in [2.0, 2.2] {
        let (s, bound, spans, rail) = build(&k, radius);
        println!(
            "case {radius} feet {} bound {bound:.17e} spans {spans}",
            i32::from(s.is_some())
        );
        let Some(s) = s else { continue };
        for &u in &[0.0, 0.37, 1.0] {
            let rp = rail.evaluate_point(u).unwrap();
            println!("rail {u} {:.17e} {:.17e} {:.17e}", rp.x, rp.y, rp.z);
            for j in 0..=4 {
                let v = f64::from(j) / 4.0;
                let p = net_of(&s).evaluate_point(Point2::new(u, v)).unwrap();
                let q = s.evaluate_point(&k, Point2::new(u, v)).unwrap();
                println!(
                    "pt {u} {v} {:.17e} {:.17e} {:.17e} def {:.17e} {:.17e} {:.17e}",
                    p.x, p.y, p.z, q.x, q.y, q.z
                );
            }
        }
    }
}

fn assert_pt(got: Point3, want: [f64; 3], what: &str) {
    assert_eq!([got.x, got.y, got.z], want, "{what}");
}

const RAIL_037: [f64; 3] = [3.700_000_000_000_001_07, 0.0, 2.090_715_071_385_112_81];

/// `Build` with the solver's radius: the fork's certificate, replaced rail and points, bit
/// for bit (240 values in all compared by `dump_for_the_fork`: the inputs at 5 knots, the
/// bounds and spans, the replaced rail and the net's and the definition's points).
#[test]
fn the_ball_s_build_is_the_fork_s() {
    let k = kernel(true);
    let (s, bound, spans, rail) = build(&k, 2.0);
    let s = s.unwrap();
    assert!(matches!(s.kind(), SurfaceKind::ProceduralBlend(_)));
    // the fork's bound 5.403_205_167_109_537_5e-9; tighter here since #13984 (each proved
    // feet's box tightened, the series' structural zeros left out), the spans the fork's
    assert_eq!((spans, bound), (40, 5.402_814_802_230_933e-9));
    assert_pt(rail.evaluate_point(0.37).unwrap(), RAIL_037, "rail (0.37)");
    let uv = Point2::new(0.37, 0.5);
    assert_pt(
        net_of(&s).evaluate_point(uv).unwrap(),
        [
            3.714_471_501_597_201_63,
            6.092_334_483_362_021_88e-1,
            6.535_098_062_054_973_68e-1,
        ],
        "net (0.37, 0.5)",
    );
    assert_pt(
        s.evaluate_point(&k, uv).unwrap(),
        [
            3.714_471_501_486_268_15,
            6.092_334_482_485_654_01e-1,
            6.535_098_064_214_700_52e-1,
        ],
        "definition (0.37, 0.5)",
    );
}

/// A ball of radius 2.2 between the same rails: they are not its feet (308cc40, c596548),
/// nothing is built and the rails are left as they were, as in the fork.
#[test]
fn rails_that_are_not_the_ball_s_feet_build_nothing() {
    let k = kernel(true);
    let (s, bound, spans, rail) = build(&k, 2.2);
    assert!(s.is_none());
    assert_eq!((spans, bound), (0, -1.0));
    let [r1, ..] = inputs(&k);
    let (a, b) = (
        rail.evaluate_point(0.37).unwrap(),
        r1.evaluate_point(0.37).unwrap(),
    );
    assert_eq!([a.x, a.y, a.z], [b.x, b.y, b.z]);
}

fn is_definition(s: &Surface) -> bool {
    matches!(s.kind(), SurfaceKind::ProceduralBlend(_))
}

/// The circle generator, as the solver calls it: the exact circle of a constant ball
/// between NURBS faces is the round by its definition, its rails replaced by the
/// stations' (the same as `Build`'s).
#[test]
fn the_exact_circle_s_round_is_its_definition() {
    let k = kernel(true);
    let (s, rail) = generator_round(&k, &mut exact(), [R, R]);
    assert!(is_definition(&s));
    assert_pt(rail.evaluate_point(0.37).unwrap(), RAIL_037, "rail (0.37)");
}

/// Otherwise the round is built as before (a B-spline, the rails as they were): the
/// approximated circle (asked for explicitly: the cross-section type 1), the flag
/// off, a ball whose rails are not its feet, and two radii.
#[test]
fn otherwise_the_round_is_built_as_before() {
    let before = |k: &Kernel, g: &mut FilletSurfaceGenerator, radii: [f64; 2], what: &str| {
        let (s, rail) = generator_round(k, g, radii);
        assert!(matches!(s.kind(), SurfaceKind::BSpline(_)), "{what}");
        let [r1, ..] = inputs(k);
        let (a, b) = (
            rail.evaluate_point(0.37).unwrap(),
            r1.evaluate_point(0.37).unwrap(),
        );
        assert_eq!([a.x, a.y, a.z], [b.x, b.y, b.z], "{what}");
    };
    let on = kernel(true);
    before(
        &on,
        &mut FilletSurfaceGenerator::circular_default(),
        [R, R],
        "approximated",
    );
    before(&kernel(false), &mut exact(), [R, R], "flag off");
    before(&on, &mut exact(), [2.2, 2.2], "not the feet");
    before(&on, &mut exact(), [R, 2.5], "two radii");
}

/// The quarter circle of radius `r` about the z axis at height `z`, scaled by `a` and moved
/// by `b` in x and y.
fn quarter(dim: u64, r: f64, z: f64, a: f64, b: f64) -> BSplineCurve {
    let p = |x: f64, y: f64| Point3::new(a * x + b, a * y + b, z);
    let w = 0.5_f64.sqrt();
    BSplineCurve::create_canonical(&CanonicalData {
        dim,
        degree: 2,
        control_points: &[p(r, 0.0), p(r, r), p(0.0, r)],
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &[3, 3],
        knots: &[0.0, 1.0],
        knot_type: KnotType::Unspecified,
        weights: Some(&[1.0, w, 1.0]),
        trim: None,
    })
    .unwrap()
}

/// The root of a cylinder of radius 8 on the plane z = 0 (NURBS faces): the circles of
/// the ball of radius 2 revolve a torus, which the round stays.
#[test]
fn a_revolved_torus_stays_the_torus() {
    let k = kernel(true);
    let c = 20.0;
    let grid = |cps: &[Point3], du: usize, w: Option<&[f64]>| {
        Surface::from(
            BSplineSurface::create_canonical(&CanonicalSurfaceData {
                u_degree: du,
                v_degree: 1,
                control_points: cps,
                form: BSplineSurfaceForm::Unspecified,
                u_multiplicities: &[du + 1, du + 1],
                v_multiplicities: &[2, 2],
                u_knots: &[0.0, 1.0],
                v_knots: &[0.0, 1.0],
                knot_type: KnotType::Unspecified,
                weights: w,
                uv_domain: None,
            })
            .unwrap(),
        )
    };
    let plate = grid(
        &[
            Point3::new(-c, -c, 0.0),
            Point3::new(-c, c, 0.0),
            Point3::new(c, -c, 0.0),
            Point3::new(c, c, 0.0),
        ],
        1,
        None,
    );
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
    let cylinder = grid(&cps, 2, Some(&ws));
    let o1 = Surface::from(OffsetSurface::new(&k, R, plate, true, 0).unwrap());
    let o2 = Surface::from(OffsetSurface::new(&k, R, cylinder, true, 0).unwrap());
    let mut centre = quarter(3, 10.0, 2.0, 1.0, 0.0);
    let mut r1 = quarter(3, 10.0, 0.0, 1.0, 0.0);
    let mut r2 = quarter(3, 8.0, 2.0, 1.0, 0.0);
    let mut w1 = quarter(2, 10.0, 0.0, 1.0 / 40.0, 0.5);
    let mut w2 = BSplineCurve::create_line_segment(
        2,
        Point3::new(0.0, 0.2, 0.0),
        Point3::new(1.0, 0.2, 0.0),
    )
    .unwrap();
    let s = exact()
        .create_surface_replacing_uv(
            &k,
            1e-3,
            FilletSolverType::ConstRadius,
            Some(&mut centre),
            &mut r1,
            &mut r2,
            &[],
            [&o1, &o2],
            [&mut w1, &mut w2],
            Some(&CircleBall {
                radii: Some([R, R]),
                chord: None,
                chamfer: None,
                ang_tol_rad: 0.1,
                law: None,
            }),
        )
        .unwrap()
        .0;
    assert!(!is_definition(&s), "{:?}", s.kind());
}
