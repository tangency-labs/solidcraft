//! #1580: with `fillet_refuse_rails_leaving_faces` (WR-302), a round whose extended rail
//! leaves its face past an end of the filleted edge, comes back onto it and leaves again
//! was refused with `NoRoom`: `RailsFitTheirFaces` tried `LeavesPastAnEnd` only for the
//! first and the last off-face run, so the middle run (past the end, between the two
//! stretches on the face) was judged as if the rail left the face between the edge's
//! ends. The fork's `518566f` has the same structure.
//!
//! The prism is #1024's with the spur's tip at y = 1.5: the top's rail of a round of 2
//! on the front top edge (y = 2), extended past x = 40, runs off the top, back onto it
//! through the spur (x 54.57 to 55.43) and off again. A middle run that leaves the face
//! between the edge's ends (a V notch in the top reaching down to y = 1.5 at x = 20)
//! is still refused.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_geom::p1::FilletErrorType;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::sweep::api_brep::create_taper_extrude;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::edge;
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

/// The prism of height 10 over `outline` (in the plane z = 0).
fn prism(k: &mut Kernel, outline: &[(f64, f64)]) -> Brep {
    let curves: Vec<Curve> = (0..outline.len())
        .map(|i| {
            let (a, b) = (outline[i], outline[(i + 1) % outline.len()]);
            Curve::from(
                create_line_segment(Vec3::new(a.0, a.1, 0.0), Vec3::new(b.0, b.1, 0.0)).unwrap(),
            )
        })
        .collect();
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_taper_extrude(k, &refs, 10.0, 0.0, 3, false).unwrap()
}

/// The prism of height 10 over the outline (0,0) (40,0) (40,5) (52,5) (55,`tip`) (58,5)
/// (70,5) (70,20) (0,20): a 40 x 20 block whose top goes on past x = 40 behind a step,
/// with a V-shaped spur reaching down to y = `tip` at x = 55.
fn spur_prism(k: &mut Kernel, tip: f64) -> Brep {
    let outline = [
        (0.0, 0.0),
        (40.0, 0.0),
        (40.0, 5.0),
        (52.0, 5.0),
        (55.0, tip),
        (58.0, 5.0),
        (70.0, 5.0),
        (70.0, 20.0),
        (0.0, 20.0),
    ];
    prism(k, &outline)
}

/// A 40 x 20 block of height 10 whose top has a V notch from its back reaching down to
/// y = `tip` at x = 20.
fn notched_block(k: &mut Kernel, tip: f64) -> Brep {
    let outline = [
        (0.0, 0.0),
        (40.0, 0.0),
        (40.0, 20.0),
        (23.0, 20.0),
        (20.0, tip),
        (17.0, 20.0),
        (0.0, 20.0),
    ];
    prism(k, &outline)
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// The front top edge, (0, 0, 10) to (40, 0, 10).
fn front_top_edge(k: &Kernel, b: &Brep) -> EdgeId {
    let near =
        |a: Point3, x: f64| (a.x - x).abs() < 1e-6 && a.y.abs() < 1e-6 && (a.z - 10.0).abs() < 1e-6;
    b.edge_list()
        .iter()
        .copied()
        .find(|&e| {
            let curve = edge::curve(b, e).unwrap().unwrap();
            let ivl = edge::interval(b, e).unwrap();
            let s = curve.evaluate_point(k, ivl.min()).unwrap();
            let t = curve.evaluate_point(k, ivl.max()).unwrap();
            (near(s, 0.0) && near(t, 40.0)) || (near(s, 40.0) && near(t, 0.0))
        })
        .expect("front top edge")
}

#[derive(Debug, PartialEq)]
enum Outcome {
    Built { removed: f64, manifold: bool },
    Refused { code: i32 },
}

/// The front top edge of the body `make` builds rounded 2.
fn round_front_edge(refuse_rails_leaving_faces: bool, make: fn(&mut Kernel) -> Brep) -> Outcome {
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_rails_leaving_faces = refuse_rails_leaving_faces;
    let mut b = make(&mut k);
    let before = volume(&k, &mut b);
    let e = front_top_edge(&k, &b);
    let result = {
        let mut session = FilletSession::new(&mut k, &mut b).unwrap();
        session
            .prepare_constant_radius_edges(&mut k, &[e], 2.0, 1, 1, 1.0)
            .unwrap();
        session
            .do_fillet(&mut k)
            .map_err(|_| session.error_info().code)
    };
    if let Err(code) = result {
        return Outcome::Refused { code };
    }
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    Outcome::Built {
        removed: before - volume(&k, &mut b),
        manifold: is_manifold_solid(&b).unwrap(),
    }
}

/// The round of 2 removes (1 - pi/4) 2^2 40 = 34.336294: it ends flush on the wall
/// x = 40 and the spur is beyond it. Without the flag SMLib (and the pinned C++) builds
/// it; with it, it is now built too (before the fix: `NoRoom`), the same body.
#[test]
fn middle_off_face_run_past_the_edge_end_is_not_refused() {
    let analytic = (1.0 - std::f64::consts::FRAC_PI_4) * 4.0 * 40.0;
    assert!((analytic - 34.336294).abs() < 1e-6);
    for on in [false, true] {
        let outcome = round_front_edge(on, |k| spur_prism(k, 1.5));
        let Outcome::Built { removed, manifold } = outcome else {
            panic!("flag {on}: {outcome:?}");
        };
        assert!(
            manifold && (removed - analytic).abs() <= 1e-6,
            "flag {on}: removed {removed}, expected {analytic}"
        );
    }
}

/// A middle off-face run that leaves the face between the edge's ends (the rail y = 2
/// crosses the notch at x = 20) is still refused with the flag.
#[test]
fn middle_off_face_run_inside_the_edge_span_is_refused() {
    assert_eq!(
        round_front_edge(true, |k| notched_block(k, 1.5)),
        Outcome::Refused {
            code: FilletErrorType::NoRoom as i32
        }
    );
}
