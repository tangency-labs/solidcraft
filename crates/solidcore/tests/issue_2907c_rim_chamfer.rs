//! tangency-labs/solidcore#2907: a tangent chain of seven edges chamfered on a curved face,
//! the rim of SolidCraft's public `examples/regressions/front_triangle_hull_triangle.json`
//! (the body before its chamfer, `data/issue_2907c_hull_rim.smb`). The band check took a rail
//! point far across the face for one inside another blend's band, whose chord direction it
//! only projected on: refused as overlapping at any size past the smallest. The edges are 15
//! to 24 long and the bands 1 to 3 wide, and they do not meet.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::loopuse;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::Brep;

const BODY: &[u8] = include_bytes!("data/issue_2907c_hull_rim.smb");
/// The chain's edges in the body's edge list.
const CHAIN: [usize; 7] = [9, 10, 8, 0, 1, 2, 3];
/// The smallest of the offsets a 1.528 chamfer has at its edges' middles.
const OFFSET: f64 = 2.8673496400813243;

fn kernel() -> Kernel {
    let mut fixes = UpstreamFixes::all();
    for held in [
        "fillet_refuse_rounds_breaking_through",
        "fillet_round_ends_on_outline",
    ] {
        *fixes.flag_mut(held).unwrap() = false;
    }
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = fixes;
    k
}

fn body(k: &mut Kernel) -> Brep {
    let mut attributes = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &loopuse::notify_ref,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut b = read_brep_from_file(k, Some(BODY.to_vec()), false, false, &mut env).unwrap();
    b.refresh_cached_topology();
    b
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// One offset on the chain builds: a clean closed solid with the seven chamfers' faces, and
/// more volume than it had (these edges are concave), none of the refusal. No closed form:
/// the gain is this build's, pinned.
#[test]
fn a_chamfered_rim_on_a_curved_face_builds() {
    let mut k = kernel();
    let mut b = body(&mut k);
    let before = volume(&k, &mut b.clone_standing_for_self().unwrap());
    let faces = b.face_list().len();
    let edges: Vec<_> = CHAIN.iter().map(|&i| b.edge_list()[i]).collect();
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    s.prepare_per_edge_constant_radius_with_second(
        &mut k,
        &edges,
        &[OFFSET; 7],
        &[OFFSET; 7],
        &[FilletSurfaceGeneratorType::Linear],
        1,
        1.0,
    )
    .unwrap();
    s.create_corners(&k, None).unwrap();
    for id in s.fillet_corners().unwrap() {
        s.set_corner_bevel(id, true).unwrap();
    }
    s.do_filleting(&mut k, None).unwrap();
    drop(s);
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    assert!(is_manifold_solid(&b).unwrap());
    assert_eq!(b.face_list().len(), faces + 7);
    let gained = volume(&k, &mut b) - before;
    assert!((gained - 80.05891).abs() < 1e-3, "{gained}");
    let report = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert!(report.is_valid(), "{report:?}");
}
