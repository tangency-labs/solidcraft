//! The work `boolean_lists` does to make one operand of a list of B-reps, as a count.
//!
//! The N tools of a batch cut are merged into one B-rep, one `MergeBrep` at a time
//! (`boolean::merge::p3::list_operand`). Each merge notifies through a record store that
//! holds a default record for every object of the operand so far. Made from nothing for
//! each merge, that store costs the size of the operand each time, so the N merges visit
//! the square of N slots. [`Kernel::work_counters`] counts every slot, key and record a pass
//! of the store's checks, seedings and resets touches (`merge_record_visits`), the same on
//! every machine.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::PI;

use solidcore::boolean::api_brep::boolean_lists;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::brep_volume_default;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::brep::p1;
use solidcore::Kernel;

const PITCH: f64 = 20.0;

/// The plate `20 n x 20 n x 12` minus `n` x `n` cylinders r 4 through it, in one
/// `boolean_lists` call with SolidCraft's fixes: checks the cut (a wall for each hole,
/// three edges for each hole, the exact volume) and returns the record visits of the
/// merges that made the tools one operand.
fn plate_cut_merge_record_visits(n: u32) -> u64 {
    let mut fixes = UpstreamFixes::all();
    fixes.fillet_refuse_rounds_breaking_through = false;
    fixes.fillet_round_ends_on_outline = false;
    let mut k = Kernel {
        fixes,
        ..Kernel::default()
    };
    let (w, thick, radius) = (PITCH * f64::from(n), 12.0, 4.0);
    let plate = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), w, w, thick).unwrap();
    let mut tools = Vec::new();
    for i in 0..n {
        for j in 0..n {
            let (x, y) = (PITCH * (f64::from(i) + 0.5), PITCH * (f64::from(j) + 0.5));
            let base = Point3::new(x, y, -1.0);
            tools.push(create_cylinder(&mut k, base, radius, thick + 2.0).unwrap());
        }
    }
    let holes = (n * n) as usize;
    k.work_counters.reset();
    let mut cut = boolean_lists(&mut k, &[plate], &[], &tools, &[], 3)
        .unwrap()
        .breps;
    let visits = k.work_counters.merge_record_visits();
    assert_eq!(cut.len(), 1);
    let cut = &mut cut[0];
    assert_eq!(p1::faces(cut, None).unwrap().len(), holes + 6);
    assert_eq!(p1::edges(cut, None).unwrap().len(), 3 * holes + 12);
    let volume = brep_volume_default(&k, cut, 1e-6).unwrap();
    let exact = w * w * thick - holes as f64 * PI * radius * radius * thick;
    println!("plate minus {holes} cylinders: {visits} record visits, volume {volume:.6}");
    assert!(
        (volume - exact).abs() <= 1e-6 * exact,
        "volume {volume}, exact {exact}"
    );
    visits
}

/// Each merge adds one cylinder's objects to the operand and reads that cylinder: its
/// record store needs the new objects' records, not the whole operand's again. So the
/// visits grow with the tools, 930 for each and 866: 24116 for 25 tools and 93866 for 100.
/// Made from nothing for each merge the store cost the operand so far each time, and the
/// visits grew with the square of the tools: 199344 and 3049794.
#[test]
fn list_operand_merges_visit_records_in_proportion_to_the_tools() {
    let visits = (
        plate_cut_merge_record_visits(5),
        plate_cut_merge_record_visits(10),
    );
    assert_eq!(visits, (930 * 25 + 866, 930 * 100 + 866));
}
