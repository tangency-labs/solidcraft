//! #18913: a constant round between two faces that turn about one axis (an exact round
//! class) is a surface of revolution about that axis, so each of its sections lies in a
//! half-plane through the axis: a meridian. `SmSurfOfRevolution`'s validity Test #10
//! checks that, comparing the surface's u = min isoline with its generator turned about
//! the axis.
//!
//! The solver's rail points p0 and p1 (and the end section's) land a little off the
//! half-plane through the axis and the ball's centre, so the arc through them was a
//! twisted generator: on `box_with_large_fillet.smb` edge 4 r 0.1 (goldens
//! `source-file-box_with_large_fillet-edge4`) its angle drifted -2.18e-6 degrees along the
//! section, failing Test #10 on the round face. Both faces turn about the class's axis, so
//! turning a rail point about that axis into the centre's half-plane keeps it on its face;
//! the round's faces now pass Level 2.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::geom::assert_array::{AssertArray, ObjectKey as AssertKey};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::math::Vec3;
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::topology::brep::{p1, p10::TopologyEnv, p7};
use solidcore::topology::database_io::FileType;
use solidcore::topology::Brep;
use solidcore::{EntityRef, Kernel};

/// The goldens' own reader's identities: one distinct key per object.
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

/// `box_with_large_fillet.smb`, arena-named. `None` without the upstream tree (CI's test
/// job has none), failing loudly when `SOLIDCORE_REQUIRE_UPSTREAM` is set.
fn read_body() -> Option<(Kernel, Brep)> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/box_with_large_fillet.smb");
    let Ok(bytes) = std::fs::read(&path) else {
        assert!(
            std::env::var_os("SOLIDCORE_REQUIRE_UPSTREAM").is_none(),
            "SOLIDCORE_REQUIRE_UPSTREAM is set and the upstream test file is missing: {}",
            path.display()
        );
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
    Some((k, body))
}

/// Every face's surface of `body` that fails its validity checks at level 2, with the
/// checks' reports.
fn invalid_surfaces(k: &Kernel, body: &Brep) -> Vec<String> {
    let mut failed = Vec::new();
    for f in p1::faces(body, None).unwrap() {
        let Some(surface) = body.face(f).unwrap().surface.as_ref() else {
            continue;
        };
        let mut list = AssertArray::default();
        if !surface
            .assert_valid(k, Some(&mut list), 2, AssertKey(0))
            .unwrap()
        {
            failed.push(format!("{f:?}: {list:?}"));
        }
    }
    failed
}

/// A cylinder r 5, h 10 with a rim rounded r 1 (goldens `source-cylinder-rim-edge1` and
/// `-edge2`, which the half-plane turn moved): built without the upstream tree, so CI's
/// test job runs it. The round is the torus class about the cylinder's axis.
#[test]
fn cylinder_rim_round_sections_are_meridians() {
    for edge_index in [1, 2] {
        let mut k = Kernel::default();
        let mut body = create_cylinder(&mut k, Vec3::ZERO, 5.0, 10.0).unwrap();
        let edge = p1::edges(&body, None).unwrap()[edge_index];
        fillet_edges(&mut k, &mut body, &[edge], 1.0, 1, 1, 1.0).unwrap();
        let failed = invalid_surfaces(&k, &body);
        assert!(failed.is_empty(), "edge {edge_index}: {failed:#?}");
    }
}

/// Edge 4 rounded r 0.1 (circular section, continuity 1, thumbweight 1, as the goldens
/// case): every face's surface passes its validity checks at level 2, Test #10 among them.
#[test]
fn class_round_sections_are_meridians() {
    let Some((mut k, mut body)) = read_body() else {
        return;
    };
    let edge = p1::edges(&body, None).unwrap()[4];
    fillet_edges(&mut k, &mut body, &[edge], 0.1, 1, 1, 1.0).unwrap();
    let failed = invalid_surfaces(&k, &body);
    assert!(failed.is_empty(), "invalid surfaces: {failed:#?}");
}
