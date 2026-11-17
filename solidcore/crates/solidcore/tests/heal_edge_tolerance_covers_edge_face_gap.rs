//! The healer's tolerance fix (`Fix_TolSizes`) gives an edge a tolerance that covers its
//! edge/face gap.
//!
//! Upstream sets a fixed edge's tolerance to `gain * gap - def` (`SmTol::CalcEdgeZoneTol3d`),
//! which is under the gap itself for gaps below ten times the default. The check's edge/face
//! test (`SmBrep` Test #4) then refuses the edge it has just fixed. Two corpus files read with
//! healing show it: on `Fillet/sphere_plane.smb` the plane/sphere circle edge is 1.69e-5 from
//! the sphere with a tolerance of 1e-5. On `Solids/revolution_mold.smb` two edges have gaps of
//! 2.99e-5 and 9.25e-5, with tolerances of 2.29e-5 and 9.17e-5.
//!
//! The kept NURB of WR-493 also brings this out. The found sphere evaluates the file's own
//! UV trims, so they are no longer dropped and rebuilt with a grown tolerance.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::geom::attribute::AttributeStore;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::database_io::FileType;
use solidcore::topology::edgeuse::p1::max_edge_face_gap;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, Kernel};

/// Reads an upstream `.smb` test file (healing on, as `SmBrep::ReadFromFile`).
fn read(k: &mut Kernel, name: &str) -> Brep {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles")
        .join(name);
    let bytes = std::fs::read(&path).unwrap();
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
    b
}

/// Every loop edge use's edge/face gap is within its edge's tolerance, and the check
/// reports no edge/face gap.
fn assert_gaps_covered(name: &str) {
    let mut k = Kernel::default();
    let mut b = read(&mut k, name);
    let eus: Vec<_> = b.edgeuses().iter().map(|(id, _)| id).collect();
    for eu in eus {
        let Some(e) = b.edgeuse(eu).unwrap().edge else { continue };
        let Some(gap) = max_edge_face_gap(&k, &mut b, eu, false).unwrap() else { continue };
        let tol = b.edge(e).unwrap().tolerance;
        assert!(gap.gap_3d <= tol, "{name}: {eu:?} of {e:?}: gap {:e} over tolerance {tol:e}", gap.gap_3d);
    }
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap().lines();
    let lines: Vec<_> = lines.iter().filter(|l| l.contains("Edge/Surface gap")).collect();
    assert!(lines.is_empty(), "{name}: {lines:?}");
}

#[test]
fn sphere_plane_circle_edge_covers_its_gap() {
    assert_gaps_covered("Fillet/sphere_plane.smb");
}

#[test]
fn revolution_mold_edges_cover_their_gaps() {
    assert_gaps_covered("Solids/revolution_mold.smb");
}
