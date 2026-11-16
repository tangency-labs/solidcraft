//! tangency-labs/solidcore#1528: writes the shell operand of the issue as the ASCII
//! `.smb` file `fixtures/offset.offset_executive_p2/data/issue1528_plate.smb`, which the
//! `offset.offset_executive_p2` case `issue1528_plate_api_shell_2` reads on both sides
//! (`SmBrep::ReadFromFile` in the shim, the Rust reader here), so the pinned C++ shells
//! the same body the port does. The operand: a plate 27.5 thick on a profile of three
//! straight sides and a half circle r 10 (the straight sides from (0, 0) to (42.5, 0) to
//! (42.5, 20) to (0, 20), the half circle centred (0, 10) on the x < 0 side), less a
//! blind hole r 5 along x from x = -10 to x = 7.5 at (y, z) = (12.5, 12.5).
//!
//! Ignored: run once to (re)generate the file, then put the printed open-face index in
//! the case's `faces`:
//! `cargo test -p solidcore --test issue_1528_write_operand -- --ignored --nocapture`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{planar_faces_at, report, shell, volume};
use solidcore::boolean::api_brep as boolean;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::object::ObjectRef;
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::primitive::api_curves as curves;
use solidcore::primitive::api_primitives as primitives;
use solidcore::sweep::api_brep as sweeps;
use solidcore::topology::api_files::{read_brep_from_file, write_brep_to_file};
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p5::transform;
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::proto_topology::p1::api_transform::{api_rotate, TransformObject};
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{EdgeId, EdgeuseId, FaceId};
use solidcore::{Brep, EntityRef, Kernel};

const OUT: &str = "fixtures/offset.offset_executive_p2/data/issue1528_plate.smb";

/// The notification layout of `issue_1584_check_brep_naming.rs`: a caller key per entity.
fn named(e: EntityRef, base: u64) -> ObjectRef {
    let (type_code, kind, index) = match e {
        EntityRef::Brep(x) => (16011, 0u64, x.index()),
        EntityRef::Region(x) => (16012, 1, x.index()),
        EntityRef::Shell(x) => (16013, 2, x.index()),
        EntityRef::Faceuse(x) => (16022, 3, x.index()),
        EntityRef::Face(x) => (16002, 4, x.index()),
        EntityRef::Loopuse(x) => (16004, 5, x.index()),
        EntityRef::Loop(x) => (16003, 6, x.index()),
        EntityRef::Edgeuse(x) => (16005, 7, x.index()),
        EntityRef::Edge(x) => (16016, 8, x.index()),
        EntityRef::Vertexuse(x) => (16006, 9, x.index()),
        EntityRef::Vertex(x) => (16021, 10, x.index()),
    };
    ObjectRef {
        type_code,
        key: ObjectKey(base + 1 + (kind << 32) + u64::from(index)),
    }
}

fn plate(k: &mut Kernel) -> Brep {
    let p = |x: f64, y: f64| Vec3::new(x, y, 0.0);
    let cs: Vec<Curve> = vec![
        Curve::from(curves::create_line_segment(p(0.0, 0.0), p(42.5, 0.0)).unwrap()),
        Curve::from(curves::create_line_segment(p(42.5, 0.0), p(42.5, 20.0)).unwrap()),
        Curve::from(curves::create_line_segment(p(42.5, 20.0), p(0.0, 20.0)).unwrap()),
        Curve::from(curves::create_arc(p(0.0, 10.0), 10.0, 90.0, 270.0).unwrap()),
    ];
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    sweeps::create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 27.5, true).unwrap()
}

fn drilled(k: &mut Kernel) -> Brep {
    let body = plate(k);
    // a cylinder r 5 along z from (-10, 12.5, 12.5), turned onto +x about its base
    let mut drill =
        primitives::create_cylinder(k, Point3::new(-10.0, 12.5, 12.5), 5.0, 17.5).unwrap();
    api_rotate(
        k,
        Some(TransformObject::Brep(&mut drill)),
        Point3::new(-10.0, 12.5, 12.5),
        Vec3::new(0.0, 1.0, 0.0),
        90.0,
    )
    .unwrap();
    let mut b = boolean::boolean_difference(k, body, drill).unwrap();
    b.refresh_cached_topology();
    b
}

