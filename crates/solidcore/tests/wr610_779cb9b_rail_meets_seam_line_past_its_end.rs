//! The SolidCraft fork's tangency-labs/smlib@779cb9b (`fillet_trace_reaches_rails`,
//! WR-610): `RailEdgeuseIntersect` runs a straight side edge on along its line on a
//! cylinder or cone too (a ruling of it), not only on a plane. A disc R13 x 2 with its top
//! rim rounded R2.5 (solidcraft#190): the wall's rail runs round the cylinder 0.5 below the
//! bottom, and the closed corner's vertex is where it meets the seam's line there. Without
//! the run-on the corner fails (error 3, "Fillet Corner Error: Unable to Compute Geometry",
//! status 1001, the fork's report); with it the corner is computed and the blend goes on to
//! the cliff overflow's construction (which db4a655 completes). The out-of-bounds
//! evaluation of the UV trim along its line (the commit's second half) is
//! `fillet_run_on_bspline_trims` (WR-654), on in both runs.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

fn kernel(cliff: bool, cone_run_on: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_rails_leaving_faces = true;
    k.fixes.fillet_trace_reaches_rails = cone_run_on;
    k.fixes.fillet_rail_end_on_continued_surface = true;
    k.fixes.fillet_run_on_bspline_trims = true;
    k.fixes.offset_extension_from_base = true;
    k.fixes.offset_extension_as_itself = true;
    k.fixes.fillet_cliff_overflow = cliff;
    k
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// The closed edge whose points all lie at height `z`.
fn rim_at(k: &Kernel, b: &Brep, z: f64) -> EdgeId {
    b.edges()
        .ids()
        .find(|&e| {
            let ivl = edge::interval(b, e).unwrap();
            let c = edge::curve(b, e).unwrap().unwrap();
            (0..=8).all(|i| {
                let p: Point3 = c
                    .evaluate_point(k, ivl.evaluate(f64::from(i) / 8.0))
                    .unwrap();
                (p.z - z).abs() < 1e-9
            })
        })
        .expect("rim")
}

/// Rounds the top rim of the disc R13 x 2 by `r`: the volume of the result, or the error
/// code and comment.
fn round_disc_rim(cliff: bool, cone_run_on: bool, r: f64) -> Result<f64, (i32, String)> {
    let mut k = kernel(cliff, cone_run_on);
    let mut b = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 13.0, 2.0).unwrap();
    let rim = rim_at(&k, &b, 2.0);
    let mut session = FilletSession::new(&mut k, &mut b).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &[rim], r, 1, 1, 1.0)
        .unwrap();
    let result = session.do_fillet(&mut k);
    let info = session.error_info().clone();
    drop(session);
    match result {
        Ok(()) => {
            assert!(is_manifold_solid(&b).unwrap(), "not a manifold solid");
            Ok(volume(&k, &mut b))
        }
        Err(_) => Err((info.code, info.comment)),
    }
}

#[test]
fn wr610_779cb9b_disc_rim_corner_meets_the_seam_line_past_its_end() {
    // the trace's reach and the run-on are one flag: without it the round fails at the
    // closed corner, as the fork reports for main
    let before = round_disc_rim(true, false, 2.5);
    assert!(
        matches!(&before, Err((3, c)) if c == "Fillet Corner Error: Unable to Compute Geometry"),
        "{before:?}"
    );
    // with it the corner's rail ends are found on the seam's line below the wall
    let after = round_disc_rim(true, true, 2.5);
    assert!(
        !matches!(&after, Err((3, _))),
        "the corner still fails: {after:?}"
    );
}

/// Within its wall (r 1.5) the round builds either way: 2 pi 169 less the ring the round
/// removes (Pappus: area (1 - pi / 4) r^2 at radius 13 - r (10 - 3 pi) / (12 - 3 pi), its centroid).
#[test]
fn wr610_779cb9b_disc_rim_within_its_wall_builds() {
    let r: f64 = 1.5;
    let pi = std::f64::consts::PI;
    let area = (1.0 - pi / 4.0) * r * r;
    let centroid = 13.0 - r * (10.0 - 3.0 * pi) / (12.0 - 3.0 * pi);
    let expected = pi * 169.0 * 2.0 - 2.0 * pi * centroid * area;
    for run_on in [false, true] {
        let v = round_disc_rim(true, run_on, r).unwrap();
        assert!((v - expected).abs() <= 1e-6 * expected, "{v} vs {expected}");
    }
}
