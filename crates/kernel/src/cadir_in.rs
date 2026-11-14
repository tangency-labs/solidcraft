//! FreeCAD, Rhino and other files decoded by cadmpeg into its neutral model (CADIR), read into
//! an import kept as B-rep records ([`ImportRecords`]): each body region assembled into a record
//! by `solidcore_cadir`, its pcurves without a closed form made by SMLib as a STEP import's are.
//! Nothing passes through STEP text.
//!
//! The file's bodies are one product named after the file: CADIR's bodies come with their own
//! names, visibility and rigid placements, which the records keep.

use cadmpeg_ir::document::CadIr;

use crate::{ImportRecords, RecordBody, StepNode};

/// `ir`'s bodies as an import kept as records, one product called `product`, written by
/// `system`. What could not be read (and what was repaired on the way) is in its warnings,
/// each naming its body.
pub fn cadir_records(ir: &CadIr, product: &str, system: &str) -> ImportRecords {
    let _span = solidcraft_profile::span("import", "cadir.records");
    let read = solidcore_cadir::read_bodies(ir, Some(&solidcraft_smlib::pcurves::SmlibTrims));
    let by_id: std::collections::HashMap<&str, &cadmpeg_ir::topology::Body> = ir.model.bodies.iter().map(|b| (b.id.as_str(), b)).collect();
    let label = |id: &str| by_id.get(id).and_then(|b| b.name.clone()).filter(|n| !n.trim().is_empty()).unwrap_or_else(|| id.to_string());
    let mut warnings = Vec::new();
    let mut bodies = Vec::with_capacity(read.read.len());
    for r in read.read {
        let body = by_id.get(r.body.as_str());
        warnings.extend(r.solid.notes.iter().map(|n| format!("{}: {n}", label(&r.body))));
        bodies.push(RecordBody {
            name: body.and_then(|b| b.name.clone()).unwrap_or_default(),
            path: vec![product.to_string()],
            instance: None,
            // Not yet: a Rhino object's colour is often its layer's default (black), which is not
            // how Rhino shades it; the STEP route brought no colour over either.
            color: None,
            hidden: body.is_some_and(|b| b.visible == Some(false)),
            placement: r.placement,
            brep: r.solid.brep,
            face_origin: r.solid.face_origin,
            sheet: r.solid.sheet,
        });
    }
    warnings.extend(read.failures.iter().map(|f| format!("{}: not imported: {}", label(&f.body), f.message)));
    let identity = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
    let tree = vec![StepNode { name: product.to_string(), transform: identity, bodies: (0..bodies.len()).collect(), children: Vec::new() }];
    ImportRecords { bodies, tree, warnings, system: system.to_string() }
}
