//! #16922: a body written to `.smb` reads back the same.
//!
//! A read covers kept UV trim curves that stray from their edge by raising the edge's
//! tolerance (`Brep::trim_cover_raises`, not upstream), and the edge then fits at its
//! tolerance from before the raise. The writer stored the raised value as the edge's own
//! tolerance, so a read of the written file raised it again from there and fitted along
//! the edge at the raised value: coarser trims on the affected faces, and the goldens
//! case `stubs4-delete-edge-sphere_diff_3cylinders-e2-r0.1` (fillet executive) read back
//! with its area moved by 1.0e-7 relative (38235.992674532565 -> 38235.996573337565).
//! The writer now stores a still-held raise as the tolerance before it, so the second
//! read re-derives the same raise from the same trims.
//!
//! `Fillet/sphere_diff_3cylinders.smb` is that case's input: its read raises edges from
//! 1e-5 to about 2.7e-4 (MEASURED).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::topology::api_files::{read_brep_from_file, write_brep_to_file};
use solidcore::topology::brep::p1 as bl;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::loopuse::notify_ref;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{EdgeId, EdgeuseId, FaceId};
use solidcore::{Brep, EntityRef, Kernel};

fn read(k: &mut Kernel, bytes: Vec<u8>) -> Brep {
    let mut attributes = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &notify_ref,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut b = read_brep_from_file(k, Some(bytes), true, false, &mut env).unwrap();
    b.refresh_cached_topology();
    b
}

fn write(k: &Kernel, b: &Brep) -> Vec<u8> {
    let store = AttributeStore::new();
    let records = NotifyRecords::new();
    let key_of = |e: EntityRef| b.owner_identity(e);
    let env = FromBrepEnv {
        attributes: &store,
        key_of: &key_of,
        records: &records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    write_brep_to_file(k, Some(b), &env, true).unwrap()
}

/// Per edge then per vertex in walk order: the tolerance and the cover's raise of it,
/// as bits.
fn tolerances(b: &Brep) -> Vec<(u64, Option<(u64, u64)>)> {
    let raises = b.trim_cover_raises();
    let bits = |r: Option<&(f64, f64)>| r.map(|&(x, y)| (x.to_bits(), y.to_bits()));
    let mut out = Vec::new();
    for e in bl::edges(b, None).unwrap() {
        let t = b.edge(e).unwrap().tolerance;
        out.push((t.to_bits(), bits(raises.edges.get(&e))));
    }
    for v in bl::vertices(b, None).unwrap() {
        let t = b.vertex(v).unwrap().tolerance;
        out.push((t.to_bits(), bits(raises.vertices.get(&v))));
    }
    out
}

#[test]
fn a_written_body_reads_back_with_the_same_tolerances_and_cover_raises() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Fillet/sphere_diff_3cylinders.smb");
    let Ok(bytes) = std::fs::read(&path) else {
        assert!(
            std::env::var_os("SOLIDCORE_REQUIRE_UPSTREAM").is_none(),
            "SOLIDCORE_REQUIRE_UPSTREAM is set and the upstream test file is missing: {}",
            path.display()
        );
        eprintln!("unavailable upstream test file: {}", path.display());
        return;
    };
    let mut k = Kernel::default();
    let b = read(&mut k, bytes);
    assert!(
        !b.trim_cover_raises().edges.is_empty(),
        "the input's read raises edges"
    );
    let first = write(&k, &b);
    let r = read(&mut k, first);
    assert_eq!(tolerances(&r), tolerances(&b));
    // (the written file is not a byte-for-byte fixed point: the read promotes a straight
    // B-spline edge curve to a line, as upstream's reader does, and the second write
    // stores the line; MEASURED, the only difference)
}
