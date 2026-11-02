//! WR-698 (smlib#79 step 7, the fork's 0d43b4c and b8e3594): with
//! `fixes.fillet_gn_section_generator` and `fixes.fillet_round_as_definition` a variable
//! round between NURBS faces is its definition: the order-1 section's `Build` takes the
//! variable radius solver's law (`GnLawRound`), its stations the solver's own, its arcs the
//! circles the balls' envelope touches each ball along, certified span by span with the
//! law in the certificate; the rails and the centre line then run over the edge's t.
//!
//! The round: two square planes, y = 0 and z = 0, along the x axis, a ball of radius
//! `r(t)` (a quadratic law over t in [0, L]) rolling between them, its centre
//! `m(t) = (t, r, r)`, its feet `(t, 0, r)` and `(t, r, 0)`. The touch circle of the ball at
//! t is the sphere `|p - m| = r` cut by the plane `(p - m) . m' = -r r'`, `m' = (1, r', r')`
//! (closed form; both rails lie on it).
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_lossless
)]

use solidcore::blend::fillet_executive::p1::FilletLaw;
use solidcore::blend::fillet_section::section::{ArcSection, GnLawRound, GnSectionFsg};
use solidcore::blend::fillet_solver::p1::{FilletSolverType, FilletSurfaceGenerator};
use solidcore::blend::fillet_standard_solver::p2::{law_in_parameter, CircleBall, LawBall};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::vector3d::Point3;
use solidcore::intersect::surface_intersector::p1::TsectPnt;
use solidcore::kernel::Kernel;
use solidcore::math::Point2;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::surface::offset_surface::OffsetSurface;
use solidcore::surface::surface::{Surface, SurfaceKind};

const L: f64 = 10.0;
const STATIONS: usize = 8;
const TOL: f64 = 1e-3;
/// The law's x control values (degree 2 over [0, L]).
const LAW: [f64; 3] = [1.0, 2.5, 1.5];

fn kernel(definition: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_gn_section_generator = true;
    k.fixes.fillet_round_as_definition = definition;
    k
}

/// r(t) and r'(t), the quadratic Bezier law.
fn r(t: f64) -> (f64, f64) {
    let s = t / L;
    let [a, b, c] = LAW;
    let v = (1.0 - s) * (1.0 - s) * a + 2.0 * s * (1.0 - s) * b + s * s * c;
    let d = (2.0 * (1.0 - s) * (b - a) + 2.0 * s * (c - b)) / L;
    (v, d)
}

