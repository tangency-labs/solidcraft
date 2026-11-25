//! #2706: a variable round whose B-spline radius law is a single point.
//!
//! `FilletLaw::bspline` reads the radius as the x coordinate of its law curve. A law whose
//! control points are all equal, here degree 1 from (3, 0, 0) to (3, 0, 0), is a curve of one
//! point: its x is 3 everywhere, so the law is the constant radius 3. The round of the
//! 10 x 20 x 30 box's edge x = 10, z = 30 (along y, 20 long) with it must be the constant round
//! of 3, which removes `(1 - pi/4) 3^2 20` in closed form.
//!
//! Two kernels: the default one, and the one that builds a round as its definition (every
//! fix on, `UpstreamFixes::all()`, as SolidCraft runs it), where each round face must be a
//! [`ProceduralBlend`](solidcore::surface::procedural_blend::ProceduralBlend) carrying the
//! law, held to the bound its certificate states.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::definition_check::BlendFaceLog;
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_standard_solver::p1::FilletLaw;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent1d::Extent1d;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::p1::{
    BSplineCurve, BSplineCurveForm, CanonicalData, KnotType,
};
use solidcore::primitive::api_primitives::create_box;
use solidcore::surface::procedural_blend::BlendRef;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

/// The accuracy the volumes are integrated to, relative.
const VOLUME_ACCURACY: f64 = 1e-11;

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, VOLUME_ACCURACY, &attrs).unwrap()
}

/// The edge from `p` to `q` (either way).
fn edge_between(k: &Kernel, b: &Brep, p: Point3, q: Point3) -> EdgeId {
    b.edges()
        .ids()
        .find(|&e| {
            let ivl = edge::interval(b, e).unwrap();
            let c = edge::curve(b, e).unwrap().unwrap();
            let s = c.evaluate_point(k, ivl.min()).unwrap();
            let t = c.evaluate_point(k, ivl.max()).unwrap();
            (s.distance(p) < 1e-9 && t.distance(q) < 1e-9)
                || (s.distance(q) < 1e-9 && t.distance(p) < 1e-9)
        })
        .expect("edge")
}

/// The degree 1 law from `a` to `b` over the edge's map [0, 1].
fn linear_law(a: Vec3, b: Vec3) -> FilletLaw {
    let curve = BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: 1,
        control_points: &[a, b],
        form: BSplineCurveForm::default(),
        knot_multiplicities: &[2, 2],
        knots: &[0.0, 1.0],
        knot_type: KnotType::default(),
        weights: None,
        trim: None,
    })
    .unwrap();
    FilletLaw::bspline(curve, &Extent1d::from_min_max(0.0, 1.0))
}

/// Which kernel rounds the box.
#[derive(Clone, Copy, Debug)]
enum Build {
    /// The default kernel.
    Default,
    /// Every fix on: a round is built as its definition.
    Definition,
}

/// A rounded box: its volume, and on the definition kernel the largest bound the round
/// faces' certificates state.
struct Rounded {
    volume: f64,
    bound: f64,
}

/// The box with its edge x = 10, z = 30 rounded with `law` (start radius 3, circular section,
/// continuity 1), checked a closed manifold solid with a clean Level 2 check. For a constant
/// law `constant` on the definition kernel, every face the round made is a procedural blend
/// whose law is that constant (to 1e-12), with no slope, along the edge and past its ends.
fn rounded_box(build: Build, law: FilletLaw, constant: Option<f64>) -> Rounded {
    let mut k = Kernel::default();
    if let Build::Definition = build {
        k.fixes.clone_from(&UpstreamFixes::all());
        k.blend_face_log = Some(BlendFaceLog::default());
    }
    let mut b = create_box(&mut k, Vec3::ZERO, 10.0, 20.0, 30.0).unwrap();
    let e = edge_between(
        &k,
        &b,
        Point3::new(10.0, 0.0, 30.0),
        Point3::new(10.0, 20.0, 30.0),
    );
    {
        let mut session = FilletSession::new(&mut k, &mut b).unwrap();
        session
            .prepare_variable_radius_edges(&mut k, &[e], 3.0, law, 1, 1)
            .unwrap();
        session.do_fillet(&mut k).unwrap();
    }
    assert!(solidcore::topology::brep::p2::is_manifold_solid(&b).unwrap());
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    let mut bound: f64 = 0.0;
    if let (Build::Definition, Some(r)) = (build, constant) {
        let made = k.blend_face_log.as_ref().unwrap().last().to_vec();
        assert!(!made.is_empty(), "the round made no face");
        for m in made {
            let surface = b.face(m.face).unwrap().surface.as_ref().unwrap();
            let Some(BlendRef::Procedural(blend)) = surface.procedural_blend() else {
                panic!(
                    "face {} ({:?}) is not a procedural blend",
                    m.face.index(),
                    m.kind
                );
            };
            bound = bound.max(BlendRef::Procedural(blend).bound());
            // the definition's law, over the leader's parameter, continued as the
            // executive continues it
            let curve = blend.law().expect("the round's law").clone();
            let ivl = curve.natural_interval();
            let mut law = FilletLaw::bspline(curve, &Extent1d::from_min_max(0.0, 1.0));
            let len = ivl.length();
            for f in [-0.25, 0.0, 0.5, 1.0, 1.25] {
                let t = ivl.min() + f * len;
                // the constant to rounding: the definition keeps its own copy of the law
                let v = law.evaluate(t, &ivl, false).unwrap();
                let near = |a: f64, b: f64| (a - b).abs() <= 1e-12 * r;
                assert!(
                    near(v[0], r) && near(v[1], 0.0) && near(v[2], 0.0),
                    "the definition's law at {t}: {v:?}"
                );
            }
        }
    }
    Rounded {
        volume: volume(&k, &mut b),
        bound,
    }
}

