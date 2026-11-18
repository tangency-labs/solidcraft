//! The rounds that overflow go first, and the rounds that go second meet them as one blend:
//! [`solidcore_ops::blend`] hands its second pass the faces its first made (the kernel's
//! `FilletSession::add_earlier_pass_face`), so a round running on into a first round's end
//! is not capped at a smooth vertex. On SolidCraft's own inputs (the kernel's issue 2364
//! files, its rig's blend-call dumps): the plate (fillets/5158) and the notched block
//! (fillets/1080). Each value is the kernel's own two-pass build of the same rounds
//! (`crates/solidcore/tests/issue_2364_alpha1_ends.rs`); with the cap the plate has 2 pi - 16/3
//! more and the block 15 faces. Its own test binary: the fix set is the process kernel's.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // test helpers

/// #16735's leak checks on a body read into the kernel: test code measuring geometry, which
/// this crate itself does not (docs/layers.md §4).
#[path = "../../solidcore/tests/support/solidcraft_fixes.rs"]
mod solidcraft_fixes;

#[path = "../../solidcore/tests/support/volume_leak.rs"]
#[allow(clippy::disallowed_methods)]
mod volume_leak;

use solidcore::geom::attribute::AttributeStore;
use solidcore::kernel::Kernel;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::loopuse;
use solidcore::topology::topology::NotifyRecords;
use solidcore::Vec3;
use solidcore_ops::{blend, session, Blend, Body, CheckLevel, Corners};

const PLATE: &[u8] =
    include_bytes!("../../solidcore/tests/data/issue_2364_solidcraft_5158_plate.smb");
const NOTCHED: &[u8] =
    include_bytes!("../../solidcore/tests/data/issue_2364_solidcraft_1080_notched_block.smb");

/// SolidCraft's rig set at the time of the kernel's check: every fix but those listed.
fn fixes() {
    let off = [
        "fillet_round_on_round_as_definition",
        "fillet_refuse_rounds_breaking_through",
    ];
    let on = solidcore::kernel::UpstreamFixes::NAMES
        .iter()
        .copied()
        .filter(|n| !off.contains(n));
    session::set_thread_fixes(on).unwrap();
}

/// The index of the edge nearest each point, read as `Body::from_smb` reads the file (the same
/// order).
fn edges_at(bytes: &[u8], at: &[[f64; 3]]) -> Vec<usize> {
    let mut k = Kernel::default();
    let mut attributes = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &loopuse::notify_ref,
        records: &mut records,
        next_key: 1 << 50,
    };
    let b = read_brep_from_file(&mut k, Some(bytes.to_vec()), false, false, &mut env).unwrap();
    at.iter()
        .map(|p| {
            let q = Vec3::new(p[0], p[1], p[2]);
            let d =
                |e| edge_closest_point(&k, &b, e, q).map_or(f64::INFINITY, |c| c.closest.distance);
            let list = b.edge_list();
            (0..list.len())
                .min_by(|&i, &j| d(list[i]).total_cmp(&d(list[j])))
                .unwrap()
        })
        .collect()
}

fn rounded(bytes: &[u8], at: &[[f64; 3]], r: f64) -> (usize, f64, Vec<bool>) {
    fixes();
    let edges = edges_at(bytes, at);
    let b = Body::from_smb(bytes.to_vec(), false).unwrap();
    let made = blend(&b, Blend::Round, &edges, &[r], &[], Corners::Filled).unwrap();
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
    (
        b.counts().unwrap()[0],
        b.volume(1e-8).unwrap(),
        made.dropped,
    )
}

/// The plate's two top edges rounded r 2 in one call: the hole-side edge (-15, 0, 2)
/// overflows and goes first, the other's round meets it as one blend. 8 faces, the kernel's
/// two passes' 1681.903764986 (the closed form 1681.903758348; capped, 1682.853610631) until
/// `brep_volume` closed tolerant edges' gaps (#16735): the 2.0e-8 excess was its leak, and the
/// body is now the closed form to 1.2e-11 (MEASURED).
#[test]
fn the_plate_s_second_round_meets_the_first() {
    let (faces, v, dropped) = rounded(PLATE, &[[-15.0, 0.0, 2.0], [0.0, 15.5, 2.0]], 2.0);
    assert_eq!(faces, 8, "8 as one blend; 10 with the smooth-vertex caps");
    assert!((v - 1_681.903_758_348).abs() <= 1e-10 * v, "{v}");
    assert_eq!(dropped, vec![false, false]);
}

