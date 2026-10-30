//! A UV trim curve attached to an edge use of an arena-named B-rep is owned by its
//! holding edge use under the identity the B-rep's other trims use (upstream
//! `pUVTrimCurve->SetOwner(this)` in `SmEdgeuse::SetUVTrimCurve`), so `SmEdgeuse`
//! `AssertValid` test #6 ("UVTrimCurve->Owner pointer must equal this Edgeuse pointer")
//! stays clean. Under `NamingScheme::Arena` the new curve used to take the raw
//! `(SmEdgeuse_TYPE, index)` key, and `Brep::use_arena_naming` left the reader's trims
//! under the reader's caller keys, where `check_brep` expects the edge use's
//! notification identity
//! (`plan/findings/topology.edgeuse_p1--arena-uv-trim-owner-identity.md`).
//!
//! The solid is the upstream test file, read by the Rust reader when the pinned upstream
//! checkout is present (as the difftest SMS tests do).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::database_io::FileType;
use solidcore::topology::edgeuse::p1::{set_uv_trim_curve, uv_trim_curve, UvTrimCurveArg};
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::NamingScheme;
use solidcore::{Brep, EntityRef, Kernel};

/// `Solids/box.smb` read by the Rust reader and arena-named, as the difftest reader
/// (`own_reader::read_file`) makes it.
fn arena_box(k: &mut Kernel) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Solids/box.smb");
    let Ok(bytes) = std::fs::read(&path) else {
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

/// The `SmEdgeuse` test #6 lines of a level-2 check.
fn test6_lines(k: &mut Kernel, b: &mut Brep) -> Vec<String> {
    check_brep(k, b, CheckLevel::Level2)
        .unwrap()
        .lines()
        .into_iter()
        .filter(|l| l.contains("Test #6"))
        .collect()
}

#[test]
fn arena_set_uv_trim_curve_owns_the_curve_by_its_holder() {
    let mut k = Kernel::default();
    let Some(mut b) = arena_box(&mut k) else {
        return;
    };
    assert_eq!(b.naming(), NamingScheme::Arena);
    assert!(test6_lines(&mut k, &mut b).is_empty());

    // every trim replaced through SetUVTrimCurve, which stores it on the SAME member of
    // the mated pair (possibly the mate of the slot the reader filled) and owns it by
    // that holder; AssertValid test #6 then checks each owner against its holder
    let uses: Vec<_> = b.edgeuses().ids().collect();
    let mut replaced = 0;
    for eu in uses {
        // (`Curve::clone` is the copy constructor: the copy has no owner, so only
        // SetUVTrimCurve can give it one)
        let fresh = {
            let (c, _) = uv_trim_curve(&b, eu).unwrap();
            let Some(c) = c else {
                continue;
            };
            assert!(c.owner().is_some(), "reader trim without owner on {eu:?}");
            c.clone()
        };
        assert_eq!(fresh.owner(), None);
        set_uv_trim_curve(
            &mut b,
            eu,
            UvTrimCurveArg::New(Box::new(fresh)),
            0.0,
            true,
            None,
        )
        .unwrap();
        let (new, _) = uv_trim_curve(&b, eu).unwrap();
        assert!(
            new.and_then(Curve::owner).is_some(),
            "new trim of {eu:?} without owner"
        );
        replaced += 1;
    }
    assert!(replaced > 0, "box has no UV trim curves");
    assert_eq!(test6_lines(&mut k, &mut b), Vec::<String>::new());
}
