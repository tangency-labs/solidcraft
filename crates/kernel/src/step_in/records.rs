//! Imports kept as B-rep records instead of STEP text: what a file read through cadmpeg's neutral
//! model (CADIR, `solidcore_cadir`) becomes. Each body is the record the reader assembled, with
//! the faces of the file it came from and where it goes; the bodies are made from the records as
//! a STEP import makes them from the records its reader assembles (SMLib takes each one as given,
//! [`super::parallel`]), so both kinds of import give the same bodies for the same solid.
//!
//! The records are saved in the design: it reopens without the original file, as a STEP import
//! does. They come back from a file, so they are checked as hostile input before any is used.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use solidcore_record::brep::Brep;

use super::parallel::{Part, placed, solid_part};
use super::{ImportIssue, ImportReport, ImportedBody, IssueKind, M4, StepImport, StepNode};
use crate::{KernelError, Result};

/// An import kept as records.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImportRecords {
    /// The bodies, in the file's order.
    pub bodies: Vec<RecordBody>,
    /// The file's product tree; its nodes' `bodies` index [`ImportRecords::bodies`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tree: Vec<StepNode>,
    /// What the file's reader could not bring over.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// The program that wrote the file, when it says.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub system: String,
}

/// One body of an import kept as records.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecordBody {
    /// The body's own name, else its product's.
    pub name: String,
    /// Product names from the root to the part that holds the body.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<String>,
    /// The name of the occurrence that holds it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    /// Surface colour, linear RGB in 0..1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[f32; 3]>,
    /// The file hides it: the design starts with it hidden.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    /// Where the record goes (column-major 4 × 4, millimetres), when not where it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<M4>,
    /// The record, in its own coordinates.
    pub brep: Brep,
    /// The file's face of each of the record's faces (an index the file's reader names it by).
    pub face_origin: Vec<u64>,
    /// No closed shell: a surface body.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sheet: bool,
}

/// The bodies an import kept as records makes, with its tree and warnings: what a STEP import
/// gives, so a design uses either the same way.
pub fn records_import(records: &ImportRecords) -> Result<StepImport> {
    let _span = solidcraft_profile::span("import", "records.import");
    if records.bodies.len() > super::MAX_BODIES {
        return Err(KernelError::Invalid(format!("an import of {} bodies (at most {})", records.bodies.len(), super::MAX_BODIES)));
    }
    check_tree(&records.tree, records.bodies.len(), 0)?;
    let mut bodies = Vec::with_capacity(records.bodies.len());
    let mut report = ImportReport { parts: records.bodies.len(), ..ImportReport::default() };
    let mut warnings = records.warnings.clone();
    for w in &records.warnings {
        report.issues.push(ImportIssue { kind: IssueKind::File, item: None, part: String::new(), faces: 0, times: 1, detail: w.clone() });
    }
    // A body SMLib refuses is left out, as a STEP import leaves out a solid it cannot make, and
    // the others still come in: `kept[k]` is where record body `k` is among the bodies made.
    let mut kept = vec![None; records.bodies.len()];
    for (k, r) in records.bodies.iter().enumerate() {
        let label = if r.name.is_empty() { format!("body {}", k + 1) } else { r.name.clone() };
        let made =
            part_of(r).and_then(|part| placed(&part.body, &r.placement.unwrap_or(super::IDENTITY)).map(|b| (part, b)).map_err(|e| e.to_string()));
        let (part, body) = match made {
            Ok(made) => made,
            Err(why) => {
                let detail = format!("{label}: not imported: {why}");
                report.issues.push(ImportIssue {
                    kind: IssueKind::NotImported,
                    item: Some(k as u64),
                    part: r.name.clone(),
                    faces: 0,
                    times: 1,
                    detail: detail.clone(),
                });
                warnings.push(detail);
                continue;
            }
        };
        // Each face known by its record body and its face in the file (from 1: a certificate
        // takes no 0), as a STEP import knows its faces by their entities.
        let item = k as u64 + 1;
        let sources =
            part.face_origin.iter().map(|f| Some(crate::FaceSource::Imported { item, face: f + 1, payload_sha256: None, producer: None })).collect();
        let body = body.with_color(r.color).with_face_sources(Some(Arc::new(sources)));
        if part.closed {
            report.solids += 1;
        }
        kept[k] = Some(bodies.len());
        bodies.push(ImportedBody {
            source_item: k as u64,
            face_origin: part.face_origin,
            name: r.name.clone(),
            body,
            color: r.color,
            path: r.path.clone(),
            instance: r.instance.clone(),
            closed: part.closed,
            file_faces: part.file_faces,
            refused: None,
            hidden: r.hidden,
        });
    }
    Ok(StepImport {
        bodies,
        tree: renumbered(&records.tree, &kept),
        warnings,
        report,
        schema: String::new(),
        system: records.system.clone(),
        preprocessor: String::new(),
        description: String::new(),
    })
}

