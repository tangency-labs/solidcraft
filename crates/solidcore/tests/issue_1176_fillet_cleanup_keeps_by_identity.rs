//! tangency-labs/solidcore#1176: the fork's fillet clean-up (`DoFilleting` with
//! `m_sPostProcessKeptEdges`/`Faces`, WR-459) kept the target's own edges and faces by
//! address, so an edge the fillet made at the address of a target edge it had deleted
//! passed `BooleanPostProcess`'s kept test and was never cleaned up: the result depended
//! on heap layout (`prog_test` fillets case 306, `Fillet/extrusion0210.smb`).
//!
//! The port keeps them by arena id (`Kernel::fixes.fillet_cleanup_keeps_target_faces`),
//! and an arena never hands a deleted record's id to a new one, so the membership is the
//! fork's fix (`SM_AI_FILLET_INPUT`, which dies with its entity): no entity made while
//! filleting is kept. This runs case 306's constant radius rounds (edges 10 to 14 at 0.5,
//! `SmConstantRadiusFS(SM_ZONE_TOL_3D, 30 deg, 2 deg)`, circular cross sections, the
//! `SmMakeSurfaceBlendSIH(1.0)` handler) with the clean-up keeping the target's faces, and
//! checks that the result is a manifold solid, that every edge the fillet made has an id
//! the target did not have, and that the same rounds give the same body after an earlier
//! fillet in the same process. Case 306's variable radius round of edge 24 needs a
//! variable radius solver with its own tolerances, which `FilletSession` does not make
//! public, and is left out.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::collections::BTreeSet;

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::{FilletSurfaceGenerator, MakeSurfaceBlendSih};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::geom::tol::ZONE_TOL_3D;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::PI;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::EdgeId;
use solidcore::{Brep, EntityRef, Kernel};

/// An upstream test file read by the Rust reader, arena-named.
fn read(k: &mut Kernel, rel: &str) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles")
        .join(rel);
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("unavailable upstream test file: {}", path.display());
        return None;
    };
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key = |_: EntityRef| ObjectRef {
        key: ObjectKey(0),
        type_code: 0,
    };
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key,
        records: &mut records,
        next_key: 1 << 50,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b.use_arena_naming().unwrap();
    Some(b)
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// Case 306's constant radius rounds on `b` (edges 10 to 14 in `GetEdges` order, r 0.5).
fn round_case_306(k: &mut Kernel, b: &mut Brep) {
    let edges = p1::edges(b, None).unwrap();
    let mut session = FilletSession::new(k, b).unwrap();
    let generator = session
        .add_surface_generator(FilletSurfaceGenerator::circular(
            false,
            ZONE_TOL_3D,
            false,
            false,
        ))
        .unwrap();
    session.set_self_intersection_handler(Some(MakeSurfaceBlendSih::new(1.0)));
    for &e in edges.iter().take(15).skip(10) {
        let Some(edgeuse) = session.blend_edgeuse(k, e).unwrap() else {
            continue;
        };
        let solver = session
            .new_constant_radius_solver(
                k,
                edgeuse,
                0.5,
                ZONE_TOL_3D,
                30.0 * PI / 180.0,
                2.0 * PI / 180.0,
            )
            .unwrap();
        session.set_solver_generator(solver, generator).unwrap();
        session.load_solver(k, solver).unwrap();
    }
    session.create_corners(k, None).unwrap();
    session.do_filleting(k, None).unwrap();
}

/// Case 306's result.
struct Case306 {
    /// The target's edge ids.
    before: BTreeSet<EdgeId>,
    /// The result's edge ids.
    after: BTreeSet<EdgeId>,
    /// The result's face count.
    faces: usize,
    /// The result's edge count.
    edges: usize,
    /// The result's volume.
    volume: f64,
}

/// Case 306 filleted with the clean-up keeping the target's faces.
fn case_306(k: &mut Kernel) -> Option<Case306> {
    k.fixes.fillet_cleanup_keeps_target_faces = true;
    let mut b = read(k, "Fillet/extrusion0210.smb")?;
    let before: BTreeSet<EdgeId> = p1::edges(&b, None).unwrap().into_iter().collect();
    // (arena ids are never reused: every id past the target's highest is a new record)
    let arena_before = b
        .edges()
        .ids()
        .map(|e| e.index() as usize + 1)
        .max()
        .unwrap_or(0);
    round_case_306(k, &mut b);
    assert!(
        is_manifold_solid(&b).unwrap(),
        "case 306 is not a manifold solid"
    );
    let after: BTreeSet<EdgeId> = p1::edges(&b, None).unwrap().into_iter().collect();
    // every edge the fillet made has an id past the target's
    for e in after.difference(&before) {
        assert!(
            e.index() as usize >= arena_before,
            "edge {e:?} made while filleting reuses a target edge's id"
        );
    }
    let faces = p1::faces(&b, None).unwrap().len();
    let edges = after.len();
    let volume = volume(k, &mut b);
    Some(Case306 {
        before,
        after,
        faces,
        edges,
        volume,
    })
}

#[test]
fn issue_1176_fillet_cleanup_keeps_the_target_by_identity() {
    let mut k = Kernel::default();
    let Some(first) = case_306(&mut k) else {
        return;
    };
    assert!(
        first.after.difference(&first.before).next().is_some(),
        "the rounds made no edge"
    );

    // the same rounds after an earlier fillet in the same process (case 306's own
    // rounds) give the same body
    let mut k2 = Kernel::default();
    case_306(&mut k2).unwrap();
    let again = case_306(&mut k2).unwrap();
    assert_eq!((again.faces, again.edges), (first.faces, first.edges));
    let (v, v2) = (first.volume, again.volume);
    assert!(
        (v2 - v).abs() <= 1e-9 * v.abs(),
        "volume {v2} after an earlier fillet, {v} before"
    );
}
