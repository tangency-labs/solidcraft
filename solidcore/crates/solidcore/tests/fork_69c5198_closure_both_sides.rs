//! WR-584 (the fork's tangency-labs/smlib@69c5198): `SmBrep::TestClosureSplitRegion`
//! asks whether a new face closes a region with `ClosureTraversal`, a depth-first walk
//! from one side that stops only at the face's mate, so a face that does not close a
//! region walks (and marks) most of its shell first.
//!
//! The repro is an open box: a 10 mm cube with its top face deleted, all ten face uses
//! of the five walls in one shell. The bottom face's edges are all shared with walls (no
//! lamina edge), and its two sides meet over the walls' lamina top edges, so it closes
//! nothing. Upstream's walk marks face uses before it reaches the mate; the two-sided
//! search `TestClosureSplitRegion` now runs marks none and gives the same answer (no
//! split). On the whole cube and the open box the two traversals agree for every face
//! use, and a union of two boxes overlapping at a corner has the polyhedron's faces,
//! edges and vertices.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::original_key;
use solidcore::boolean::api_brep as sm;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::attribute::AttributeStore;
use solidcore::geom::context::{MarkLock, ALL_MARKS};
use solidcore::math::Vec3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::{p3, p6};
use solidcore::topology::edge::NotifyEnv;
use solidcore::topology::edgeuse::p1 as edgeuse_p1;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::topology_traverser::{
    closure_traversal, closure_traversal_from_both_sides, Entity, Marks,
};
use solidcore::topology::{Brep, FaceId, FaceuseId};
use solidcore::Kernel;

/// The face whose vertices all have `z == z`.
fn face_at_z(b: &Brep, z: f64) -> FaceId {
    *b.face_list()
        .iter()
        .find(|&&f| {
            b.face(f).unwrap().loops.iter().all(|&l| {
                b.loop_(l).unwrap().edges.iter().all(|le| {
                    b.edge(le.edge)
                        .unwrap()
                        .vertices
                        .iter()
                        .all(|&v| b.vertex(v).unwrap().point.z == z)
                })
            })
        })
        .unwrap()
}

/// Both face uses of every face of `b` (the arena keeps deleted faces' records).
fn live_faceuses(b: &Brep) -> Vec<FaceuseId> {
    let mut out = Vec::new();
    for &f in b.face_list() {
        let fu = b.face(f).unwrap().faceuse.unwrap();
        out.push(fu);
        out.push(b.faceuse(fu).unwrap().mate.unwrap());
    }
    out
}

