//! #2953: the drafted peg's front top edge (convex) and concave arc rounded 2 together, the
//! tangent peg of #2360 extruded with a 5 degree inward taper. The bar's drafted front face runs
//! tangent into the cone along its generator through x = 40, and the top's cone is the same cone
//! split along that generator, so a fourth edge runs on from the seam past the vertex
//! V = (40, y0, 10), y0 = 10 tan 5 degrees.
//!
//! The corner at V is the blend corner's Roll-over at a cusp (#22151). The top's cone is in two
//! faces, split along the generator through V, so V has a fourth edge: the Roll-over takes the
//! split out first (#23441), and the corner is the one of three edges the one-face cone has.

#![allow(
    clippy::disallowed_methods, // closed-form oracles, not ops geometry
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test

#[path = "../../solidcore/tests/support/solidcraft_fixes.rs"]
mod solidcraft_fixes;

mod common;

use common::edge_at;
use serde_json::Value;
use solidcore_ops::blend::{blend, Blend, Corners};
use solidcore_ops::{session, Body, CheckLevel};
use solidcore_record::brep::Brep as Record;
use std::io::Read as _;

const CASE: &str = "report-drafted-peg-front-and-concave-rounds-together-r2";
const CORPUS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../solidcore/tests/kernel_cases/fillets"
);

/// The record the case's blend takes: its operand followed back through the copies and tags
/// to a body the entry gives.
fn operand() -> Record {
    let entry: Value =
        serde_json::from_str(&std::fs::read_to_string(format!("{CORPUS}/{CASE}.json")).unwrap())
            .unwrap();
    let calls = entry["calls"].as_array().unwrap();
    let given = entry["bodies"].as_object().unwrap();
    let blend_call = calls
        .iter()
        .find(|c| c["op"].as_str().unwrap().starts_with("blend"))
        .unwrap();
    let mut id = blend_call["inputs"][0].as_str().unwrap().to_owned();
    while !given.contains_key(&id) {
        let made = calls
            .iter()
            .find(|c| {
                c["outputs"]
                    .as_array()
                    .is_some_and(|o| o.iter().any(|x| x.as_str() == Some(id.as_str())))
            })
            .unwrap();
        id = made["inputs"][0].as_str().unwrap().to_owned();
    }
    let gz = std::fs::File::open(format!("{CORPUS}/{CASE}.bodies.json.gz")).unwrap();
    let mut text = String::new();
    flate2::read::GzDecoder::new(gz)
        .read_to_string(&mut text)
        .unwrap();
    let mut records: Value = serde_json::from_str(&text).unwrap();
    serde_json::from_value(records[&id].take()).unwrap()
}

/// The volume the rounds remove from the same peg built with the top's cone in one face (its
/// end circle extruded and joined on its own, no projected split): the solidcraft-cli fillet
/// rig's `measured.volume_mm3` before and after, 8291.612193903850 - 8277.104337962608.
const ONE_FACE_REMOVES: f64 = 14.507_855_941_242;

#[test]
fn the_rounds_together_build_through_the_cusp() {
    session::set_thread_fixes(solidcraft_fixes::solidcraft_fix_names()).unwrap();
    let (body, _) = Body::from_record(&operand(), true).unwrap();
    let y0 = 10.0 * 5.0_f64.to_radians().tan();
    let picks = [
        edge_at(&body, [20.0, y0, 10.0]),
        edge_at(&body, [32.0 + y0, 8.0, 10.0]),
    ];
    let before = body.volume(1e-10).unwrap();
    blend(&body, Blend::Round, &picks, &[2.0], &[], Corners::Filled).unwrap();
    assert!(body.check(CheckLevel::All).unwrap().is_valid());
    let removed = before - body.volume(1e-10).unwrap();
    // the split taken out at the cusp, the corner is the Roll-over's of three edges, as on the
    // one-face cone: the same round. The two builds put the cone's seam in different places
    // and agree to 1.53e-5 (1.1e-6 of what they remove); a corner of four edges, or the Mixed
    // corner's Coons fill, is a different round
    assert!(
        (removed - ONE_FACE_REMOVES).abs() < 2e-5,
        "removed {removed}, the one-face cone's {ONE_FACE_REMOVES}"
    );
}