/// The constant round of 3 removes `(1 - pi/4) 9 20` of the box's 6000.
fn constant_removed() -> f64 {
    (1.0 - core::f64::consts::FRAC_PI_4) * 9.0 * 20.0
}

/// On the definition kernel the point law's round is the procedural blend of the constant
/// law 3, and removes the constant round's closed form within what its certificate states:
/// the round's area, `(pi / 2) 3 20`, times its bound, plus the volume's accuracy.
#[test]
fn a_single_point_law_is_the_constant_round() {
    let law = linear_law(Vec3::new(3.0, 0.0, 0.0), Vec3::new(3.0, 0.0, 0.0));
    let round = rounded_box(Build::Definition, law, Some(3.0));
    let removed = 6000.0 - round.volume;
    let want = constant_removed();
    let area = core::f64::consts::FRAC_PI_2 * 3.0 * 20.0;
    let tol = area * round.bound + VOLUME_ACCURACY * 6000.0;
    eprintln!(
        "point law removes {removed:.12}, want {want:.12}, off {:.3e}, certified {tol:.3e}",
        removed - want
    );
    assert!(
        (removed - want).abs() <= tol,
        "removed {removed}, want {want}"
    );
}

/// The point law and the law whose curve runs in y, (3, 0, 0) to (3, 1, 0), are the same
/// radius, so they build the same round, on either kernel (before #2706 the point law
/// failed on both: status 1001).
#[test]
fn a_single_point_law_rounds_as_the_graded_constant_law() {
    for build in [Build::Default, Build::Definition] {
        let point = rounded_box(
            build,
            linear_law(Vec3::new(3.0, 0.0, 0.0), Vec3::new(3.0, 0.0, 0.0)),
            Some(3.0),
        );
        let graded = rounded_box(
            build,
            linear_law(Vec3::new(3.0, 0.0, 0.0), Vec3::new(3.0, 1.0, 0.0)),
            Some(3.0),
        );
        let (point, graded) = (6000.0 - point.volume, 6000.0 - graded.volume);
        eprintln!("{build:?}: point {point:.12}, graded {graded:.12}");
        assert!(
            (point - graded).abs() <= 1e-9 * graded,
            "{build:?}: point {point}, graded {graded}"
        );
    }
}

/// The law is continued past its curve's ends as the function it is: the point law is 3,
/// with no slope, before and after the edge.
#[test]
fn a_single_point_law_evaluates_past_its_ends() {
    let mut law = linear_law(Vec3::new(3.0, 0.0, 0.0), Vec3::new(3.0, 0.0, 0.0));
    let ivl = Extent1d::from_min_max(0.0, 20.0);
    for t in [-5.0, 0.0, 10.0, 20.0, 25.0] {
        assert_eq!(
            law.evaluate(t, &ivl, false).unwrap(),
            [3.0, 0.0, 0.0],
            "at {t}"
        );
        assert_eq!(
            law.evaluate(t, &ivl, true).unwrap(),
            [3.0, 0.0, 0.0],
            "at {t}"
        );
    }
}

/// A steep law, 3 down to `r_end` along the edge, continued as its line crosses zero
/// inside the reach the solver evaluates past the edge's end (it traces the run-on half
/// the edge's length past it, and the trace reach looks further): at the station 1.5 the
/// continued radius is below zero. The run-on is trimmed away by the end face, so the
/// round is the one its law gives along the edge: sound, and between the constant rounds
/// of `r_end` and 3, on either kernel.
#[test]
fn a_law_that_turns_negative_past_its_end_rounds_its_edge() {
    for build in [Build::Default, Build::Definition] {
        for r_end in [0.5, 0.05] {
            let law = || linear_law(Vec3::new(3.0, 0.0, 0.0), Vec3::new(r_end, 0.0, 0.0));
            let at_run_on = law()
                .evaluate(30.0, &Extent1d::from_min_max(0.0, 20.0), false)
                .unwrap()[0];
            assert!(at_run_on < 0.0, "{at_run_on}");
            let removed = 6000.0 - rounded_box(build, law(), None).volume;
            let off = |r: f64| (1.0 - core::f64::consts::FRAC_PI_4) * r * r * 20.0;
            eprintln!("{build:?} r_end {r_end}: removes {removed:.12}");
            assert!(
                removed > off(r_end) && removed < off(3.0),
                "{build:?} r_end {r_end}: removed {removed}"
            );
        }
    }
}