/// `nodes` with each body index `k` the made body `kept[k]` is, and the bodies left out dropped.
fn renumbered(nodes: &[StepNode], kept: &[Option<usize>]) -> Vec<StepNode> {
    nodes
        .iter()
        .map(|n| StepNode {
            name: n.name.clone(),
            transform: n.transform,
            bodies: n.bodies.iter().filter_map(|b| kept.get(*b).copied().flatten()).collect(),
            children: renumbered(&n.children, kept),
        })
        .collect()
}

/// [`records_import`] through the recent imports' cache ([`super::step_import_shared`]'s), keyed
/// by the records' serialised form.
pub fn records_import_shared(records: &ImportRecords) -> Result<Arc<StepImport>> {
    let text = records_text(records)?;
    let key = super::text_key(&text);
    if let Some(imp) = super::recent(key) {
        return Ok(imp);
    }
    let imp = Arc::new(records_import(records)?);
    super::remember(key, &imp, text.len() <= super::KEEP_SMALL_BYTES);
    Ok(imp)
}

/// The records' serialised form: what an import kept as records is, as STEP text is what a STEP
/// import is (its cache key and its bodies' certificates are of this).
pub fn records_text(records: &ImportRecords) -> Result<String> {
    serde_json::to_string(records).map_err(|e| KernelError::Invalid(format!("import records: {e}")))
}

/// The body SMLib makes of one record, checked first: a record from a file is hostile input.
fn part_of(r: &RecordBody) -> std::result::Result<Part, String> {
    if r.face_origin.len() != r.brep.faces.len() {
        return Err(format!("{} face origins for {} faces", r.face_origin.len(), r.brep.faces.len()));
    }
    if let Some(m) = r.placement
        && m.iter().flatten().any(|x| !x.is_finite())
    {
        return Err("a placement that is not finite".into());
    }
    solidcore_ops::record_check::validate(&r.brep).map_err(|e| format!("the record is invalid: {e}"))?;
    solidcore_ops::record_check::agreement(&r.brep).map_err(|e| format!("the record's geometry disagrees: {e}"))?;
    // A sheet is what its record is, whatever the file says: no bounded region but the
    // infinite one's.
    let sheet = r.sheet || r.brep.regions.len() <= 1;
    let solid = solidcore_step::topology::Solid {
        item: 0,
        name: r.name.clone(),
        product: None,
        brep: r.brep.clone(),
        face_origin: r.face_origin.clone(),
        sheet,
        notes: Vec::new(),
        turned: Vec::new(),
    };
    solid_part(&solid).map_err(|f| f.msg)
}

/// Every node's bodies index one of `count` bodies, and the tree is no deeper than an assembly
/// the STEP reader reads.
fn check_tree(nodes: &[StepNode], count: usize, depth: usize) -> Result<()> {
    if depth > super::MAX_ASSEMBLY_DEPTH {
        return Err(KernelError::Invalid("an import's product tree nested too deeply".into()));
    }
    for n in nodes {
        if n.bodies.iter().any(|b| *b >= count) {
            return Err(KernelError::Invalid(format!("product `{}` names a body the import does not have", n.name)));
        }
        if n.transform.iter().flatten().any(|x| !x.is_finite()) {
            return Err(KernelError::Invalid(format!("product `{}` has a placement that is not finite", n.name)));
        }
        check_tree(&n.children, count, depth + 1)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "records_tests.rs"]
mod tests;
