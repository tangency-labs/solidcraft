//! Imports kept as records build the bodies a STEP import builds: the step-conformance parts
//! (cones, cylinders, a dome, a NURBS torus ring, a washer, a sheet) read by the STEP reader into
//! records, then made into bodies from those records alone. Records from a file are hostile: a
//! broken one fails its import with the reason, never a panic.

use super::super::{IDENTITY, StepNode, step_import};
use super::{ImportRecords, RecordBody, records_import};
use crate::measure;

/// The records the STEP reader assembles from `text`, kept as an import.
fn kept(text: &str) -> ImportRecords {
    let ex = solidcore_step::p21::parse(text).unwrap();
    let read = solidcore_step::topology::read_solids_with(&ex, &solidcraft_smlib::pcurves::SmlibTrims);
    assert!(read.failures.is_empty(), "{:?}", read.failures);
    let bodies: Vec<RecordBody> = read
        .solids
        .into_iter()
        .map(|s| RecordBody {
            name: s.name,
            path: Vec::new(),
            instance: None,
            color: None,
            hidden: false,
            placement: None,
            brep: s.brep,
            face_origin: s.face_origin,
            sheet: s.sheet,
        })
        .collect();
    let tree = vec![StepNode { name: "part".into(), transform: IDENTITY, bodies: (0..bodies.len()).collect(), children: Vec::new() }];
    ImportRecords { bodies, tree, warnings: Vec::new(), system: String::new() }
}

#[test]
fn records_build_the_bodies_the_step_text_builds() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    for part in ["cone_point", "cylinder_seam", "dome", "dome_nurbs", "torus_ring_nurbs", "washer", "grooved_shaft", "sheet_disc"] {
        let text = std::fs::read_to_string(dir.join(format!("{part}.step"))).unwrap();
        let from_step = step_import(&text).unwrap();
        // Only parts the reader reads whole compare: the records are what it assembled.
        if from_step.bodies.iter().any(|b| b.refused.is_some()) {
            continue;
        }
        let from_records = records_import(&kept(&text)).unwrap_or_else(|e| panic!("{part}: {e}"));
        assert_eq!(from_records.bodies.len(), from_step.bodies.len(), "{part}");
        for (r, s) in from_records.bodies.iter().zip(&from_step.bodies) {
            let (mr, ms) = (measure(&r.body).unwrap(), measure(&s.body).unwrap());
            assert_eq!((mr.faces, r.closed, &r.face_origin), (ms.faces, s.closed, &s.face_origin), "{part}");
            assert!((mr.volume - ms.volume).abs() <= 1e-9 * ms.volume.abs().max(1.0), "{part}: {} vs {}", mr.volume, ms.volume);
        }
    }
}

#[test]
fn a_record_placed_moves_its_body() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    let text = std::fs::read_to_string(dir.join("washer.step")).unwrap();
    let mut records = kept(&text);
    let at = records_import(&records).unwrap();
    // Column-major: the last column is the move.
    records.bodies[0].placement = Some([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [5.0, 0.0, 0.0, 1.0]]);
    let moved = records_import(&records).unwrap();
    let (a, b) = (measure(&at.bodies[0].body).unwrap(), measure(&moved.bodies[0].body).unwrap());
    assert!((b.centroid.x - a.centroid.x - 5.0).abs() < 1e-9 && (b.volume - a.volume).abs() < 1e-9, "{a:?} {b:?}");
}

#[test]
fn broken_records_fail_their_import_with_the_reason() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    let good = kept(&std::fs::read_to_string(dir.join("cone_point.step")).unwrap());
    // A face origin missing.
    let mut r = good.clone();
    r.bodies[0].face_origin.pop();
    assert!(records_import(&r).unwrap_err().to_string().contains("face origins"));
    // An edge naming a vertex the record does not have.
    let mut r = good.clone();
    r.bodies[0].brep.edges[0].vertices[0] = 999;
    assert!(records_import(&r).unwrap_err().to_string().contains("invalid"));
    // A product naming a body the import does not have.
    let mut r = good.clone();
    r.tree[0].bodies.push(7);
    assert!(records_import(&r).unwrap_err().to_string().contains("names a body"));
    // A placement that is not finite.
    let mut r = good;
    r.bodies[0].placement = Some([[f64::NAN; 4]; 4]);
    assert!(records_import(&r).unwrap_err().to_string().contains("not finite"));
}
