//! tangency-labs/solidcraft#16686: when a round's trace cannot skin its fillet surface, the
//! flush notes the solver's status (`SurfSkinningFailure`) and stops before the rails get
//! their curves. Upstream's `CalcFilletGeom` then returns success with every trace point
//! processed, and a later step fails on the rails it never made (a null pointer, 1006, was
//! how the goldens case `sweep4-native-crash-sphere_diff_3cylinders-e30-r0.1` ended). The
//! solver's geometry step now fails, as `CreateFilletBrep` reports a failed `CalcFilletGeom`:
//! `SM_FILERR_EDGE_PROBLEM` (4) with status 10 at the edge.
//!
//! The case is the goldens one `sweep5-sphere_diff_3cylinders-e29-r1.0`:
//! `Fillet/sphere_diff_3cylinders.smb`, edge 29 rounded r 1 (circular cross section, G1,
//! thumbweight 1). Edge 30 at r 0.1 no longer reaches the skinning: since #16373's
//! projection changes its trace stops earlier, where the fillet touches the base surface's
//! boundary a second time (upstream's own error, solver status left `Unprocessed`; MEASURED),
//! and #16373 moved that row from 1006 to 1001. Edge 29 at r 1 still fails to skin
//! (MEASURED: the flush's error is "the fillet surface could not be skinned"). It is not a
//! regression case for the trace's refusal: with that refusal disabled the case still ends
//! with the same (4, 10) note (MEASURED), and no goldens case now reaches a skinning
//! failure whose outcome the refusal changes. The refusal's own sequences are pinned by
//! `fillet_solver::p4`'s `UnmadeGeometry` and `c4_trace` unit tests.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_geom::p1::{FilStatus, FilletSolverId};
use solidcore::geom::attribute::AttributeStore;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, Kernel};

/// An upstream test file read by the Rust reader, arena-named.
fn read(k: &mut Kernel, rel: &str) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles")
        .join(rel);
    let Ok(bytes) = std::fs::read(&path) else {
        assert!(
            std::env::var_os("SOLIDCORE_REQUIRE_UPSTREAM").is_none(),
            "SOLIDCORE_REQUIRE_UPSTREAM is set and the upstream test file is missing: {}",
            path.display()
        );
        eprintln!("unavailable upstream test file: {}", path.display());
        return None;
    };
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key = solidcore::topology::loopuse::notify_ref;
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key,
        records: &mut records,
        next_key: 1 << 50,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b.use_arena_naming().unwrap();
    Some(b)
}

#[test]
fn a_fillet_surface_that_cannot_be_skinned_fails_the_solver_s_geometry() {
    let mut k = Kernel::default();
    let Some(mut b) = read(&mut k, "Fillet/sphere_diff_3cylinders.smb") else {
        return;
    };
    let edge = p1::edges(&b, None).unwrap()[29];
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    s.prepare_constant_radius_edges(&mut k, &[edge], 1.0, 1, 1, 1.0)
        .unwrap();
    let result = s.do_fillet(&mut k);
    assert!(result.is_err(), "{result:?}");
    // the trace could not skin the fillet surface
    let solver = s.solver(FilletSolverId(0)).unwrap();
    assert_eq!(solver.base().status, FilStatus::SurfSkinningFailure);
    // reported where the solver's geometry is made, not by a later step on unmade rails
    let info = s.error_info();
    assert_eq!((info.code, info.status), (4, 10), "{info:?}");
    assert_eq!(info.comment, "Fillet Surface Computation Failure");
}
