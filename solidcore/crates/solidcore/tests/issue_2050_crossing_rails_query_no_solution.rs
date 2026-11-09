//! tangency-labs/solidcore#2050: `fixes.fillet_refuse_crossing_rails` (WR-460, #1073)
//! failed a sound round when a closest-point query on the other round's edge had no
//! solution (`edge_closest_point`'s 1001): a query with no solution finds no crossing
//! there, so it no longer fails the fillet.
//!
//! The body (`data/issue_2050_two_radii_y.smd`, written by `write_brep_to_file`) is a
//! 50 x 40 x 30 box with its vertical edges rounded R8. Its top loop, in the issue's edge
//! order, is rounded R4.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // test helpers

use std::f64::consts::PI;

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p3::brep_copy;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{edge, vertex, EdgeId};
use solidcore::{Brep, EntityRef, Kernel, Result};

/// The top loop's edges, in the order the issue passes them.
const TOP_LOOP: [u32; 8] = [20, 15, 1, 21, 23, 11, 7, 22];

/// `SmBrep::ReadFromFile` of the issue's body.
fn body(k: &mut Kernel) -> Brep {
    let bytes = include_bytes!("data/issue_2050_two_radii_y.smd").to_vec();
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
        next_key: 1 << 40,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b.refresh_cached_topology();
    b
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}

/// Round the top loop R4 with the check as given; the copy and the fillet's result.
fn round(refuse_crossing_rails: bool) -> (Kernel, Brep, Result<()>) {
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_crossing_rails = refuse_crossing_rails;
    let read = body(&mut k);
    let mut b = brep_copy(&mut k, &read).unwrap();
    let ids: Vec<EdgeId> = TOP_LOOP.iter().map(|&i| EdgeId::from_index(i)).collect();
    for &e in &ids {
        let (s, t) = edge::start_end_vertices(&b, e).unwrap();
        for v in [s, t] {
            let z = vertex::point(&b, v.unwrap()).unwrap().z;
            assert!((z - 30.0).abs() < 1e-6, "edge {e:?} not on the top");
        }
    }
    let result = {
        let mut session = FilletSession::new(&mut k, &mut b).unwrap();
        session
            .prepare_constant_radius_edges(&mut k, &ids, 4.0, 1, 1, 1.0)
            .unwrap();
        session.do_fillet(&mut k)
    };
    (k, b, result)
}

/// V = (2000 - (4 - pi) 64) 30 - 116 s - 2 pi (8 - 4 (10 - 3 pi)/(12 - 3 pi)) s with
/// s = (1 - pi/4) 16: the R8-cornered box less the R4 round's cross-section swept along
/// the straight edges (116 = 2 (34 + 24)) and round the four corners (Pappus).
fn expected() -> f64 {
    let s = (1.0 - PI / 4.0) * 16.0;
    (2000.0 - (4.0 - PI) * 64.0) * 30.0
        - 116.0 * s
        - 2.0 * PI * (8.0 - 4.0 * (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI)) * s
}

#[test]
fn issue_2050_round_with_crossing_check_builds() {
    let (k, mut b, result) = round(true);
    result.unwrap();
    let v = volume(&k, &mut b);
    let want = expected();
    assert!((want - 57800.239724).abs() < 1e-6, "closed form {want}");
    assert!(
        (v - want).abs() <= 1e-9 * want.abs(),
        "volume {v}, expected {want}"
    );
}

#[test]
fn issue_2050_round_without_check_builds() {
    let (k, mut b, result) = round(false);
    result.unwrap();
    let v = volume(&k, &mut b);
    let want = expected();
    assert!(
        (v - want).abs() <= 1e-9 * want.abs(),
        "volume {v}, expected {want}"
    );
}
