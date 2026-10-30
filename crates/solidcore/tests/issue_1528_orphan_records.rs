//! tangency-labs/solidcore#1528 follow-up: after a Boolean difference no deleted
//! topology record stays in the B-rep's arenas. Upstream `SmBrep::DeleteFace` and
//! `SmBrep::SqueezeEdge` `delete` the face, its face uses, loops, loop uses and vertex
//! uses, and the squeezed edge, its uses and the deleted vertex; the port used to take
//! them off the B-rep's lists only, so `faces().len()` and arena walks counted 12 faces
//! and 17 edges for the 8 faces and 15 edges of the #1528 operand (finding
//! `plan/findings/topology.brep_p3--deleted-records-stay-in-arenas.md`).
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

/// The #1528 operand: a plate 27.5 thick on three straight sides and a half circle
/// r 10, less a blind hole r 5 along x (as `issue_1528_write_operand.rs` builds it).
fn drilled_plate(k: &mut Kernel) -> Brep {
    let p = |x: f64, y: f64| Vec3::new(x, y, 0.0);
    let cs: Vec<Curve> = vec![
        Curve::from(curves::create_line_segment(p(0.0, 0.0), p(42.5, 0.0)).unwrap()),
        Curve::from(curves::create_line_segment(p(42.5, 0.0), p(42.5, 20.0)).unwrap()),
        Curve::from(curves::create_line_segment(p(42.5, 20.0), p(0.0, 20.0)).unwrap()),
        Curve::from(curves::create_arc(p(0.0, 10.0), 10.0, 90.0, 270.0).unwrap()),
    ];
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    let body = sweeps::create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 27.5, true).unwrap();
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

/// A 10 cube less a cylinder r 2 at its middle: `height` 12 from below goes through,
/// 6 stops inside (a blind hole).
fn box_less_cylinder(k: &mut Kernel, height: f64) -> Brep {
    let body = primitives::create_box(k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let drill = primitives::create_cylinder(k, Point3::new(5.0, 5.0, -1.0), 2.0, height).unwrap();
    boolean::boolean_difference(k, body, drill).unwrap()
}

/// The face and edge arenas of `b` hold exactly the B-rep's face and edge lists: the
/// only list-only removals of faces and edges were `DeleteFace` and `SqueezeEdge`'s
/// (`GlueEdges` destroys its deleted edge's record). The other arenas are printed, not
/// checked: vertices (`topology.brep_p5`'s list-only removal), regions, face uses,
/// loops and the use records may still be left by paths outside this fix (finding
/// `plan/findings/topology.brep--orphaned-arena-records-2026-10-26.md`); the follow-up
/// that fixes those paths checks them.
fn assert_no_orphan_records(label: &str, b: &Brep) {
    let face_loops: usize = b.faces().iter().map(|(_, f)| f.loops.len()).sum();
    let edge_uses: usize = b.edges().iter().map(|(_, e)| e.edgeuses.len()).sum();
    let vertex_uses: usize = b.vertices().iter().map(|(_, v)| v.vertexuses.len()).sum();
    let rows = [
        ("face", b.faces().len(), b.face_list().len(), true),
        ("edge", b.edges().len(), b.edge_list().len(), true),
        ("vertex", b.vertices().len(), b.vertex_list().len(), false),
        (
            "region",
            b.regions().len(),
            bl::regions(b, None).unwrap().len(),
            false,
        ),
        ("face use", b.faceuses().len(), 2 * b.faces().len(), false),
        ("loop", b.loops().len(), face_loops, false),
        ("loop use", b.loopuses().len(), 2 * b.loops().len(), false),
        ("edge use", b.edgeuses().len(), edge_uses, false),
        ("vertex use", b.vertexuses().len(), vertex_uses, false),
    ];
    for (what, arena, live, _) in rows {
        eprintln!("{label}: {what} arena {arena}, in the B-rep {live}");
    }
    for (what, arena, live, checked) in rows {
        if checked {
            assert_eq!(
                arena, live,
                "{label}: {what} arena {arena}, in the B-rep {live}"
            );
        }
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
