//! WR-685 and WR-696 on the default kernel (#2273): a variable round is the union of its
//! balls with no flag (the G^n section generator's flag, `fillet_gn_section_generator`, is
//! retired: WR-685 addendum 2026-11-06). Its section is the circle along which the balls'
//! envelope touches the ball, never the great circle through the rails; where its radius
//! law has a corner, each side is its own piece built as the union of the balls there; and
//! a round whose balls fold over one another is refused, naming the radius there.
//!
//! Before, the default kernel swept great circles, which have no law corner to stop at:
//! the 20 cube's top front edge rounded by the linear law 3, 2, 4, 2, 3 came back as a
//! body whose round's net doubles back on itself (`check_brep` #5), and a law whose balls
//! fold failed in the round's surface (1001) with no reason.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::definition_check::{bare_blend_faces, BlendFaceKind, BlendFaceLog};
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_executive::NoRoomReason;
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
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}

/// The edge from `p` to `q`, and whether it runs from `q` to `p`.
fn edge_between(k: &Kernel, b: &Brep, p: Point3, q: Point3) -> (EdgeId, bool) {
    b.edges()
        .ids()
        .find_map(|e| {
            let ivl = edge::interval(b, e).unwrap();
            let c = edge::curve(b, e).unwrap().unwrap();
            let s = c.evaluate_point(k, ivl.min()).unwrap();
            let t = c.evaluate_point(k, ivl.max()).unwrap();
            if s.distance(p) < 1e-9 && t.distance(q) < 1e-9 {
                Some((e, false))
            } else if s.distance(q) < 1e-9 && t.distance(p) < 1e-9 {
                Some((e, true))
            } else {
                None
            }
        })
        .expect("edge")
}

/// The law of degree `degree` through the control radii `r` over `knots` (with their
/// multiplicities `mult`), its y the law's parameter at the Greville abscissae `g`.
fn law(degree: usize, r: &[f64], g: &[f64], knots: &[f64], mult: &[usize]) -> FilletLaw {
    let control: Vec<Vec3> = r
        .iter()
        .zip(g)
        .map(|(&r, &g)| Vec3::new(r, g, 0.0))
        .collect();
    let curve = BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree,
        control_points: &control,
        form: BSplineCurveForm::default(),
        knot_multiplicities: mult,
        knots,
        knot_type: KnotType::default(),
        weights: None,
        trim: None,
    })
    .unwrap();
    FilletLaw::bspline(curve, &Extent1d::from_min_max(0.0, 1.0))
}

/// `b`'s edge from `p` to `q` rounded by the law of control radii `r` from `p` (built by
/// `law_of` from the radii in the edge's own direction): the fillet's error and its typed
/// `NoRoom` reason, if it fails.
fn round(
    k: &mut Kernel,
    b: &mut Brep,
    p: Point3,
    q: Point3,
    r: &[f64],
    law_of: impl Fn(&[f64]) -> FilletLaw,
) -> Result<(), (String, Option<NoRoomReason>)> {
    let (e, reversed) = edge_between(k, b, p, q);
    let mut r = r.to_vec();
    if reversed {
        r.reverse();
    }
    let mut session = FilletSession::new(k, b).unwrap();
    session
        .prepare_variable_radius_edges(k, &[e], r[0], law_of(&r), 1, 1)
        .unwrap();
    session.do_fillet(k).map_err(|err| {
        let info = session.error_info();
        (format!("{err:?}: {}", info.comment), info.no_room_reason())
    })
}

