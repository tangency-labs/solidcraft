//! The goldens row `blend.fillet_executive_p3` `issue_2020_mitred_put_back_d9_full`
//! (tangency-labs/solidcore#2020's put-back) in closed form: the four r3 rounds put back on
//! the 40 x 30 box whose top was moved down 9, so the slab is 1 high (`0 <= z <= 1`).
//!
//! Each round is a cylinder of radius 3 whose axis runs 3 inside its side and 3 below the
//! top (`z = -2`). The slab's side walls are only 1 high, so each round runs past its wall
//! (a cliff overflow) and is trimmed at its crease with the bottom (WR-695): the walls go,
//! and the round meets the bottom at `s(0) = sqrt(9 - 4) = sqrt(5)` outside its axis. The
//! rounds meet in mitres on the diagonals. At height z the section is the rectangle
//! `[3 - s, 37 + s] x [3 - s, 27 + s]` with `s(z) = sqrt(9 - (z + 2)^2)`, so
//!
//! - the eight vertices are the top's `(3 | 37, 3 | 27, 1)` and the bottom's
//!   `(a | 40 - a, a | 30 - a, 0)` with `a = 3 - sqrt(5)`;
//! - the volume is `int_0^1 (34 + 2s)(24 + 2s) dz = 816 + 116 S1 + 4 S2`, with
//!   `S2 = int_0^1 s^2 dz = 8/3` and
//!   `S1 = int_2^3 sqrt(9 - t^2) dt = 9 pi / 4 - sqrt(5) - (9/2) asin(2/3)`.
//!
//! The result is one manifold solid of 8 vertices, 12 edges and 6 faces (top, bottom and
//! the four rounds), clean at `check_brep` Level2.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::brep::{p10::TopologyEnv, p7};
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::Brep;
use solidcore::{EntityRef, Kernel, Point3};

/// The goldens reader's object keys (`goldens::own_reader::read_key_of`).
fn read_key_of(e: EntityRef) -> ObjectRef {
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

/// `mitred_put_back_d9.smb` read as the goldens row reads it.
fn put_back_body(k: &mut Kernel) -> Brep {
    let bytes =
        include_bytes!("../../../fixtures/blend.fillet_executive_p3/mitred_put_back_d9.smb")
            .to_vec();
    let mut body = Brep::new_with_infinite_region().unwrap();
    let mut attributes = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &read_key_of,
        records: &mut records,
        next_key: 1u64 << 50,
    };
    p7::read_from_file(k, &mut body, bytes, FileType::Ascii, false, &mut env).unwrap();
    body.use_arena_naming().unwrap();
    body
}

#[test]
fn the_put_back_rounds_past_the_one_high_walls_have_their_closed_form() {
    let mut k = Kernel::default();
    let mut body = put_back_body(&mut k);
    let edges = p1::edges(&body, None).unwrap();
    // The row's edges 5, 6, 7, 4: the top's four edges.
    let selected: Vec<_> = [5, 6, 7, 4].iter().map(|&i| edges[i]).collect();
    for &e in &selected {
        let edge = body.edge(e).unwrap();
        for v in [edge.interval.min(), edge.interval.max()] {
            let z = edge
                .curve
                .as_ref()
                .unwrap()
                .evaluate_point(&k, v)
                .unwrap()
                .z;
            assert!(
                (z - 1.0).abs() < 1e-12,
                "a selected edge is not on the top: z {z}"
            );
        }
    }
    fillet_edges(&mut k, &mut body, &selected, 3.0, 1, 1, 1.0).unwrap();

    assert!(is_manifold_solid(&body).unwrap());
    let counts = (
        p1::vertices(&body, None).unwrap().len(),
        p1::edges(&body, None).unwrap().len(),
        p1::faces(&body, None).unwrap().len(),
    );
    assert_eq!(counts, (8, 12, 6), "vertices, edges, faces");
    let check = check_brep(&mut k, &mut body, CheckLevel::Level2).unwrap();
    assert_eq!(
        check.assert_valid,
        Some(true),
        "check_brep Level2: {check:?}"
    );
    let lines = check.lines();
    assert!(lines.is_empty(), "check_brep Level2: {lines:?}");

    let a = 3.0 - 5.0_f64.sqrt();
    let mut expected = Vec::new();
    for (x, y, z) in [
        (3.0, 3.0, 1.0),
        (37.0, 3.0, 1.0),
        (37.0, 27.0, 1.0),
        (3.0, 27.0, 1.0),
        (a, a, 0.0),
        (40.0 - a, a, 0.0),
        (40.0 - a, 30.0 - a, 0.0),
        (a, 30.0 - a, 0.0),
    ] {
        expected.push(Point3::new(x, y, z));
    }
    let mut unmatched = expected.clone();
    for v in p1::vertices(&body, None).unwrap() {
        let p = body.vertex(v).unwrap().point;
        let (i, d) = unmatched
            .iter()
            .enumerate()
            .map(|(i, q)| (i, (p - *q).length()))
            .min_by(|x, y| x.1.total_cmp(&y.1))
            .unwrap();
        assert!(d <= 1e-9 * 40.0, "vertex {p:?} is {d} from the closed form");
        unmatched.remove(i);
    }
    assert!(unmatched.is_empty());

    let s1 = 9.0 * std::f64::consts::PI / 4.0 - 5.0_f64.sqrt() - 4.5 * (2.0_f64 / 3.0).asin();
    let s2 = 8.0 / 3.0;
    let closed = 816.0 + 116.0 * s1 + 4.0 * s2;
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let v = brep_volume(&k, &mut body, 1e-10, &attrs).unwrap();
    assert!(
        (v - closed).abs() <= 1e-9 * closed,
        "volume {v} against the closed form {closed} (rel {:e})",
        (v - closed).abs() / closed
    );
}