fn plane_y0() -> Surface {
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

fn plane_z0() -> Surface {
    Surface::from(
        BSplineSurface::create_bilinear_surface(
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(L, 0.0, 0.0),
            Point3::new(0.0, L, 0.0),
            Point3::new(L, L, 0.0),
        )
        .unwrap(),
    )
}

fn canonical(
    dim: u64,
    degree: usize,
    cps: &[Point3],
    mult: &[usize],
    knots: &[f64],
) -> BSplineCurve {
    BSplineCurve::create_canonical(&CanonicalData {
        dim,
        degree,
        control_points: cps,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: mult,
        knots,
        knot_type: KnotType::Unspecified,
        weights: None,
        trim: None,
    })
    .unwrap()
}

/// The law over t, as `law_in_parameter` makes it from a B-spline law (out of bounds).
fn law_curve() -> BSplineCurve {
    let cps: Vec<Point3> = LAW.iter().map(|&x| Point3::new(x, 0.0, 0.0)).collect();
    let mut c = canonical(3, 2, &cps, &[3, 3], &[0.0, L]);
    c.set_out_of_bounds_enabled(true);
    c
}

/// The edge's curve: the x axis over t in [0, L].
fn leader() -> BSplineCurve {
    let mut c = canonical(
        3,
        1,
        &[Point3::new(0.0, 0.0, 0.0), Point3::new(L, 0.0, 0.0)],
        &[2, 2],
        &[0.0, L],
    );
    c.set_out_of_bounds_enabled(true);
    c
}

/// A quadratic Bezier over [0, 1] whose control points are `f(x, r)` at the law's: the
/// curve through `f(t, r(t))` at c = t / L, exactly (x and r are both quadratic in c).
fn bezier(dim: u64, f: impl Fn(f64, f64) -> Point3) -> BSplineCurve {
    let cps: Vec<Point3> = LAW
        .iter()
        .enumerate()
        .map(|(i, &rv)| f(i as f64 * L / 2.0, rv))
        .collect();
    canonical(dim, 2, &cps, &[3, 3], &[0.0, 1.0])
}

/// The solver's curves over its parameter c = t / L: rails, uv curves, centre line.
fn inputs() -> [BSplineCurve; 5] {
    [
        bezier(3, |t, rv| Point3::new(t, 0.0, rv)),
        bezier(3, |t, rv| Point3::new(t, rv, 0.0)),
        bezier(2, |t, rv| Point3::new(t / L, rv / L, 0.0)),
        bezier(2, |t, rv| Point3::new(t / L, rv / L, 0.0)),
        bezier(3, |t, rv| Point3::new(t, rv, rv)),
    ]
}

/// The solver's stations: t in `user_doubles[0]`, c = t / L the curves' parameter.
fn points() -> Vec<TsectPnt> {
    (0..=STATIONS)
        .map(|i| {
            let c = i as f64 / STATIONS as f64;
            TsectPnt {
                curve_parameter: c,
                user_doubles: [c * L, 0.0, 0.0, 0.0],
                ..TsectPnt::default()
            }
        })
        .collect()
}

struct Round {
    surface: Option<Surface>,
    bound: f64,
    spans: usize,
    rail1: BSplineCurve,
    centre: BSplineCurve,
}

/// `Build` with the law, as the circle generator calls it.
fn build(k: &Kernel) -> Round {
    let [mut r1, mut r2, mut w1, mut w2, mut c] = inputs();
    let (a, b) = (plane_y0(), plane_z0());
    let (lead, law) = (leader(), law_curve());
    let pts = points();
    let mut g = GnSectionFsg::circle(ArcSection {
        approximate_arcs: false,
        ..ArcSection::default()
    });
    let lr = GnLawRound {
        leader: &lead,
        law: &law,
        points: &pts,
    };
    let s = g
        .build_given_ball(
            k,
            &mut c,
            [&mut r1, &mut r2],
            TOL,
            [&a, &b],
            [&mut w1, &mut w2],
            0.0,
            Some(&lr),
        )
        .unwrap();
    Round {
        surface: s,
        bound: g.certified_bound,
        spans: g.certified_spans,
        rail1: r1,
        centre: c,
    }
}

/// How far `p` is off the touch circle at t: (|p - m| - r, (p - m) . m' + r r').
fn off_circle(t: f64, p: Point3) -> (f64, f64) {
    let (rv, rd) = r(t);
    let m = Point3::new(t, rv, rv);
    let dm = Point3::new(1.0, rd, rd);
    let d = p - m;
    (d.length() - rv, d.dot(dm) + rv * rd)
}

#[test]
fn a_variable_round_is_its_definition_on_the_touch_circles() {
    let k = kernel(true);
    let round = build(&k);
    let s = round
        .surface
        .expect("the solver's rails are the balls' feet");
    let SurfaceKind::ProceduralBlend(pb) = s.kind() else {
        panic!("not the definition: {:?}", s.kind());
    };
    assert!(
        round.bound > 0.0 && round.bound <= 0.1 * TOL,
        "{}",
        round.bound
    );
    assert!(round.spans >= STATIONS, "{}", round.spans);
    // the net and the definition run over t, the definition on the touch circles exactly
    for i in 0..=20 {
        let t = i as f64 * L / 20.0;
        for j in 0..=4 {
            let v = j as f64 / 4.0;
            let q = s.evaluate_point(&k, Point2::new(t, v)).unwrap();
            let (dr, dp) = off_circle(t, q);
            assert!(
                dr.abs() < 1e-9 && dp.abs() < 1e-9,
                "def at ({t}, {v}): {dr:e} {dp:e}"
            );
            let p = pb.net().evaluate_point(Point2::new(t, v)).unwrap();
            assert!(
                p.distance(q) <= round.bound,
                "net at ({t}, {v}): {}",
                p.distance(q)
            );
        }
        // the rail is the first foot, the centre line the balls' centres, over t
        let (rv, _) = r(t);
        let rail = round.rail1.evaluate_point(t).unwrap();
        assert!(
            rail.distance(Point3::new(t, 0.0, rv)) < 1e-6,
            "rail at {t}: {rail:?}"
        );
        let m = round.centre.evaluate_point(t).unwrap();
        assert!(
            m.distance(Point3::new(t, rv, rv)) < 1e-6,
            "centre at {t}: {m:?}"
        );
    }
}

/// The circle generator's gate (`definition_surface`), on offset surfaces of the planes,
/// with what it reads off a variable radius solver.
fn generator_round(k: &Kernel, has_edgeuse: bool) -> Surface {
    let [mut r1, mut r2, mut w1, mut w2, mut c] = inputs();
    let o1 = Surface::from(OffsetSurface::new(k, -1.0, plane_y0(), true, 0).unwrap());
    let o2 = Surface::from(OffsetSurface::new(k, 1.0, plane_z0(), true, 0).unwrap());
    let law_x: Vec<Point3> = LAW.iter().map(|&x| Point3::new(x, 0.0, 0.0)).collect();
    let law = FilletLaw::bspline(
        canonical(3, 2, &law_x, &[3, 3], &[0.0, 1.0]),
        &Extent1d::from_min_max(0.0, 1.0),
    );
    let ball = CircleBall {
        radii: None,
        ang_tol_rad: 0.1,
        law: Some(LawBall {
            has_edgeuse,
            edge: Some(leader()),
            law,
            interval: Extent1d::from_min_max(0.0, L),
            reverse: false,
        }),
    };
    let mut g = FilletSurfaceGenerator::circular(false, 0.05, false, false);
    g.create_surface_replacing_uv(
        k,
        TOL,
        FilletSolverType::VariableRadius,
        Some(&mut c),
        &mut r1,
        &mut r2,
        &points(),
        [&o1, &o2],
        [&mut w1, &mut w2],
        Some(&ball),
    )
    .unwrap()
}

#[test]
fn the_circle_generator_builds_an_edge_s_variable_round_as_its_definition() {
    // flag on: the definition (before 0d43b4c a varying ball was never one)
    let k = kernel(true);
    let s = generator_round(&k, true);
    assert!(
        matches!(s.kind(), SurfaceKind::ProceduralBlend(_)),
        "{:?}",
        s.kind()
    );
    let q = s.evaluate_point(&k, Point2::new(0.37 * L, 0.5)).unwrap();
    let (dr, dp) = off_circle(0.37 * L, q);
    assert!(dr.abs() < 1e-9 && dp.abs() < 1e-9, "{dr:e} {dp:e}");
    // a corner's variable ball (no edge use) stays as before (b8e3594)
    let corner = generator_round(&k, false);
    assert!(
        matches!(corner.kind(), SurfaceKind::BSpline(_)),
        "{:?}",
        corner.kind()
    );
    // flag off: the skinned circles, as upstream
    let off = generator_round(&kernel(false), true);
    assert!(
        matches!(off.kind(), SurfaceKind::BSpline(_)),
        "{:?}",
        off.kind()
    );
}

#[test]
fn the_law_in_the_edge_s_parameter_is_the_solver_s() {
    let ivl = Extent1d::from_min_max(2.0, 6.0);
    let law_x: Vec<Point3> = LAW.iter().map(|&x| Point3::new(x, 0.0, 0.0)).collect();
    let cases = [
        FilletLaw::linear(1.0, 2.0, None),
        FilletLaw::bspline(
            canonical(3, 2, &law_x, &[3, 3], &[0.0, 1.0]),
            &Extent1d::from_min_max(0.25, 0.75),
        ),
    ];
    for law in cases {
        for reverse in [false, true] {
            let lb = LawBall {
                has_edgeuse: true,
                edge: None,
                law: law.clone(),
                interval: ivl,
                reverse,
            };
            let c = law_in_parameter(&lb).unwrap().unwrap();
            let mut solver_law = law.clone();
            for i in 0..=8 {
                let t = 2.0 + 0.5 * i as f64;
                let want = solver_law.evaluate(t, &ivl, reverse).unwrap()[0];
                let got = c.evaluate_point(t).unwrap().x;
                assert!(
                    (got - want).abs() <= 1e-12 * want.abs(),
                    "{reverse} {t}: {got} {want}"
                );
            }
        }
    }
}
