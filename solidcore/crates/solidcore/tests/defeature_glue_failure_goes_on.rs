//! `SmFeatureExecutive::DoDefeature` of face 2 of `Fillet/fillet1019.smb` (the
//! `blend.feature_executive` case `single_face_fillet1019_2`). In the `PiecewiseMerge`
//! of source face 3's extended faces, `GlueCoincidentBrepVertices` fails (a `DeleteEdge`
//! of a non-manifold lamina edge, 1001) after every imprint has worked. Upstream
//! `ImprintIntersections` records the status and `IntersectInsertRelate` goes on
//! (`SmTopologyIntersector.cpp:2387`, `:1264`). Probes on both sides showed the two
//! B-reps in the same state before and after that glue. The port used to count the
//! failing glue as a half-edited B-rep and stop there as for UB-242, so the merge ended
//! early and the feature's patch kept 1 face where the C++ has 4. It now goes on as
//! upstream: no `IntersectInsertRelate` of the rebuild stops on UB-242, and the patch
//! face counts are the C++ reference's (difftest `single_face_fillet1019_2`). That patch
//! leaves lamina edges off the loop, so it does not fill the hole: the defeature is refused
//! and the base put back (WR-656), where upstream returns the base open.
//!
//! The model is the upstream test file, read by the Rust reader when the pinned upstream
//! checkout is present (as the difftest does).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::feature_executive::{FeatureExecutive, RebuildBrepType};
use solidcore::error::defeature::DefeatureFailure;
use solidcore::error::{Reason, SM_ERR};
use solidcore::geom::attribute::AttributeStore;
use solidcore::geom::attribute::ObjectKey;
use solidcore::geom::object::ObjectRef;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, EntityRef, Kernel, SolidError};

/// `Fillet/fillet1019.smb`, read and arena-named as the difftest reader does.
fn fillet1019(k: &mut Kernel) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Fillet/fillet1019.smb");
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
    let key = |_: EntityRef| ObjectRef {
        key: ObjectKey(0),
        type_code: 0,
    };
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
fn defeature_goes_on_past_a_failing_glue() {
    let mut k = Kernel::default();
    let Some(mut b) = fillet1019(&mut k) else {
        return;
    };
    let face = bp1::faces(&b, None).unwrap()[2];
    let mut ex = FeatureExecutive::new(&mut k, &mut b).unwrap();
    ex.add_feature(
        &mut k,
        &[face],
        RebuildBrepType::ExtendSurface,
        RebuildBrepType::FromOtherBrep,
    )
    .unwrap()
    .unwrap();
    // the merge goes on to the C++'s 4-face patch, which leaves lamina edges off the loop
    // (ValidatePatch), so the base patch is not built and the hole would stay open: refused,
    // as WR-656 (the executive drops its features, so the patch's faces are not read back)
    let r = ex.do_defeature(&mut k);
    // refused at the open loop's edge, naming the limit: the faces along it, extended, do
    // not enclose it (#3218); the status is still SM_ERR
    match &r {
        Err(e @ SolidError::Failed(f)) => {
            assert!(
                matches!(
                    f.reason,
                    Reason::Defeature(DefeatureFailure::NotEnclosed { .. })
                ),
                "{r:?}"
            );
            assert!(matches!(f.entity, Some(EntityRef::Edge(_))), "{r:?}");
            assert_eq!(e.sm_status(), Some(SM_ERR), "{r:?}");
        }
        other => panic!("{other:?}"),
    }
    assert!(ex.features().is_empty());
    assert!(
        !k.upstream_ub_passed.contains(&"UB-242"),
        "an IntersectInsertRelate stopped on UB-242: {:?}",
        k.upstream_ub_passed
    );
}
