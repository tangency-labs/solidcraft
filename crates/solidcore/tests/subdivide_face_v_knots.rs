//! `SmBrep::SubdivideFaceAtDiscontinuities` cuts a face along every knot of its surface:
//! its V pass counts the V knots, where upstream counts the U knots (`SmBrep.cpp:18483`,
//! WR-589, fixed on the default path). The fixtures are unit-square sheets on degree-1
//! B-spline surfaces (`fixtures/topology.brep_p10/subdivide_*.smb`), subdivided at every
//! interior knot (`SM_CT_CINFINITY`). A degree-1 sheet's knots are at its control
//! points, so the cuts are the grid lines through them (closed form):
//! - one U knot and two V knots (x = 0.3; y = 0.2 and 0.7): 6 faces, where upstream
//!   cuts at the U knot and the first V knot only (4);
//! - no U knot and one V knot (y = 0.3): 2 faces, where upstream makes no cut (1);
//! - one U knot and no V knot (x = 0.3): 2 faces, where upstream reads past its V knots.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::geom_utility::p2::ContinuityType;
use solidcore::geom::object::ObjectRef;
use solidcore::topology::brep::p1::faces;
use solidcore::topology::brep::p10::{subdivide_face_at_discontinuities, TopologyEnv};
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::edge::NotifyEnv;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{EdgeId, FaceId};
use solidcore::{Brep, EntityRef, Kernel, SolidError};
use std::collections::BTreeSet;

/// A distinct identity per entity kind and index.
fn key_of(e: EntityRef) -> ObjectRef {
    let (kind, index) = match e {
        EntityRef::Brep(x) => (0, x.index()),
        EntityRef::Region(x) => (1, x.index()),
        EntityRef::Shell(x) => (2, x.index()),
        EntityRef::Faceuse(x) => (3, x.index()),
        EntityRef::Face(x) => (4, x.index()),
        EntityRef::Loopuse(x) => (5, x.index()),
        EntityRef::Loop(x) => (6, x.index()),
        EntityRef::Edgeuse(x) => (7, x.index()),
        EntityRef::Edge(x) => (8, x.index()),
        EntityRef::Vertexuse(x) => (9, x.index()),
        EntityRef::Vertex(x) => (10, x.index()),
    };
    ObjectRef {
        type_code: 0,
        key: ObjectKey((1u64 << 40) * (kind + 1) + u64::from(index)),
    }
}

/// What a subdivision leaves: the result, the number of faces, and the distinct x and y
/// of the vertices (to 1e-12, as integers of 1e-12).
struct Subdivided {
    result: Result<Vec<FaceId>, SolidError>,
    faces: usize,
    xs: BTreeSet<i64>,
    ys: BTreeSet<i64>,
}

/// `v` in units of 1e-12.
fn grid(v: f64) -> i64 {
    (v * 1e12).round() as i64
}

/// Reads fixture `name` and subdivides its face at every interior knot.
fn subdivide(name: &str) -> Subdivided {
    let mut k = Kernel::default();
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key_of,
        records: &mut records,
        next_key: 1 << 50,
    };
    read_from_file(
        &mut k,
        &mut b,
        fixture(name),
        FileType::Ascii,
        false,
        &mut env,
    )
    .unwrap();
    let face = faces(&b, None).unwrap()[0];
    let curve_ref = |e: EdgeId, _type_code: i64| key_of(EntityRef::Edge(e));
    let mut notify = NotifyEnv {
        records: &mut records,
        attributes: &mut attributes,
        context: None,
        key_of: &key_of,
    };
    let r = subdivide_face_at_discontinuities(
        &mut k,
        &mut b,
        &mut notify,
        &curve_ref,
        face,
        ContinuityType::CInfinity,
    );
    let n = faces(&b, None).unwrap().len();
    let points: Vec<_> = solidcore::topology::brep::p1::vertices(&b, None)
        .unwrap()
        .into_iter()
        .map(|v| solidcore::topology::vertex::point(&b, v).unwrap())
        .collect();
    Subdivided {
        result: r,
        faces: n,
        xs: points.iter().map(|p| grid(p.x)).collect(),
        ys: points.iter().map(|p| grid(p.y)).collect(),
    }
}

/// `values` in units of 1e-12.
fn set(values: &[f64]) -> BTreeSet<i64> {
    values.iter().map(|&v| grid(v)).collect()
}

/// A crafted fixture of the `topology.brep_p10` difftest.
fn fixture(name: &str) -> Vec<u8> {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/topology.brep_p10");
    std::fs::read(root.join(name)).unwrap()
}

#[test]
fn one_u_knot_and_two_v_knots_make_six_faces() {
    let s = subdivide("subdivide_u1v2.smb");
    s.result.unwrap();
    assert_eq!(s.faces, 6);
    assert_eq!(s.xs, set(&[0.0, 0.3, 1.0]));
    assert_eq!(s.ys, set(&[0.0, 0.2, 0.7, 1.0]));
}

#[test]
fn a_v_knot_alone_is_cut() {
    let s = subdivide("subdivide_u0v1.smb");
    s.result.unwrap();
    assert_eq!(s.faces, 2);
    assert_eq!(s.xs, set(&[0.0, 1.0]));
    assert_eq!(s.ys, set(&[0.0, 0.3, 1.0]));
}

#[test]
fn a_u_knot_alone_is_cut() {
    let s = subdivide("subdivide_u1v0.smb");
    s.result.unwrap();
    assert_eq!(s.faces, 2);
    assert_eq!(s.xs, set(&[0.0, 0.3, 1.0]));
    assert_eq!(s.ys, set(&[0.0, 1.0]));
}
