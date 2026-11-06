//! The kernel rig's corpus (`crates/solidcore/tests/kernel_cases/`) for the families' tests: an
//! entry, its operand bodies as kernel bodies with their record order, the values the rig
//! measured, and the rig's fix set.
// Test support: it reads files and panics on what it does not find.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::disallowed_methods
)]

use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde_json::Value;
use solidcore::mass::integrator::properties::brep_area;
use solidcore_convert::brep::{to_kernel, Order};

use crate::session::{self, run};
use crate::{Body, CheckLevel, Error};

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../solidcore/tests/kernel_cases")
}

/// The entry `case` (`<area>/<case>`).
pub(crate) fn entry(case: &str) -> Value {
    let path = corpus().join(format!("{case}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap()
}

/// Operand `name` of `case`, made from its record as the rig made it (trusted or healed), and
/// its order.
pub(crate) fn operand(case: &str, name: &str) -> (Body, Order) {
    let e = entry(case);
    let gz = corpus()
        .join(case)
        .with_file_name(e["records"].as_str().unwrap());
    let mut text = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(&gz).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    let all: serde_json::Map<String, Value> = serde_json::from_str(&text).unwrap();
    let record: solidcore_record::brep::Brep = serde_json::from_value(all[name].clone()).unwrap();
    let trusted = e["bodies"][name]["trusted"].as_bool().unwrap();
    let made = run("to_kernel", |k| {
        Ok(match to_kernel(k, &record, trusted) {
            Ok((b, o)) => Ok(Some((b, o))),
            Err(e) => Err(Error::Record(format!("{e:?}"))),
        })
    });
    let (b, o) = made.unwrap().unwrap().unwrap();
    (Body::new(b), o)
}

/// A body's order: the kernel's own lists (a body ops made, not read from a record).
pub(crate) fn kernel_order(b: &Body) -> Order {
    let g = b.lock();
    Order {
        faces: g.face_list().to_vec(),
        edges: g.edge_list().to_vec(),
        vertices: g.vertex_list().to_vec(),
    }
}

/// What the rig measured, summed over the case's result bodies.
#[derive(Debug)]
pub(crate) struct Measured {
    pub(crate) volume: f64,
    pub(crate) area: f64,
    pub(crate) counts: [usize; 3],
}

pub(crate) fn rig(e: &Value) -> Measured {
    let m = &e["rig"]["measured"];
    assert_eq!(e["rig"]["status"], "pass");
    Measured {
        volume: m["volume_mm3"].as_f64().unwrap(),
        area: m["area_mm2"].as_f64().unwrap(),
        counts: [
            m["faces"].as_u64().unwrap() as usize,
            m["edges"].as_u64().unwrap() as usize,
            m["vertices"].as_u64().unwrap() as usize,
        ],
    }
}

/// The corpus's raw counts (the result records' own).
pub(crate) fn raw_counts(e: &Value) -> [usize; 3] {
    let c = &e["outcome"]["raw_counts"];
    ["faces", "edges", "vertices"].map(|k| c[k].as_u64().unwrap() as usize)
}

/// The rig's fix set (the corpus's `fixes.json`), as SolidCraft's rig ran.
pub(crate) fn rig_fixes() {
    let text = std::fs::read_to_string(corpus().join("fixes.json")).unwrap();
    let names: Vec<String> = serde_json::from_str(&text).unwrap();
    session::set_thread_fixes(names.iter().map(String::as_str)).unwrap();
}

/// `|a - b| / |b|`.
pub(crate) fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs()
}

/// The kernel's area at the rig's accuracy.
pub(crate) fn area(b: &Body) -> f64 {
    session::measure_body("brep_area", b, |k, b| brep_area(k, b, 1e-8)).unwrap()
}

/// The kernel's every check passes.
pub(crate) fn valid(what: &str, b: &Body) {
    let r = b.check(CheckLevel::All).unwrap();
    assert!(r.is_valid(), "{what}: {r:?}");
}