#[test]
#[ignore = "generator: writes the #1528 fixture file"]
fn write_issue_1528_operand() {
    let mut k = Kernel::default();
    let b = drilled(&mut k);
    write_and_shell(&mut k, &b, OUT, "1528");
}

/// The box-and-full-cylinder version of the plate (the plate as first rebuilt for WR-578):
/// a 42.5 x 20 x 27.5 box joined with a full cylinder r 10 on the z axis through (0, 10),
/// less the same blind hole, which turns onto +x by a placement instead of a rotation.
const OUT_BOX_CYLINDER: &str =
    "fixtures/offset.offset_executive_p2/data/issue1528_box_cylinder.smb";

fn box_cylinder(k: &mut Kernel) -> Brep {
    let slab = primitives::create_box(k, Point3::new(0.0, 0.0, 0.0), 42.5, 20.0, 27.5).unwrap();
    let round = primitives::create_cylinder(k, Point3::new(0.0, 10.0, 0.0), 10.0, 27.5).unwrap();
    let plate = boolean::boolean_union(k, slab, round).unwrap();
    let mut hole = primitives::create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 5.0, 17.5).unwrap();
    // local z (the axis) along +x from x = -10: x = (0, 1, 0), y = (0, 0, 1)
    let placement = Axis2Placement::new(
        Point3::new(-10.0, 12.5, 12.5),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    );
    transform(k, &mut hole, &placement, None, false).unwrap();
    let mut b = boolean::boolean_difference(k, plate, hole).unwrap();
    b.refresh_cached_topology();
    b
}

#[test]
#[ignore = "generator: writes the #1528 box-and-cylinder fixture file"]
fn write_issue_1528_box_cylinder_operand() {
    let mut k = Kernel::default();
    let b = box_cylinder(&mut k);
    write_and_shell(&mut k, &b, OUT_BOX_CYLINDER, "1528 box-cylinder");
}

/// Writes `b` to `out` (repository-relative), reads it back as the runner does and prints
/// the open face's index and the read-back shell's outcome.
fn write_and_shell(k: &mut Kernel, b: &Brep, out: &str, label: &str) {
    let store = AttributeStore::new();
    let recs = NotifyRecords::new();
    let key_of = |e| named(e, 0);
    let env = FromBrepEnv {
        attributes: &store,
        key_of: &key_of,
        records: &recs,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    let bytes = write_brep_to_file(k, Some(b), &env, true).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join(out);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, &bytes).unwrap();

    // Read it back as the runner does (`ReadFromFile` into a new B-rep), so the index
    // printed is the open face's position in the read body's face order (`GetFaces`).
    let mut store2 = AttributeStore::new();
    let mut recs2 = NotifyRecords::new();
    let read_key = |e| named(e, 1 << 40);
    let mut tenv = TopologyEnv {
        all_attributes: &[],
        attributes: &mut store2,
        key_of: &read_key,
        records: &mut recs2,
        next_key: 1 << 50,
    };
    let mut read = read_brep_from_file(k, Some(bytes.clone()), true, false, &mut tenv).unwrap();
    read.refresh_cached_topology();
    let open = planar_faces_at(k, &read, 0, 42.5);
    assert_eq!(open.len(), 1, "the far end face");
    let index = read.face_list().iter().position(|&f| f == open[0]).unwrap();
    eprintln!(
        "ISSUE {label} wrote {out} ({} bytes): faces {} edges {} volume {:?}; open face index {index}",
        bytes.len(),
        read.faces().len(),
        read.edges().len(),
        volume(k, &mut read)
    );
    let result = shell(k, read, -2.0, &open);
    report(
        k,
        &format!("{label} read-back shell 2 open at x 42.5"),
        result,
        None,
        1e-6,
    );
}