/// The 20 cube's top front edge y = 0, z = 20 rounded by the linear law 3, 2, 4, 2, 3,
/// whose corners at a quarter, a half and three quarters along it dip, peak and dip (the
/// difftest case `blend.fillet_executive_p3` `law-cube20-e0-deg1-two-corners`): the round
/// builds on the default kernel as the union of its balls, a valid body.
#[test]
fn a_law_with_corners_is_its_balls_union_on_the_default_kernel() {
    let k = &mut Kernel::default();
    let mut b = create_box(k, Vec3::ZERO, 20.0, 20.0, 20.0).unwrap();
    let linear = |r: &[f64]| {
        law(
            1,
            r,
            &[0.0, 0.25, 0.5, 0.75, 1.0],
            &[0.0, 0.25, 0.5, 0.75, 1.0],
            &[2, 1, 1, 1, 2],
        )
    };
    let (p, q) = (Point3::new(0.0, 0.0, 20.0), Point3::new(20.0, 0.0, 20.0));
    round(k, &mut b, p, q, &[3.0, 2.0, 4.0, 2.0, 3.0], linear).expect("the round builds");
    stitch_and_orient(k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    let lines = check_brep(k, &mut b, CheckLevel::Level2).unwrap().lines();
    assert!(lines.is_empty(), "check_brep: {lines:?}");
    let removed = 8000.0 - volume(k, &mut b);
    eprintln!("removed {removed}");
    // the corner between the faces less the union of the balls: the radii 2 to 4 take
    // between 0.86 and 3.43 of each unit of the edge (r^2 (1 - pi/4))
    assert!(
        removed > 20.0 * 0.858 && removed < 20.0 * 3.434,
        "{removed}"
    );
}

/// The box 10 x 20 x 30's edge x = 10, z = 30 rounded by the cubic law 2, 2, 40, 3, 2 from
/// y = 0 (the fork's capi test `section_variable_round_spline_folds`): its balls fold over
/// one another, and the default kernel refuses it, naming the radius where the law stops
/// being progressive. Before, the great circles' surface failed (1001), no reason given.
#[test]
fn folding_balls_are_refused_on_the_default_kernel() {
    let k = &mut Kernel::default();
    k.fixes.fillet_trace_reaches_rails = true;
    let mut b = create_box(k, Vec3::ZERO, 10.0, 20.0, 30.0).unwrap();
    let cubic = |r: &[f64]| {
        law(
            3,
            r,
            &[0.0, 1.0 / 6.0, 0.5, 5.0 / 6.0, 1.0],
            &[0.0, 0.5, 1.0],
            &[4, 1, 4],
        )
    };
    let (p, q) = (Point3::new(10.0, 0.0, 30.0), Point3::new(10.0, 20.0, 30.0));
    let (err, reason) = round(k, &mut b, p, q, &[2.0, 2.0, 40.0, 3.0, 2.0], cubic)
        .expect_err("a folded round is refused");
    eprintln!("{err}");
    assert_eq!(reason, Some(NoRoomReason::BallsFold));
    // as with every fix on (wr685_variable_round_touch_circle.rs; the fork's capi test:
    // 19.6356317)
    assert!(
        err.contains("fold over one another where its radius is 19.63563:"),
        "{err}"
    );
}

/// A law built from its control radii.
type LawOf<'a> = &'a dyn Fn(&[f64]) -> FilletLaw;

/// The rounds above are their definitions (review of tangency-labs/solidcore#2936): with
/// SolidCraft's fix set each face the round makes, its pieces and the corner balls' zones
/// between them, is a [`solidcore::surface::procedural_blend::ProceduralBlend`], and none is
/// bare (`blend::definition_check`).
#[test]
fn the_rounds_are_their_definitions_with_every_fix() {
    let linear = |r: &[f64]| {
        law(
            1,
            r,
            &[0.0, 0.25, 0.5, 0.75, 1.0],
            &[0.0, 0.25, 0.5, 0.75, 1.0],
            &[2, 1, 1, 1, 2],
        )
    };
    let dip = |r: &[f64]| law(1, r, &[0.0, 0.5, 1.0], &[0.0, 0.5, 1.0], &[2, 1, 2]);
    let cases: [(&[f64], LawOf, usize); 2] = [
        (&[3.0, 2.0, 4.0, 2.0, 3.0], &linear, 6),
        (&[4.0, 2.0, 4.0], &dip, 3),
    ];
    for (r, law_of, faces) in cases {
        let k = &mut Kernel::default();
        k.fixes = UpstreamFixes::all();
        k.blend_face_log = Some(BlendFaceLog::default());
        let mut b = create_box(k, Vec3::ZERO, 20.0, 20.0, 20.0).unwrap();
        let (p, q) = (Point3::new(0.0, 0.0, 20.0), Point3::new(20.0, 0.0, 20.0));
        round(k, &mut b, p, q, r, law_of).expect("the round builds");
        let made = k.blend_face_log.as_ref().unwrap().last().to_vec();
        // the round's pieces, and a corner ball's zone at each corner of the law
        assert_eq!(made.len(), faces, "{r:?}");
        assert!(made.iter().any(|m| m.kind == BlendFaceKind::Round), "{r:?}");
        for m in &made {
            let s = b.face(m.face).unwrap().surface.as_ref().unwrap();
            assert!(
                s.procedural_blend().is_some(),
                "{r:?}: {:?} is not a definition",
                m.kind
            );
        }
        assert!(bare_blend_faces(&b, &made).unwrap().is_empty(), "{r:?}");
    }
}