/// A 10 mm cube with its top face deleted (no edges or vertices deleted).
fn open_box(k: &mut Kernel) -> Brep {
    let mut b = create_box(k, Vec3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let top = face_at_z(&b, 10.0);
    b.set_editing_enabled(true);
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key_of = original_key;
    let mut env = NotifyEnv {
        records: &mut records,
        attributes: &mut attributes,
        context: None,
        key_of: &key_of,
    };
    p3::delete_face(
        k,
        &mut b,
        &mut env,
        &mut Marks::default(),
        top,
        p3::DeleteFaceOptions::default(),
    )
    .unwrap();
    b
}

/// `TestClosureSplitRegion` on the open box's bottom face use: its outputs, and how many
/// face uses the caller's marks hold a record for afterwards.
fn split_bottom() -> (bool, usize, usize, usize) {
    let mut k = Kernel::default();
    let mut b = open_box(&mut k);
    let bottom = face_at_z(&b, 0.0);
    let fu = b.face(bottom).unwrap().faceuse.unwrap();
    let mate = b.faceuse(fu).unwrap().mate.unwrap();
    // the premise: both sides in one shell, so the closure test runs
    assert_eq!(b.faceuse(fu).unwrap().shell, b.faceuse(mate).unwrap().shell);
    let shells = b.shells().len();
    let regions = b.regions().len();

    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key_of = original_key;
    let mut env = NotifyEnv {
        records: &mut records,
        attributes: &mut attributes,
        context: None,
        key_of: &key_of,
    };
    let mut marks = Marks::default();
    let (shell, region) =
        p6::test_closure_split_region(&mut k, &mut b, &mut marks, &mut env, false, fu).unwrap();
    let live = live_faceuses(&b);
    let marked = live
        .iter()
        .filter(|&&id| marks.record(Entity::Faceuse(id)).is_some())
        .count();
    assert_eq!(b.shells().len(), shells);
    assert_eq!(b.regions().len(), regions);
    (
        shell.is_some() || region.is_some(),
        marked,
        live.len(),
        b.face_list().len(),
    )
}

#[test]
fn open_face_both_sides_marks_nothing() {
    let (split, marked, faceuses, faces) = split_bottom();
    assert_eq!(faces, 5);
    assert_eq!(faceuses, 10);
    assert!(!split, "an open face splits nothing");
    assert_eq!(marked, 0, "the two-sided search marks no face use");
}

/// Both traversals' answers for every face use of `b`.
fn both_answers(k: &mut Kernel, b: &Brep) -> Vec<(FaceuseId, bool, bool)> {
    let mut out = Vec::new();
    for fu in live_faceuses(b) {
        let mut lock = MarkLock::new(std::slice::from_mut(&mut k.context), 0, ALL_MARKS).unwrap();
        let old = closure_traversal(k, b, &mut Marks::default(), fu, &lock, None).unwrap();
        lock.release(std::slice::from_mut(&mut k.context)).unwrap();
        out.push((fu, old, closure_traversal_from_both_sides(b, fu).unwrap()));
    }
    out
}

#[test]
fn both_traversals_agree_closed_cube_and_open_box() {
    let mut k = Kernel::default();
    let cube = create_box(&mut k, Vec3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let answers = both_answers(&mut k, &cube);
    assert_eq!(answers.len(), 12);
    // every side of a cube's face bounds a closed set of face uses (its own side)
    for (fu, old, new) in answers {
        assert!(old, "{fu:?}");
        assert_eq!(old, new, "{fu:?}");
    }
    let open = open_box(&mut k);
    let answers = both_answers(&mut k, &open);
    assert_eq!(answers.len(), 10);
    // every face of the open box has a lamina edge (walls) or reaches its mate (bottom)
    for (fu, old, new) in answers {
        assert!(!old, "{fu:?}");
        assert_eq!(old, new, "{fu:?}");
    }
}

/// Face, edge and vertex counts and the vertex coordinates (sorted) of a union of two
/// overlapping boxes.
fn union() -> (usize, usize, usize, Vec<[u64; 3]>) {
    let mut k = Kernel::default();
    let a = create_box(&mut k, Vec3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let b = create_box(&mut k, Vec3::new(5.0, 5.0, 5.0), 10.0, 10.0, 10.0).unwrap();
    let r = sm::boolean(&mut k, a, b, BooleanOperation::Union)
        .unwrap()
        .result;
    let mut points: Vec<[u64; 3]> = r
        .vertices()
        .iter()
        .map(|(_, v)| {
            [
                v.point.x.to_bits(),
                v.point.y.to_bits(),
                v.point.z.to_bits(),
            ]
        })
        .collect();
    points.sort_unstable();
    (
        r.face_list().len(),
        r.edges().len(),
        r.vertices().len(),
        points,
    )
}

/// The union of [0, 10]^3 and [5, 15]^3: 6 + 6 faces (3 of each cut by the other); the
/// 7 corners of each cube outside the other plus the 6 points where two planes of one
/// cube meet a plane of the other ((10, 10, 5), (5, 5, 10) and their permutations), 20
/// vertices; and 12 + 20 - 2 = 30 edges (Euler, one closed genus-0 shell).
#[test]
fn union_of_two_boxes_is_the_corner_polyhedron() {
    let (faces, edges, vertices, points) = union();
    assert_eq!((faces, edges, vertices), (12, 30, 20));
    let mut expected: Vec<[u64; 3]> = Vec::new();
    for (lo, hi) in [(0.0_f64, 10.0_f64), (5.0, 15.0)] {
        for x in [lo, hi] {
            for y in [lo, hi] {
                for z in [lo, hi] {
                    // the corner inside the other cube is gone
                    if (lo == 0.0 && x == hi && y == hi && z == hi)
                        || (lo == 5.0 && x == lo && y == lo && z == lo)
                    {
                        continue;
                    }
                    expected.push([x.to_bits(), y.to_bits(), z.to_bits()]);
                }
            }
        }
    }
    for p in [
        [10.0_f64, 10.0, 5.0],
        [10.0, 5.0, 10.0],
        [5.0, 10.0, 10.0],
        [5.0, 5.0, 10.0],
        [5.0, 10.0, 5.0],
        [10.0, 5.0, 5.0],
    ] {
        expected.push([p[0].to_bits(), p[1].to_bits(), p[2].to_bits()]);
    }
    expected.sort_unstable();
    assert_eq!(points, expected);
}

/// A broken shell: one edge use of the open box's bottom face taken out of its edge's
/// edge use list, so it has no radial. Both searches fail on it (the fork's skipped it
/// and answered), and `TestClosureSplitRegion` fails before adding a shell or region.
#[test]
fn a_missing_radial_fails_both_searches_before_any_split() {
    let mut k = Kernel::default();
    let mut b = open_box(&mut k);
    let bottom = face_at_z(&b, 0.0);
    let fu = b.face(bottom).unwrap().faceuse.unwrap();
    let lu = b.faceuse(fu).unwrap().loopuses[0];
    let broken = edgeuse_p1::loopuse_edgeuses(&b, lu).unwrap()[0];
    let edge = b.edgeuse(broken).unwrap().edge.unwrap();
    b.edge_mut(edge).unwrap().edgeuses.retain(|&e| e != broken);
    assert_eq!(edgeuse_p1::radial(&b, broken).unwrap(), None);

    let mut lock = MarkLock::new(std::slice::from_mut(&mut k.context), 0, ALL_MARKS).unwrap();
    let old = closure_traversal(&mut k, &b, &mut Marks::default(), fu, &lock, None);
    lock.release(std::slice::from_mut(&mut k.context)).unwrap();
    assert!(old.is_err(), "the one-sided walk fails: {old:?}");
    let new = closure_traversal_from_both_sides(&b, fu);
    assert!(new.is_err(), "the two-sided search fails too: {new:?}");

    let shells = b.shells().len();
    let regions = b.regions().len();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key_of = original_key;
    let mut env = NotifyEnv {
        records: &mut records,
        attributes: &mut attributes,
        context: None,
        key_of: &key_of,
    };
    let split =
        p6::test_closure_split_region(&mut k, &mut b, &mut Marks::default(), &mut env, false, fu);
    assert!(split.is_err(), "{split:?}");
    assert_eq!(b.shells().len(), shells, "no shell added");
    assert_eq!(b.regions().len(), regions, "no region added");
}
