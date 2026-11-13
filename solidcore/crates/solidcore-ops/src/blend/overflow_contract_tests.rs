//! R10 on a 5 x 5 bar consumes both original supports: its new seams cross, not touch.
//!
//! The section is the square intersected with the circle centred at (y,z)=(10,-5),
//! radius 10. Its two surviving long cylinder/plane seams have outward-normal dot
//! product -1/2 (120 degrees), not the nonexistent original supports' G1 contact.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)] // Independent closed-form oracle and corpus test data, not ops geometry.

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::blend::{blend, Blend, Corners};
use crate::{session, Body};
use serde_json::Value;
use solidcore_record::brep::Brep;

const CASE: &str = "4782-fillet-bar-edge-wider-than-both-faces";

/// The shared kernel-case corpus.
fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../solidcore/tests/kernel_cases")
}

/// The operation and operand record for the oversized-round case.
fn entry() -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(corpus().join(format!("fillets/{CASE}.json"))).unwrap(),
    )
    .unwrap()
}

/// The rig grades the result's geometry; this checks the two remaining contracts.
#[test]
fn the_oversized_round_keeps_its_operand_and_definition() {
    let root = corpus();
    let fixes: Vec<String> =
        serde_json::from_str(&std::fs::read_to_string(root.join("fixes.json")).unwrap()).unwrap();
    session::set_fixes(fixes.iter().map(String::as_str)).unwrap();
    let entry = entry();
    let path = root
        .join("fillets")
        .join(entry["records"].as_str().unwrap());
    let mut text = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    let records: Value = serde_json::from_str(&text).unwrap();
    let record: Brep = serde_json::from_value(records["b2"].clone()).unwrap();
    let (input, order) = Body::from_record(&record, true).unwrap();
    let before = input.to_record().unwrap().0;
    let (body, _) = input.copy_with_order(&order).unwrap();
    blend(&body, Blend::Round, &[1], &[10.0], &[], Corners::Filled).unwrap();

    // Inspect the live kernel body, not an .smb round-trip: retained exact-class
    // metadata lives beside the analytic surface and is not the stored analytic kind.
    let result = body.lock();
    let kernel = solidcore::Kernel::default();
    let mut curved = 0;
    for (_, face) in result.faces().iter() {
        let surface = face.surface.as_ref().unwrap();
        if !surface.is_planar(&kernel, 1e-9).unwrap().is_planar {
            curved += 1;
            assert!(surface.procedural_blend().is_some(), "the round is bare");
        }
    }
    assert_eq!(
        curved, 1,
        "exactly one round face must carry its definition"
    );
    assert_eq!(input.to_record().unwrap().0, before, "operand changed");
}
