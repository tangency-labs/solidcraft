//! Rounds whose fillet geometry works on an original face's own surface (upstream's
//! `pFace->GetSurface()`, which keeps its face as owner): the face classifications and
//! trimmed surface caches upstream makes through that owner run with the target B-rep
//! at hand.
//!
//! - `SmFilletEdge::CalcCrossSection` on the original face: `sm_CreateUVCurve`'s point
//!   drops classify on the face (`pFace->PointClassify`), difftest
//!   `big-rad-filleted_box-r2.0-top-edges` (`prog_test` `smFilletTest.cpp:1176`).
//! - `SmFilletEdge::CalcFilletIntSideFace` against the side face's surface:
//!   `sm_SrfSrfIntersection`'s general intersector makes the face's trimmed cache,
//!   difftest `stubs4-shell_fillet2-e1-r0.5`.
//!
//! Before the face context was wired both stopped at `Unported` (`SmFace::PointClassify`,
//! `query.face_p2`; `TrimSrfCache::build_tree_with_face`, `surface.trim_srf_cache`). The
//! expected outcomes are the C++ reference's (difftest compares them exactly).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::topology::brep::{p1, p10::TopologyEnv, p7};
use solidcore::topology::database_io::FileType;
use solidcore::topology::Brep;
use solidcore::{EntityRef, Kernel, SolidError};

/// The difftest own reader's identities: one distinct key per object.
fn key_of(e: EntityRef) -> ObjectRef {
    use EntityRef as E;
    let (type_code, kind, i) = match e {
        E::Brep(x) => (16011, 0u64, x.index()),
        E::Region(x) => (16012, 1, x.index()),
        E::Shell(x) => (16013, 2, x.index()),
        E::Faceuse(x) => (16022, 3, x.index()),
        E::Face(x) => (16002, 4, x.index()),
        E::Loopuse(x) => (16004, 5, x.index()),
        E::Loop(x) => (16003, 6, x.index()),
        E::Edgeuse(x) => (16005, 7, x.index()),
        E::Edge(x) => (16016, 8, x.index()),
        E::Vertexuse(x) => (16006, 9, x.index()),
        E::Vertex(x) => (16021, 10, x.index()),
    };
    ObjectRef {
        type_code,
        key: ObjectKey(((kind + 1) << 40) | u64::from(i)),
    }
}

/// Reads the upstream test file and rounds `edges` (by index) with radius `r`
/// (`SmApiFilletEdges`, circular cross section): the result and the face count after
/// it. `None` without the upstream tree (CI's test job has none; the difftest job runs
/// the same cases through `blend.fillet_executive_p3`).
fn round(file: &str, r: f64, edges: &[usize]) -> Option<(Result<(), SolidError>, usize)> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles")
        .join(file);
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("unavailable upstream test file: {}", path.display());
        return None;
    };
    let mut k = Kernel::default();
    let mut body = Brep::new_with_infinite_region().unwrap();
    let mut attributes = AttributeStore::default();
    let mut records = solidcore::topology::topology::NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key_of,
        records: &mut records,
        next_key: 1u64 << 50,
    };
    p7::read_from_file(&mut k, &mut body, bytes, FileType::Ascii, false, &mut env).unwrap();
    body.use_arena_naming().unwrap();
    let all = p1::edges(&body, None).unwrap();
    let selected: Vec<_> = edges.iter().map(|&i| all[i]).collect();
    let result = fillet_edges(&mut k, &mut body, &selected, r, 1, 1, 1.0);
    Some((result, p1::faces(&body, None).unwrap().len()))
}

#[test]
fn cross_section_on_the_original_face_drops_points_with_the_face() {
    let Some((result, faces)) = round(
        "Fillet/filleted_box.smb",
        2.0,
        &[3, 6, 9, 12, 15, 17, 20, 23],
    ) else {
        return;
    };
    result.unwrap();
    assert_eq!(faces, 18);
}

#[test]
fn side_face_intersection_makes_the_face_trimmed_cache() {
    let Some((result, _)) = round("Fillet/shell_fillet2.smb", 0.5, &[1]) else {
        return;
    };
    // the C++ reference fails the round with SM_FAILURE (1001)
    assert!(
        matches!(result, Err(SolidError::Upstream { code: 1001, .. })),
        "{result:?}"
    );
}
