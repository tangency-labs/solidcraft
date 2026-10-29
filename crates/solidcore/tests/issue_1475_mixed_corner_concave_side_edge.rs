//! tangency-labs/solidcore#1475 (WR-524): a bar 100 x 30 x 10 (x 0..100, y -15..15)
//! joined with a boss r 20, height 20, on the origin (SolidCraft rig case fillets/1207).
//! The bar's top front edge (y = -15, z = 10) and the boss's concave base arc over the
//! bar (both halves, split at the cylinder's seam) are rounded r 2 together. They meet
//! at (13.229, -15, 10), where the third edge, the boss's line on the bar's front face,
//! is concave and runs down from the vertex: a 3x2 mixed-convexity corner whose
//! unfilleted edge is concave. `SmFillet3x2MixedConvexityCorner::MakeCornerTopology`
//! gives the fillets the roles of an L-shaped box (convex unfilleted edge) and fails
//! with status 1001. The opt-in `Kernel::fixes.fillet_3x2_mixed_concave_side_edge`
//! lets the roles follow the unfilleted edge's convexity, as tangency-labs/smlib@1255993.
//!
//! The operand is the C++ union's body as SolidCraft hands it over (its flat arrays,
//! written with `BrepData::write_to_file`), read back with SolidCraft's `BrepFromFlat`
//! options; solidcore's own boolean builds a different body (see the issue).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Vec3;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p10::{
    make_topology_from_data, MakeTopologyOptions, TopologyEnv, HEALER_VERSION,
};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep_data::BrepData;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{Brep, EdgeId};
use solidcore::{EntityRef, Kernel};

const BODY: &[u8] = include_bytes!("data/issue_1475_bar_and_boss.smd");

/// SolidCraft's topology identities (type code, entity index).
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

/// The body as SolidCraft's `BrepFromFlat` builds it: no UV trims, no analytics, same
/// tolerances, healer on.
fn load(k: &mut Kernel) -> Brep {
    let mut data = BrepData::read_from_file(k, BODY.to_vec(), FileType::Ascii).unwrap();
    data.set_healer_version(HEALER_VERSION);
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut store = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let next_key = (data.uv_curves.len() + data.curves_3d.len() + data.surfaces.len()) as u64 + 1;
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut store,
        key_of: &key_of,
        records: &mut records,
        next_key,
    };
    let options = MakeTopologyOptions {
        make_uv_trim_curves: false,
        add_analytics: false,
        force_same_tol: true,
        check_uv_trim_curves: false,
        always_heal: false,
        healer_enabled: true,
    };
    make_topology_from_data(k, &mut b, &mut data, &mut env, options).unwrap();
    b
}

/// The edge nearest `p` (SolidCraft's pick: its points are display-mesh points, off the arc).
fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let mut best = (f64::MAX, None);
    for e in bp1::edges(b, None).unwrap() {
        if let Ok(c) = edge_closest_point(k, b, e, Vec3::new(p[0], p[1], p[2])) {
            if c.closest.distance < best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    assert!(best.0 < 0.5, "no edge at {p:?}");
    best.1.unwrap()
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}

/// Rounds the bar's top front edge and both halves of the boss's base arc as
/// `SmApiFilletEdgesPerEdge` does; `Err` carries the fillet error info code.
fn round(fix: bool, radius: f64) -> (Kernel, Brep, Result<(), (solidcore::SolidError, i32)>) {
    let mut k = Kernel::default();
    k.fixes.fillet_3x2_mixed_concave_side_edge = fix;
    let mut b = load(&mut k);
    let edges: Vec<EdgeId> = [
        [56.614_378_277_661_47, -15.0, 10.0],
        [14.793_060_209_241_245, 13.254_056_122_601_737, 10.0],
        [19.849_045_376_133_382, -2.343_832_202_869_714, 10.0],
    ]
    .iter()
    .map(|&p| pick(&k, &b, p))
    .collect();
    let r = {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        let step = s
            .prepare_per_edge_constant_radius(
                &mut k,
                &edges,
                &[radius; 3],
                &[FilletSurfaceGeneratorType::Circular],
                1,
                1.0,
            )
            .and_then(|()| s.create_corners(&k, None))
            .and_then(|()| s.do_filleting(&mut k, None));
        let code = s.error_info().code;
        step.map_err(|e| (e, code))
    };
    b.refresh_cached_topology();
    if r.is_ok() {
        stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
        b.refresh_cached_topology();
    }
    (k, b, r)
}

/// The unrounded body: the bar, the boss, less the half-disk of the boss over the bar
/// (|y| <= 15, x >= 0) through the bar's height.
fn body_volume() -> f64 {
    let pi = core::f64::consts::PI;
    let overlap = 10.0 * (15.0 * 175f64.sqrt() + 400.0 * 0.75f64.asin());
    100.0 * 30.0 * 10.0 + pi * 20.0 * 20.0 * 20.0 - overlap
}

#[test]
fn operand_is_the_bar_and_boss() {
    let mut k = Kernel::default();
    let mut b = load(&mut k);
    let v = volume(&k, &mut b);
    let expected = body_volume();
    assert!((v - expected).abs() < 1e-6 * expected, "{v} vs {expected}");
}

#[test]
fn corner_fails_with_upstream_behaviour() {
    for radius in [1.0, 2.0] {
        let (_, _, r) = round(false, radius);
        let (e, code) = r.unwrap_err();
        assert!(
            matches!(e, solidcore::SolidError::Upstream { code: 1001, .. }),
            "{e:?}"
        );
        // SM_FILERR_VERTEX_PROBLEM: "Corner Geometry Computation Failure"
        assert_eq!(code, 3);
    }
}

#[test]
fn corner_builds_with_the_fix() {
    for radius in [1.0, 2.0] {
        let (k, mut b, r) = round(true, radius);
        r.unwrap();
        assert!(is_manifold_solid(&b).unwrap());
        // One closed shell: V - E + F = 16 - 25 + 11 = 2.
        assert_eq!(b.face_list().len(), 11);
        assert_eq!(b.edge_list().len(), 25);
        assert_eq!(b.vertex_list().len(), 16);
        if radius == 2.0 {
            // The issue's estimate: the arc round's spandrel strip ending on y = +-15 adds
            // 28.9190 (the arc round alone builds to exactly that), the front round's
            // spandrel out to the boss wall removes 74.0736; the corner patch lies inside
            // the (2r)^3 cube about the vertex.
            let v = volume(&k, &mut b);
            let estimate = body_volume() + 28.9190 - 74.0736;
            assert!(
                (v - estimate).abs() < (2.0 * radius).powi(3),
                "{v} vs {estimate}"
            );
        }
    }
}
