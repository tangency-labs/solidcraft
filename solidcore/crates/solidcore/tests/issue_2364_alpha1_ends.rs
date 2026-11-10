//! The fork's smlib#130 (`v1.1.0-alpha.1`) fixes to the End at a smooth vertex (smlib#124,
//! WR-697), on SolidCraft's own inputs: the bodies its rig handed the
//! fillet (`data/issue_2364_solidcraft_*.smb`, the rig's blend-call dumps, perf-inputs
//! 2364-ends).
//!
//! - a763ac1: an edge whose faces meet tangentially (the slotted bar's flat top running onto
//!   its round end, SolidCraft fillets/3369) ends no tangent chain, and the round ends on the
//!   end face past it. Before, its R2 round ended there with two smooth-vertex caps on the
//!   slot's walls: 13 faces, the same volume. Now 11 faces.
//! - 9822462: a round that goes second (SolidCraft's `overflow_first`: the rounds that
//!   overflow their faces go first, then what is left of the others) meets the rounds that
//!   went first as one blend. The caller hands the second pass the faces the first made
//!   (`FilletSession::add_earlier_pass_face`, the fork's `AddEarlierPassFace`), and
//!   `EndAtSmoothVertex` takes no edge of theirs. Before, the second round was capped where
//!   it ran on into the first round's end arc, leaving a shoulder of r^3 (pi/4 - 2/3) more
//!   material at each such corner. SolidCraft fillets/5158 (the plate's two edges rounded
//!   r 2, the plate's own thickness) and fillets/1080 (the notched block's top outline R8).
//!
//! Every value is the fork's at 9822462 (`alpha-1-fixes`, built with `-ffp-contract=off`),
//! the same calls through `SmFilletExecutive` (a circular section at constant radius,
//! continuity 1, thumbweight 1; for the second pass, `AddEarlierPassFace` with each face
//! the first pass made), the volume by `SmApiBrepComputeVolume` at 1e-8.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{Kernel, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{loopuse, EdgeId, FaceId};
use solidcore::Brep;

const SLOT_BAR: &[u8] = include_bytes!("data/issue_2364_solidcraft_3369_slot_bar.smb");
const PLATE: &[u8] = include_bytes!("data/issue_2364_solidcraft_5158_plate.smb");
const NOTCHED: &[u8] = include_bytes!("data/issue_2364_solidcraft_1080_notched_block.smb");

/// SolidCraft's rig set at the time (#2364's check): every fix but the four it leaves off.
fn kernel() -> Kernel {
    let mut f = UpstreamFixes::all();
    f.cone_unbounded_parameterized_as_nurb = false;
    f.ellipse_open_arc_keeps_end_angle = false;
    f.fillet_overflow_crease_along_edge = false;
    f.fillet_refuse_rounds_breaking_through = false;
    Kernel {
        fixes: f,
        ..Kernel::default()
    }
}

/// A SolidCraft file, read under the notification naming as SolidCraft reads it.
fn read(k: &mut Kernel, bytes: &[u8]) -> Brep {
    let mut attributes = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &loopuse::notify_ref,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut b = read_brep_from_file(k, Some(bytes.to_vec()), false, false, &mut env).unwrap();
    b.refresh_cached_topology();
    b
}

/// The edge nearest `p`.
fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (f64::INFINITY, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance < best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap()
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// One pass: the edges at `at` rounded `r` in one session, `earlier` handed to it as the
/// faces an earlier pass made; returns the faces this pass made.
fn round(k: &mut Kernel, b: &mut Brep, at: &[[f64; 3]], r: f64, earlier: &[FaceId]) -> Vec<FaceId> {
    let before: Vec<FaceId> = b.face_list().to_vec();
    let edges: Vec<EdgeId> = at.iter().map(|&p| pick(k, b, p)).collect();
    {
        let mut s = FilletSession::new(k, b).unwrap();
        s.prepare_per_edge_constant_radius(
            k,
            &edges,
            &vec![r; edges.len()],
            &[FilletSurfaceGeneratorType::Circular],
            1,
            1.0,
        )
        .unwrap();
        for &f in earlier {
            s.add_earlier_pass_face(f);
        }
        s.create_corners(k, None).unwrap();
        s.do_filleting(k, None).unwrap();
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default()).unwrap();
    b.refresh_cached_topology();
    b.face_list()
        .iter()
        .copied()
        .filter(|f| !before.contains(f))
        .collect()
}

/// The result's face count and volume, checked one manifold solid.
fn measured(k: &Kernel, b: &mut Brep) -> (usize, f64) {
    assert!(is_manifold_solid(b).unwrap());
    (b.face_list().len(), volume(k, b))
}

/// 14 (475 + 90.25 pi) - 3 (90.25 pi) - 24 (1 - pi/4): the bar, less the slot and the round
/// (the fork's a763ac1).
#[test]
fn a_round_ending_on_a_tangent_seam_ends_on_the_end_face() {
    let mut k = kernel();
    let mut b = read(&mut k, SLOT_BAR);
    round(&mut k, &mut b, &[[6.5, 0.0, 9.5]], 2.0, &[]);
    let (faces, v) = measured(&k, &mut b);
    assert_eq!(faces, 11, "the fork at 9822462: 11 (13 with the two caps)");
    // the fork: 9763.665662831 (here 9763.665662831354)
    assert!((v - 9_763.665_662_831).abs() <= 1e-9 * v, "{v}");
}

/// The plate's hole-side edge (-15, 0, 2) overflows and goes first; the other edge's round
/// then meets it as one blend. The fork: 1681.903764986359 (the closed form 1681.903758348;
/// with the cap, 1682.853610631, the shoulder 2 pi - 16/3 more). Here 1681.903764966137,
/// 2.0e-8 less: the first pass already measures 1706.889016313 against the fork's
/// 1706.889016333346 (that round runs into the hole, a fitted band), as before smlib#130.
#[test]
fn the_plate_s_second_round_meets_the_first() {
    let mut k = kernel();
    let mut b = read(&mut k, PLATE);
    let first = round(&mut k, &mut b, &[[-15.0, 0.0, 2.0]], 2.0, &[]);
    round(&mut k, &mut b, &[[0.0, 15.5, 2.0]], 2.0, &first);
    let (faces, v) = measured(&k, &mut b);
    assert_eq!(faces, 8);
    assert!((v - 1_681.903_764_986).abs() <= 1e-9 * v, "{v}");
}

/// The notched block's front and right edges (over the notch) go first, then the back and
/// left. The fork: 10198.718249145, 13 faces (here 10198.718249145044) (the closed form 10198.718038816; with the
/// caps, 10320.299091700, 15 faces, two shoulders 256 pi - 2048/3 more).
#[test]
fn the_notched_block_s_second_rounds_meet_the_first() {
    let mut k = kernel();
    let mut b = read(&mut k, NOTCHED);
    let first = round(
        &mut k,
        &mut b,
        &[[20.0, 0.0, 10.0], [40.0, 15.0, 10.0]],
        8.0,
        &[],
    );
    round(
        &mut k,
        &mut b,
        &[[20.0, 30.0, 10.0], [0.0, 15.0, 10.0]],
        8.0,
        &first,
    );
    let (faces, v) = measured(&k, &mut b);
    assert_eq!(faces, 13);
    assert!((v - 10_198.718_249_145).abs() <= 1e-9 * v, "{v}");
}
