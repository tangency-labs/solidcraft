//! tangency-labs/solidcore#1528 follow-up: no deleted topology record stays in the
//! B-rep's arenas, after the primitive constructors, a linear sweep or a Boolean
//! difference. Upstream `delete`s the records `SmBrep::DeleteFace`, `SqueezeEdge`, the
//! edge topology makers (a vertex shell's vertex use), `sm_RmAndDel_OldLoopVertices`,
//! the loop removal of `MakeEdgeInFaceWithVU` and the shell and region clean-ups
//! drop; the port used to take them off the B-rep's lists only, so `faces().len()` and
//! arena walks counted 12 faces and 17 edges for the 8 faces and 15 edges of the #1528
//! operand (findings `plan/findings/topology.brep_p3--deleted-records-stay-in-arenas.md`,
//! `plan/findings/topology.brep--deleted-records-leave-their-arenas-follow-up.md`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep as boolean;
use solidcore::curve::curve::Curve;
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::primitive::api_curves as curves;
use solidcore::primitive::api_primitives as primitives;
use solidcore::sweep::api_brep as sweeps;
use solidcore::topology::brep::p1 as bl;
use solidcore::topology::proto_topology::p1::api_transform::{api_rotate, TransformObject};
use solidcore::{Brep, Kernel};

/// The #1528 plate: 27.5 thick on three straight sides and a half circle r 10.
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

/// The #1528 operand: the plate less a blind hole r 5 along x (as
/// `issue_1528_write_operand.rs` builds it).
fn drilled_plate(k: &mut Kernel) -> Brep {
    let body = plate(k);
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
    boolean::boolean_difference(k, body, drill).unwrap()
}

fn cube(k: &mut Kernel) -> Brep {
    primitives::create_box(k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap()
}

/// A cylinder r 2 up the middle of [`cube`] from below: `height` 12 goes through, 6
/// stops inside.
fn drill(k: &mut Kernel, height: f64) -> Brep {
    primitives::create_cylinder(k, Point3::new(5.0, 5.0, -1.0), 2.0, height).unwrap()
}

/// [`cube`] less [`drill`]: a through hole (`height` 12) or a blind hole (6).
fn box_less_cylinder(k: &mut Kernel, height: f64) -> Brep {
    let body = cube(k);
    let drill = drill(k, height);
    boolean::boolean_difference(k, body, drill).unwrap()
}

/// Every live record of `b` is part of the B-rep: the face, edge and vertex arenas hold
/// exactly the B-rep's lists, the region arena its regions, every face has its two face
/// uses, every loop is a face's and has its two loop uses, and every edge use and vertex
/// use is on its edge's or vertex's list.
fn assert_no_orphan_records(label: &str, b: &Brep) {
    let face_loops: usize = b.faces().iter().map(|(_, f)| f.loops.len()).sum();
    let edge_uses: usize = b.edges().iter().map(|(_, e)| e.edgeuses.len()).sum();
    let vertex_uses: usize = b.vertices().iter().map(|(_, v)| v.vertexuses.len()).sum();
    let rows = [
        ("face", b.faces().len(), b.face_list().len()),
        ("edge", b.edges().len(), b.edge_list().len()),
        ("vertex", b.vertices().len(), b.vertex_list().len()),
        (
            "region",
            b.regions().len(),
            bl::regions(b, None).unwrap().len(),
        ),
        ("face use", b.faceuses().len(), 2 * b.faces().len()),
        ("loop", b.loops().len(), face_loops),
        ("loop use", b.loopuses().len(), 2 * b.loops().len()),
        ("edge use", b.edgeuses().len(), edge_uses),
        ("vertex use", b.vertexuses().len(), vertex_uses),
    ];
    for (what, arena, live) in rows {
        eprintln!("{label}: {what} arena {arena}, in the B-rep {live}");
    }
    for (what, arena, live) in rows {
        assert_eq!(
            arena, live,
            "{label}: {what} arena {arena}, in the B-rep {live}"
        );
    }
}

#[test]
fn issue_1528_operand_keeps_no_deleted_records() {
    let mut k = Kernel::default();
    let b = drilled_plate(&mut k);
    assert_eq!((b.face_list().len(), b.edge_list().len()), (8, 15));
    assert_no_orphan_records("#1528 drilled plate", &b);
}

#[test]
fn box_less_cylinder_keeps_no_deleted_records() {
    let mut k = Kernel::default();
    for (label, height) in [("through hole", 12.0), ("blind hole", 6.0)] {
        let b = box_less_cylinder(&mut k, height);
        assert_no_orphan_records(label, &b);
    }
}

#[test]
fn primitives_and_sweeps_keep_no_deleted_records() {
    let mut k = Kernel::default();
    let b = cube(&mut k);
    assert_no_orphan_records("box", &b);
    let c = drill(&mut k, 12.0);
    assert_no_orphan_records("cylinder", &c);
    let p = plate(&mut k);
    assert_no_orphan_records("plate sweep", &p);
}
