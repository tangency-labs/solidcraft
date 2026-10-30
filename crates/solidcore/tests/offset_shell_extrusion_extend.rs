//! An extended solid offset of `Defeature/extrusion.smb` keeping face 0 as a shell face,
//! inset by 2 (the `offset.offset_executive_p2` case `pa_Defeature_extrusion_f0_d2_in_ext`).
//!
//! Inset by 2, a round of the part offsets to a cylinder that the lines its edges were
//! lifted to no longer lie on. `MapEdgeuses` then builds those edges' UV trim curves by
//! dropping 101 points (`CreateUVTrimCurve`'s last resort), whose distance to the surface
//! (`sm_CalcDistAndDev`, about 1.69) raises the edge tolerances. The port measured -1 for
//! every point (its solve returned the solutions without the surface they lie on), so
//! the edges kept their tolerance, the stitch that closes the offset glued fewer edges
//! and the result kept five vertices more than the C++ reference.
//!
//! The expected counts and bounds are the pinned C++ reference's.
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
fn extrusion(k: &mut Kernel) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Defeature/extrusion.smb");
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

#[test]
fn inset_extended_offset_keeps_the_reference_topology() {
    let mut k = Kernel::default();
    let Some(mut part) = extrusion(&mut k) else {
        return;
    };
    let face0 = bp1::faces(&part, None).unwrap()[0];
    let mut executive = OffsetExecutive::new(OffsetOperation::SolidOffset, 2.0, 1e-4);
    executive.set_inset(true);
    executive.set_extend_convex_edges(true);
    executive.set_merge_results(false);
    executive.set_create_offset_solid(false);
    executive.set_shell_faces(&[face0]);
    let mut notifications = Notifications::new(&mut part);
    let result = executive
        .do_solid_offset(
            &mut k,
            Some(&mut part),
            SolidOffsetOptions::default(),
            &mut notifications.context(),
        )
        .unwrap();

    let faces = bp1::faces(&result, None).unwrap();
    let edges = bp1::edges(&result, None).unwrap();
    let vertices = bp1::vertices(&result, None).unwrap();
    assert_eq!(
        (faces.len(), edges.len(), vertices.len()),
        (11, 34, 14),
        "faces, edges, vertices"
    );
    // The C++ result's least vertex x is 3.999999999999994: no vertex is left on the
    // x = 2 offset of the round (the port's was 2).
    let min_x = vertices
        .iter()
        .map(|&v| result.vertex(v).unwrap().point.x)
        .fold(f64::INFINITY, f64::min);
    assert!((min_x - 4.0).abs() < 1e-9, "least vertex x {min_x}");
}
