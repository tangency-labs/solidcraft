//! A rounded solid offset deletes an offset face whose lamina edge lies closer to the
//! original surface than the offset distance (`BuildOffsetOfFaceuse`, cpp:3730-3759).
//!
//! `Boolean/extrudeHoles1.smb` outset by 24.7 keeping face 0 as a shell face (the
//! `offset.offset_executive_p2` case `pi_extrudeHoles1_f0_k0.38_out_rnd`), and
//! `Defeature/extrusion.smb` inset by 2.5 keeping face 0: one offset face has a lamina
//! edge point nearer the original surface than 24.7 (resp. 2.5) less twice the
//! approximation tolerance, so upstream deletes it with
//! `SmTopologyIntersector::DeleteFaces`. The port ran that deletion through the kernel
//! context while the face builder held its notification callbacks, so the deletion
//! failed as a nested edit, its status was ignored and the face was kept: the
//! unstitched offset had one face and four edges more than the C++ reference.
//!
//! The expected counts are the pinned C++ reference's.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::Notifications;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::offset::offset_executive::p1::{OffsetExecutive, OffsetOperation};
use solidcore::offset::offset_executive::p2::SolidOffsetOptions;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, EntityRef, Kernel};

/// The upstream test file, read as the difftest runner reads it; `None` (the test is
/// skipped) without the pinned upstream checkout.
fn upstream_part(k: &mut Kernel, name: &str) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles")
        .join(name);
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

/// The unstitched rounded offset (`DoSolidOffset` with `bSkipStitchMerge`) of `name`
/// keeping face 0 as a shell face: its face and edge counts.
fn unstitched_counts(name: &str, distance: f64, tol: f64, inset: bool) -> Option<(usize, usize)> {
    let mut k = Kernel::default();
    let mut part = upstream_part(&mut k, name)?;
    let face0 = bp1::faces(&part, None).unwrap()[0];
    let mut executive = OffsetExecutive::new(OffsetOperation::SolidOffset, distance, tol);
    executive.set_inset(inset);
    executive.set_extend_convex_edges(false);
    executive.set_merge_results(true);
    executive.set_create_offset_solid(false);
    executive.set_shell_faces(&[face0]);
    let mut notifications = Notifications::new(&mut part);
    let result = executive
        .do_solid_offset(
            &mut k,
            Some(&mut part),
            SolidOffsetOptions {
                skip_stitch_merge: true,
            },
            &mut notifications.context(),
        )
        .unwrap();
    Some((
        bp1::faces(&result, None).unwrap().len(),
        bp1::edges(&result, None).unwrap().len(),
    ))
}

#[test]
fn extrude_holes_outset_deletes_the_face_off_its_original() {
    let Some(counts) = unstitched_counts(
        "Boolean/extrudeHoles1.smb",
        24.7,
        0.005_957_493_548_898_4,
        false,
    ) else {
        return;
    };
    assert_eq!(counts, (24, 84), "faces, edges");
}

#[test]
fn extrusion_inset_deletes_the_face_off_its_original() {
    let Some(counts) = unstitched_counts("Defeature/extrusion.smb", 2.5, 1e-4, true) else {
        return;
    };
    assert_eq!(counts, (14, 65), "faces, edges");
}
