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

use super::parallel::{Part, placed, record_part};
use super::{ImportIssue, ImportReport, ImportedBody, IssueKind, LeftOut, M4, StepImport, StepNode};
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
    /// The record, in its own coordinates; none for a copy ([`RecordBody::copy_of`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brep: Option<Brep>,
    /// The file's face of each of the record's faces (an index the file's reader names it by);
    /// none for a copy.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub face_origin: Vec<u64>,
    /// Another placement of an earlier body of the import (its index), as an assembly that uses
    /// one part twice places it twice: this body is that one's record, faces and sheet, placed by
    /// its own `placement`, and the same part to the design (its faces are that body's). Its own
    /// `brep`, `face_origin` and `sheet` are not used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_of: Option<usize>,
    /// No closed shell: a surface body.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sheet: bool,
}

/// The pcurves an importer gives the shared assembly stage for edge uses with no closed form:
/// SMLib's own, as a STEP import's are. Readers outside the kernel (cadmpeg's neutral model, in
/// `solidcraft-io`) pass this, so only the kernel uses SMLib.
pub fn import_trims() -> &'static (dyn solidcore_ops::pcurve::Trims + Sync) {
    &solidcraft_smlib::pcurves::SmlibTrims
}

/// The bodies an import kept as records makes, with its tree and warnings: what a STEP import
/// gives, so a design uses either the same way.
pub fn records_import(records: &ImportRecords) -> Result<StepImport> {
    let _span = solidcraft_profile::span("import", "records.import");
    if records.bodies.len() > super::MAX_BODIES {
        return Err(KernelError::Invalid(format!("an import of {} bodies (at most {})", records.bodies.len(), super::MAX_BODIES)));
    }
    check_tree(&records.tree, records.bodies.len(), 0, &mut 0)?;
    let mut bodies = Vec::with_capacity(records.bodies.len());
    // A part placed again is one part: its copies are not counted again.
    let mut report = ImportReport { parts: records.bodies.iter().filter(|r| r.copy_of.is_none()).count(), ..ImportReport::default() };
    let mut warnings = records.warnings.clone();
    for w in &records.warnings {
        report.issues.push(ImportIssue { kind: IssueKind::File, item: None, part: String::new(), faces: 0, times: 1, detail: w.clone() });
    }
    // A body SMLib refuses is left out, as a STEP import leaves out a solid it cannot make, and
    // the others still come in: `kept[k]` is where record body `k` is among the bodies made.
    let mut kept = vec![None; records.bodies.len()];
    // Each original's part, made once and placed again for each of its copies.
    let mut originals: std::collections::HashMap<usize, Part> = std::collections::HashMap::new();
    // The parts counted among the import's solids.
    let mut counted = std::collections::HashSet::new();
    for (k, r) in records.bodies.iter().enumerate() {
        let label = if r.name.is_empty() { format!("body {}", k + 1) } else { r.name.clone() };
        // The record body whose part this is: itself, or the one it is a copy of.
        let source = r.copy_of.unwrap_or(k);
        let part = match r.copy_of {
            None => part_of(r),
            Some(j) => copy_part(records, &originals, k, j),
        };
        // An original's part is kept for its copies whatever its own placement: a copy placed
        // where it can be still comes in when the original's placement is refused.
        if r.copy_of.is_none()
            && let Ok(part) = &part
        {
            originals.insert(k, part.clone());
        }
        let made = part.and_then(|part| {
            finite_placement(r)?;
            placed(&part.body, &r.placement.unwrap_or(super::IDENTITY)).map(|b| (part, b)).map_err(|e| e.to_string())
        });
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
        let item = source as u64 + 1;
        let sources: Option<Vec<_>> = part
            .face_origin
            .iter()
            .map(|f| Some(Some(crate::FaceSource::Imported { item, face: f.checked_add(1)?, payload_sha256: None, producer: None })))
            .collect();
        let Some(sources) = sources else {
            let detail = format!("{label}: not imported: a face origin out of range");
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
        };
        let body = body.with_color(r.color).with_face_sources(Some(Arc::new(sources)));
        // A part is counted once, by the first of its bodies that comes in.
        if part.closed && counted.insert(source) {
            report.solids += 1;
        }
        kept[k] = Some(bodies.len());
        bodies.push(ImportedBody {
            source_item: source as u64,
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
        tree: renumbered(&records.tree, &kept, &bodies),
        bodies,
        warnings,
        report,
        schema: String::new(),
        system: records.system.clone(),
        preprocessor: String::new(),
        description: String::new(),
    })
}

/// `nodes` with each body index `k` the made body `kept[k]` is, with that body's file item, and
/// the bodies left out at their place among the node's solids, as a STEP import's tree keeps a
/// solid it could not read: a stored body name stays with its body. A node's stored left-out
/// solids (`left_out`, each at its place among the node's solids in file order) are kept where
/// they are, and the bodies take the places between them, so a body left out now is at the place
/// it had. An index past the records names no body and is dropped.
fn renumbered(nodes: &[StepNode], kept: &[Option<usize>], made: &[ImportedBody]) -> Vec<StepNode> {
    nodes
        .iter()
        .map(|n| {
            let (mut bodies, mut items) = (Vec::new(), Vec::new());
            let mut left_out = n.left_out.clone();
            let taken: std::collections::HashSet<usize> = n.left_out.iter().map(|l| l.at).collect();
            let mut at = 0usize;
            for k in &n.bodies {
                while taken.contains(&at) {
                    at = at.saturating_add(1);
                }
                match kept.get(*k) {
                    Some(Some(b)) => {
                        bodies.push(*b);
                        items.push(made.get(*b).map_or(*k as u64, |b| b.source_item));
                    }
                    Some(None) => left_out.push(LeftOut { at, item: *k as u64 }),
                    None => {}
                }
                at = at.saturating_add(1);
            }
            left_out.sort_by_key(|l| l.at);
            StepNode { name: n.name.clone(), transform: n.transform, bodies, items, left_out, children: renumbered(&n.children, kept, made) }
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
    let brep = r.brep.as_ref().ok_or("no record")?;
    if r.face_origin.len() != brep.faces.len() {
        return Err(format!("{} face origins for {} faces", r.face_origin.len(), brep.faces.len()));
    }
    solidcore_ops::record_check::validate(brep).map_err(|e| format!("the record is invalid: {e}"))?;
    solidcore_ops::record_check::agreement(brep).map_err(|e| format!("the record's geometry disagrees: {e}"))?;
    // A sheet is what its record is, whatever the file says: no bounded region but the
    // infinite one's.
    let sheet = r.sheet || brep.regions.len() <= 1;
    record_part(brep, &r.face_origin, sheet).map_err(|f| f.msg)
}

/// The part of body `k`, a copy of body `j`: `j`'s, which must be an earlier original that
/// imported.
fn copy_part(records: &ImportRecords, originals: &std::collections::HashMap<usize, Part>, k: usize, j: usize) -> std::result::Result<Part, String> {
    if j >= k {
        return Err(format!("a copy of body {}, which does not come before it", j.saturating_add(1)));
    }
    if records.bodies.get(j).is_none_or(|o| o.copy_of.is_some()) {
        return Err(format!("a copy of body {}, which is itself a copy", j + 1));
    }
    originals.get(&j).cloned().ok_or_else(|| format!("a copy of body {}, which was not imported", j + 1))
}

/// The body's placement, when it has one, is finite.
fn finite_placement(r: &RecordBody) -> std::result::Result<(), String> {
    match r.placement {
        Some(m) if m.iter().flatten().any(|x| !x.is_finite()) => Err("a placement that is not finite".into()),
        _ => Ok(()),
    }
}

/// Every node's bodies index one of `count` bodies, and the tree is no deeper and has no more
/// nodes ([`super::MAX_VISITS`], counted in `visits` over every root) than an assembly the STEP
/// reader reads.
fn check_tree(nodes: &[StepNode], count: usize, depth: usize, visits: &mut usize) -> Result<()> {
    if depth > super::MAX_ASSEMBLY_DEPTH {
        return Err(KernelError::Invalid("an import's product tree nested too deeply".into()));
    }
    for n in nodes {
        *visits += 1;
        if *visits > super::MAX_VISITS {
            return Err(KernelError::Invalid(format!("an import's product tree of more than {} products", super::MAX_VISITS)));
        }
        if n.bodies.iter().any(|b| *b >= count) {
            return Err(KernelError::Invalid(format!("product `{}` names a body the import does not have", n.name)));
        }
        if n.transform.iter().flatten().any(|x| !x.is_finite()) {
            return Err(KernelError::Invalid(format!("product `{}` has a placement that is not finite", n.name)));
        }
        check_tree(&n.children, count, depth + 1, visits)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "records_tests.rs"]
mod tests;
