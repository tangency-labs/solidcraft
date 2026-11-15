//! The SolidCraft fork's tangency-labs/smlib@db4a655 (`fillet_cliff_overflow`, WR-631 /
//! WR-635 / WR-636): a closed rim blended past the far end of its wall is built as if the
//! wall went on, then cut by the face beyond. The disc R13 x 2 with its top rim rounded
//! R2.5 (solidcraft#190): the round's rail on the wall lies 0.5 below the disc's bottom,
//! past the cylinder's domain, so the wall is continued as itself, the rail is one closed
//! edge (one circle), and the removal region closes on the band of the continued wall
//! between the rail and the blended rim.
//!
//! Expected (the fork's capi test `closed_rims_blended_past_their_walls`, the rig's
//! analytic answer): the disc pi 338 less the round's spandrel of revolution,
//! 957.692238945; the fork builds 957.692238947 (this port 957.6922564 at the volume
//! integrator's 1e-6 accuracy). Without the flag the round is refused; with it but without
//! db4a655 it was refused as "lies on no face's exact continuation" (`NotExact`).

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

fn kernel(cliff: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_rails_leaving_faces = true;
    k.fixes.fillet_trace_reaches_rails = true;
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
fn round_disc_rim(cliff: bool, r: f64) -> Result<f64, (i32, String)> {
    let mut k = kernel(cliff);
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
fn db4a655_disc_rim_rounded_past_its_wall_closes_on_the_wall_continued() {
    let built = round_disc_rim(true, 2.5);
    let expected = 957.692_238_945;
    match built {
        Ok(v) => assert!(
            (v - expected).abs() <= 1e-6 * expected,
            "volume {v} against {expected}"
        ),
        Err(e) => panic!("refused: {e:?}"),
    }
}

#[test]
fn db4a655_without_the_flag_the_round_is_refused() {
    assert!(round_disc_rim(false, 2.5).is_err());
}
