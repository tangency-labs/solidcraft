//! Public-operation regression for tangency-labs/solidcore#521 / WR-268 and WR-266:
//! what `Kernel::fixes.heal_stage3_failures_propagate` changes in a healed body.
//!
//! `SetProps_Stage3` evaluates the new faces the heal's seam splits make. On the flat
//! cylinder below (radius 1, height `1e-9`) that evaluation fails for the new side
//! face: `SmFace::ClassifyLoops` returns `SM_ERR` (1001) because the face it is handed
//! has no loops to classify. SMLib (and so the default kernel) drops that status and
//! reads the empty classification as `SM_ERR_NOTYET_HEAL_FACE` (1040) from the missing
//! loop tree root, and `Cache_FaceProps_Stage3` quarantines the face. With the opt-in
//! flag the classification status itself is reported (WR-268), so the quarantine does
//! not happen: the face stays a target and `Fix_SplitFaceAtSeams` runs on it, which
//! drops every UV trim curve of the body.
//!
//! The flat cylinder is read from `data/wr266_flat_cylinder.smb`. That file is the body
//! `create_cylinder(origin, 1.0, 1e-9)` built before tangency-labs/solidcraft#15350, written
//! with `write_brep_to_file` (binary, so its geometry is exact). Since #15350
//! `Surface::is_degenerate_curve` is geometric: the side face spans 1e-9 along its height, a
//! degenerate curve, and `create_cylinder` refuses it (1001). The reader rebuilds the B-rep
//! without making faces, so no degeneracy test applies to it (nor would #15512's refusal in
//! `create_face_from_surface`). Read back, the body heals as the built one did. Only the arena
//! order of its edge uses is the file's, which orders `uv_trim_curves` below.
//!
//! The measurement is through public APIs only: the public reader `read_brep_from_file`,
//! the public entry point `api_heal_brep` with every other `Kernel::fixes` field at its
//! default, and the public query `uv_trim_curve_pointer`. Status, entity counts, face,
//! edge and vertex tolerances, the faces that keep a surface and the region void flags
//! are identical in both modes; only the UV trim curves differ.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::geom::attribute::AttributeStore;
use solidcore::heal::heal_data::p1::api_heal_brep;
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p3::topology_ref;
use solidcore::topology::edgeuse::p1::uv_trim_curve_pointer;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, Kernel};

/// The measured body, with every public value that the two modes share, plus the UV
/// trim curves they differ in.
#[derive(PartialEq, Debug)]
struct Healed {
    shells: usize,
    faces: usize,
    loops: usize,
    edges: usize,
    vertices: usize,
    regions: usize,
    faceuses: usize,
    loopuses: usize,
    edgeuses: usize,
    face_tolerance_bits: Vec<u64>,
    faces_with_surface: usize,
    edge_tolerance_bits: Vec<u64>,
    vertex_tolerance_bits: Vec<u64>,
    region_is_void: Vec<i32>,
    /// One flag per edge use, in arena order: does its UV trim curve exist?
    uv_trim_curves: Vec<bool>,
}

/// The flat cylinder (radius 1, height 1e-9), read from the file `create_cylinder` wrote
/// before #15350 (module doc).
fn flat_cylinder(k: &mut Kernel) -> Brep {
    let bytes = include_bytes!("data/wr266_flat_cylinder.smb").to_vec();
    let mut attributes = AttributeStore::new();
    let mut records = NotifyRecords::new();
    let read_key = |e| topology_ref(1, e);
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &read_key,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut b = read_brep_from_file(k, Some(bytes), false, false, &mut env).unwrap();
    b.refresh_cached_topology();
    b
}

fn heal_flat_cylinder(propagate: bool) -> Healed {
    let mut k = Kernel::default();
    // All other fixes stay at their `Default` values.
    k.fixes.heal_stage3_failures_propagate = propagate;
    let mut b = flat_cylinder(&mut k);

    // Public heal operation, whole sequence, no UV trim pass.
    assert_eq!(api_heal_brep(&mut k, Some(&mut b)), Ok(()));

    let sorted_bits = |vals: Vec<f64>| {
        let mut bits: Vec<u64> = vals.into_iter().map(f64::to_bits).collect();
        bits.sort_unstable();
        bits
    };
    Healed {
        shells: b.shells().len(),
        faces: b.faces().len(),
        loops: b.loops().len(),
        edges: b.edges().len(),
        vertices: b.vertices().len(),
        regions: b.regions().len(),
        faceuses: b.faceuses().len(),
        loopuses: b.loopuses().len(),
        edgeuses: b.edgeuses().len(),
        face_tolerance_bits: sorted_bits(
            b.faces()
                .iter()
                .map(|(_, f)| f.tolerance)
                .collect::<Vec<_>>(),
        ),
        faces_with_surface: b
            .faces()
            .iter()
            .filter(|(_, f)| f.surface.is_some())
            .count(),
        edge_tolerance_bits: sorted_bits(
            b.edges()
                .iter()
                .map(|(_, e)| e.tolerance)
                .collect::<Vec<_>>(),
        ),
        vertex_tolerance_bits: sorted_bits(
            b.vertices()
                .iter()
                .map(|(_, v)| v.tolerance)
                .collect::<Vec<_>>(),
        ),
        region_is_void: b.regions().iter().map(|(_, r)| r.is_void()).collect(),
        uv_trim_curves: b
            .edgeuses()
            .iter()
            .map(|(id, _)| match uv_trim_curve_pointer(&b, id) {
                // a read error must not masquerade as an absent trim
                Ok(present) => present.is_some(),
                Err(err) => panic!("uv_trim_curve_pointer({id:?}) errored: {err:?}"),
            })
            .collect(),
    }
}

#[test]
fn wr266_public_heal_stage3_failure_quarantines_the_face_and_keeps_its_uv_trims() {
    let default = heal_flat_cylinder(false);
    let fixed = heal_flat_cylinder(true);

    // The default (SMLib) body: the flat cylinder collapses to 3 faces, 3 loops,
    // 2 edges, 1 vertex and 3 regions, all at the healed tolerance, with four of the
    // eight edge uses carrying a UV trim curve (in the file's edge-use order; the body
    // `create_cylinder` built kept them at [0, 3, 4, 6]).
    let expected_default = Healed {
        shells: 3,
        faces: 3,
        loops: 3,
        edges: 2,
        vertices: 1,
        regions: 3,
        faceuses: 6,
        loopuses: 6,
        edgeuses: 8,
        face_tolerance_bits: vec![f64::to_bits(1e-5); 3],
        faces_with_surface: 3,
        edge_tolerance_bits: vec![f64::to_bits(1e-5); 2],
        vertex_tolerance_bits: vec![f64::to_bits(1e-5)],
        region_is_void: vec![1, 0, 0],
        uv_trim_curves: vec![true, false, true, false, true, false, true, false],
    };
    assert_eq!(default, expected_default, "default heal body changed");

    // WR-268 with the flag on: the same status and the same topology, tolerances and
    // regions, and no UV trim curves at all (the un-quarantined face is passed to
    // `Fix_SplitFaceAtSeams`, which clears them).
    let expected_fixed = Healed {
        uv_trim_curves: vec![false; 8],
        ..expected_default
    };
    assert_eq!(fixed, expected_fixed, "flag-on heal body changed");
}
