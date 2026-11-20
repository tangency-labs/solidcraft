//! A real STEP assembly, placed by `placed_bodies`.
//!
//! `tests/data/assembly_extract.stp` is an extract of an assembly exported by HOOPS Exchange,
//! with its product names made neutral (a root holding two instances of a foot, each made of a
//! washer and an angle bracket). Like many real exporters it writes every occurrence transformation
//! non-conformantly: `rep_1` is the child, yet `item_1` is a placement of the *parent*
//! representation. The instance `foot_24` sits at (52.125, -662.908, 0) turned a
//! quarter turn about Z; reading the transformation backwards put it at the opposite sign.

#![allow(clippy::expect_used, clippy::panic, clippy::float_arithmetic)] // test

use solidcore::geom::step_matrix::Matrix4;
use solidcore_ops::{placed_bodies, PlacedBody};
use solidcore_record::step::{p21, product};

const TOLERANCE: f64 = 1e-9;
/// `MANIFOLD_SOLID_BREP` of the 5 mm washer, 167.14 mm above the foot's origin.
const WASHER: u64 = 110_077;
/// `MANIFOLD_SOLID_BREP` of the 30-30 angle bracket, at the foot's origin.
const BRACKET: u64 = 110_364;
/// Height of the washer in the foot, from its `AXIS2_PLACEMENT_3D` #109980.
const WASHER_HEIGHT: f64 = 167.14;
/// Origin of the foot `foot_24` in the root, from `AXIS2_PLACEMENT_3D` #109952.
const FOOT_ORIGIN: [f64; 2] = [52.125_184_115, -662.908_156_057];

fn placed() -> Vec<PlacedBody> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/assembly_extract.stp"
    );
    let text = std::fs::read_to_string(path).expect("the extract is versioned beside the tests");
    let exchange = p21::parse(&text).expect("the extract parses");
    let structure = product::read(&exchange).expect("the extract has a product structure");
    placed_bodies(&structure).expect("the extract expands")
}

fn matches(got: &Matrix4, want: &Matrix4) -> bool {
    got.iter()
        .flatten()
        .zip(want.iter().flatten())
        .all(|(got, want)| (got - want).abs() < TOLERANCE)
}

/// The foot turned a quarter turn about Z (`x = +Y`), moved to `FOOT_ORIGIN`, `z` up.
fn turned_foot(z: f64) -> Matrix4 {
    [
        [0.0, 1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [FOOT_ORIGIN[0], FOOT_ORIGIN[1], z, 1.0],
    ]
}

/// The foot `foot_33`, at the origin of the root without a turn.
fn plain_foot(z: f64) -> Matrix4 {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, z, 1.0],
    ]
}

fn assert_placed(placed: &[PlacedBody], entity: u64, want: &Matrix4) {
    assert!(
        placed
            .iter()
            .any(|body| body.entity == entity && matches(&body.transform, want)),
        "no body #{entity} at {want:?} in {placed:?}"
    );
}

#[test]
fn a_real_assembly_places_each_foot_instance_at_its_real_offset() {
    let placed = placed();
    assert_eq!(placed.len(), 4, "two feet, a washer and a bracket each");
    // The turned foot: a wrong sign of the child's placement puts it at (-52.125, +662.908).
    assert_placed(&placed, WASHER, &turned_foot(WASHER_HEIGHT));
    assert_placed(&placed, BRACKET, &turned_foot(0.0));
    // The foot at the root's origin keeps its own height only.
    assert_placed(&placed, WASHER, &plain_foot(WASHER_HEIGHT));
    assert_placed(&placed, BRACKET, &plain_foot(0.0));
}
