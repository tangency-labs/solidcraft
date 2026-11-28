//! #2953: the drafted peg's front top edge (convex) and concave arc rounded 2 together, the
//! tangent peg of #2360 extruded with a 5 degree inward taper. The bar's drafted front face runs
//! tangent into the cone along its generator through x = 40, and the top's cone is the same cone
//! split along that generator, so a fourth edge runs on from the seam past the vertex
//! V = (40, y0, 10), y0 = 10 tan 5 degrees.
//!
//! The corner at V is the blend corner's Mixed one. Its faces are Coons patches of their sides,
//! not a rolling ball's surface, so since #22151 the round is refused there, naming V, until the
//! Roll-over build takes this corner.

#![allow(
    clippy::disallowed_methods, // closed-form oracles, not ops geometry
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test
use solidcore::math::det;

#[path = "../../solidcore/tests/support/solidcraft_fixes.rs"]
mod solidcraft_fixes;

mod common;

use common::edge_at;
use serde_json::Value;
use solidcore_ops::blend::{blend, Blend, Corners};
use solidcore_ops::{session, Body, Error};
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

#[test]
fn the_rounds_together_are_refused_at_the_cusp() {
    session::set_thread_fixes(solidcraft_fixes::solidcraft_fix_names()).unwrap();
    let (body, _) = Body::from_record(&operand(), true).unwrap();
    let y0 = 10.0 * det::tan(5.0_f64.to_radians());
    let picks = [
        edge_at(&body, [20.0, y0, 10.0]),
        edge_at(&body, [32.0 + y0, 8.0, 10.0]),
    ];
    let e = blend(&body, Blend::Round, &picks, &[2.0], &[], Corners::Filled).unwrap_err();
    let Error::CornerUnsupported { why, at } = &e else {
        panic!("not a corner refusal: {e}");
    };
    let why = why.as_deref().unwrap_or_default();
    assert!(why.contains("narrows to zero width"), "{why}");
    let [x, y, z] = at.unwrap();
    let off = det::hypot(det::hypot(x - 40.0, y - y0), z - 10.0);
    assert!(off < 1e-6, "refused at ({x}, {y}, {z})");
}
