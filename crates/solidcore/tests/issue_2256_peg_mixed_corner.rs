//! tangency-labs/solidcore#2256 (WR-710): the tangent peg's front top edge and its concave
//! arc rounded r 2 together.
//!
//! The body (`data/issue_2256_peg_step_reimported.smb`, #2256's attachment, sha256
//! `1ad77fabebf79737e249208db89e2b2e04afd9953ab6efcaf3ae978c7a3acfba`) is SolidCraft's kernel
//! case fillets/report-peg-step-front-and-concave-rounds-together-r2 just before its fillet: a bar
//! 40 x 16 x 10 whose far end is a half disc R8 about (40, 8), joined with a disc R8 about the
//! same centre extruded from z = 10 to 20, written to STEP and read back. The front face y = 0
//! runs tangent into the cylinder along x = 40, so the front top edge and the concave arc
//! (where the bar's top meets the cylinder) leave the vertex (40, 0, 10) along one tangent,
//! -x: the bar's top closes to a cusp between them. The vertex has three edges, two filleted
//! and of differing convexity (`SmFillet3x2MixedConvexityCorner`); the rails on the top face,
//! y = 2 and the circle R10 about (40, 8), cross at (32, 2, 10), before the corner.
//!
//! The planes y = 0, y = 16, z = 0, x = 0 and the cylinder's top z = 20 bound the body and
//! every blend should lie within them. The corner patch the pinned construction builds stands
//! 6.544e-3 out of the front face y = 0 though `check_brep` passes. With
//! `fillet_refuse_mixed_corner_crossing_rails` the fillet refuses the corner instead (it builds
//! no corner for it); without it the body is built as before.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test helpers

use solidcore::blend::fillet_corner::p3::MIXED_CORNER_RAILS_CROSS;
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::query::api_queries::{brep_bounding_box, edge_closest_point};
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{Brep, EdgeId};
use solidcore::{EntityRef, SolidError};

fn key_of(e: EntityRef) -> ObjectRef {
    let (type_code, kind, i) = match e {
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
        key: ObjectKey(((kind + 1) << 32) | u64::from(i)),
    }
}

/// Every opt-in fix on (SolidCraft's set) but the blend corner (`fillet_blend_corner`,
/// whose Mixed corner builds this corner: `issue_2360_mixed_corner.rs`), with WR-710's
/// refusal on or off.
fn kernel(refuse: bool) -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = UpstreamFixes::all();
    k.fixes.fillet_blend_corner = false;
    k.fixes.fillet_refuse_mixed_corner_crossing_rails = refuse;
    k
}

fn read(k: &mut Kernel) -> Brep {
    let data = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/issue_2256_peg_step_reimported.smb"
    ))
    .unwrap();
    let mut attributes = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key_of,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut b = read_brep_from_file(k, Some(data), false, false, &mut env).unwrap();
    b.refresh_cached_topology();
    b
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (f64::INFINITY, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance < best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap()
}

/// The front top edge (y = 0, z = 10, nearest (20, 0, 10)) and the concave arc (z = 10,
/// about (40, 8), R8, nearest (32, 8, 10)); with `back`, also the back top edge (y = 16).
fn round(refuse: bool, back: bool) -> (Kernel, Brep, solidcore::Result<()>) {
    let mut k = kernel(refuse);
    let mut b = read(&mut k);
    let mut picks = vec![[20.0, 0.0, 10.0], [32.0, 8.0, 10.0]];
    if back {
        picks.push([20.0, 16.0, 10.0]);
    }
    let es: Vec<EdgeId> = picks.iter().map(|p| pick(&k, &b, *p)).collect();
    let r = fillet_edges(&mut k, &mut b, &es, 2.0, 1, 1, 1.0);
    b.refresh_cached_topology();
    (k, b, r)
}

fn assert_refused(r: &solidcore::Result<()>) {
    match r {
        Err(SolidError::Unsupported { reason, .. }) => {
            assert_eq!(*reason, MIXED_CORNER_RAILS_CROSS);
        }
        other => panic!("expected the WR-710 refusal, got {other:?}"),
    }
}

/// With the refusal on, the corner at (40, 0, 10) is refused, never built off its face.
#[test]
fn front_and_concave_arc_refused() {
    let (_, _, r) = round(true, false);
    assert_refused(&r);
}

/// With the back top edge too, both cusp corners ((40, 0, 10) and (40, 16, 10)) are of this
/// kind: refused as well.
#[test]
fn front_concave_arc_and_back_refused() {
    let (_, _, r) = round(true, true);
    assert_refused(&r);
}

/// With the refusal off, the pinned construction runs as before: a body `check_brep` passes,
/// standing 6.544e-3 out of the front face y = 0 (the defect the refusal guards; #2308 holds
/// a real construction for the corner).
#[test]
fn front_and_concave_arc_built_without_refusal() {
    let (mut k, mut b, r) = round(false, false);
    r.unwrap();
    let bb = brep_bounding_box(&k, &b, true).unwrap();
    assert!(
        // 6.544244339992678e-3 with the front round's cylinder on the solved stations; on
        // its exact spine (#2405) the corner patch moves 8.1e-7
        (bb.min_at(1) + 6.543433008971893e-3).abs() < 1e-9,
        "min y {}",
        bb.min_at(1)
    );
    let v = volume(&k, &mut b);
    // 9402.870799856591 with the front round on the solved stations (#2405)
    assert!((v - 9402.870802612037).abs() < 1e-6, "volume {v}");
    assert!(check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .is_valid());
}