/// The notched block's top outline R8 in one call: the front and right edges (over the
/// notch) go first, then the back and left meet them. 13 faces, 10198.718249145 (capped, 15
/// faces and 10320.299091700) until `brep_volume` closed tolerant edges' gaps (#16735): the
/// 2.1e-8 excess was its leak, and the body is now the closed form 10198.718038816 to 5.8e-12
/// (MEASURED, `issue_2364_alpha1_ends`).
#[test]
fn the_notched_block_s_second_rounds_meet_the_first() {
    let at = [
        [20.0, 0.0, 10.0],
        [40.0, 15.0, 10.0],
        [20.0, 30.0, 10.0],
        [0.0, 15.0, 10.0],
    ];
    let (faces, v, dropped) = rounded(NOTCHED, &at, 8.0);
    assert_eq!(faces, 13, "13 as one blend; 15 with the smooth-vertex caps");
    assert!((v - 10_198.718_038_816).abs() <= 1e-10 * v, "{v}");
    assert_eq!(dropped, vec![false; 4]);
}

/// A round whose G^n section certificate starts with every first span unproved builds within
/// its work budget (SolidCraft's fuzz `atomic_commands`, seed 2: its body `b27` as the rig's
/// export gives it, edges 28 and 22 of the five-edge tangent chain at their record indices,
/// rounded 2.99). Edge 28's face is a 3 mm round already in the body, so the ball's offset tube
/// is 0.01 across, near the focal distance. Before the certificate kept the offset's
/// cancellation there (#16652), its spans stayed unproved: without a budget it certified after
/// 1,271 halvings (about 450 s, measured 2026-11-11) and the operation then failed with status
/// 1001; with this budget it was refused at 123 halvings (165 stations). It now certifies
/// within the budget, and the round builds a valid body. No real round is refused end to end
/// any more: the refusal itself is covered by the kernel's tests on synthetic rounds
/// (`blend::fillet_section::section`, `work_budget_case` and `budget_round`) and by
/// `blend::tests::a_certificate_past_its_budget_is_refused_as_not_certified`. A guard
/// cancels the round after five minutes, so a certificate that stops being held to its budget
/// fails the test instead of running on.
#[test]
fn the_fuzz_round_near_a_focal_distance_builds_within_its_budget() {
    use solidcore_ops::blend::{blend_ordered, BlendOptions};
    use std::io::Read;
    session::set_thread_fixes(solidcraft_fixes::solidcraft_fix_names()).unwrap();
    let gz: &[u8] = include_bytes!("data/atomic_seed2_b27.bodies.json.gz");
    let mut text = String::new();
    flate2::read::GzDecoder::new(gz)
        .read_to_string(&mut text)
        .unwrap();
    let mut records: serde_json::Value = serde_json::from_str(&text).unwrap();
    let record: solidcore_record::brep::Brep =
        serde_json::from_value(records["b27"].take()).unwrap();
    let (input, order) = Body::from_record(&record, true).unwrap();
    let v0 = input.volume(1e-8).unwrap();
    // 5,577.325354466 with the integrator before #16735, which leaked through the tolerant
    // edges' gaps; with them closed it reads 1.3e-5 lower (MEASURED: no closed form). The move
    // is checked on the body: within its old leak bound, and origin-free.
    assert!((v0 - 5_577.325_341_277_863).abs() <= 1e-9 * v0, "{v0}");
    {
        let mut k = Kernel::default();
        let mut b = volume_leak::read_smb(&mut k, &input.to_smb(false).unwrap(), false);
        volume_leak::assert_moved_by_the_old_leak(&k, &mut b, 5_577.325_354_466, v0, 1e-8);
    }
    let (body, order) = input.copy_with_order(&order).unwrap();
    // a guard: a certificate that is not held to its budget is cancelled after five minutes
    // (and the test fails, Cancelled), instead of running on for hours. Held to it, the round
    // built in 74 to 85 s (goldens profile, a loaded 32-thread box, 2026-11-15)
    let token = solidcore_ops::cancel::CancelToken::default();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let guard = {
        let token = token.clone();
        std::thread::spawn(move || {
            if done_rx
                .recv_timeout(std::time::Duration::from_secs(300))
                .is_err()
            {
                token.cancel();
            }
        })
    };
    let r = solidcore_ops::cancel::within(&token, || {
        blend_ordered(
            &body,
            &order,
            Blend::Round,
            &[28, 22],
            &[2.99],
            &[],
            Corners::Filled,
            BlendOptions::default(),
        )
    });
    let _ = done_tx.send(());
    guard.join().unwrap();
    r.unwrap();
    assert!(body.check(CheckLevel::All).unwrap().is_valid());
    assert_eq!(body.counts().unwrap()[0], 16);
    // the round removes 26.371 of 5,577.325. Tolerance chosen, not derived: 1e-6 of the
    // volume (0.0056), far above the volume integral's 1e-8 accuracy and small station moves
    // a later certificate may make, and some 5,000 times below what the round removes
    let v = body.volume(1e-8).unwrap();
    assert!((v - 5_550.953_986_400).abs() <= 1e-6 * v, "{v}");
}
