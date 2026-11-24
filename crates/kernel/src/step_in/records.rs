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
    /// The bodies, in the file's order. Reading records refuses more than an import makes
    /// (`bounded_bodies`).
    #[serde(deserialize_with = "bounded_bodies")]
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
    /// How the file shows it beyond its colour: its opacity, the colour's name, and the faces with
    /// colours of their own, each `face` being the file's face (a value of `face_origin`), not the
    /// body's face, as a STEP import's paint is before its body is made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paint: Option<crate::Paint>,
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
    /// The body as triangles, for a part the file gives as triangles alone (an AP242 tessellated
    /// part): a mesh body, as STL and 3MF bring in, in place of a record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh: Option<RecordMesh>,
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

/// A mesh body's triangles: vertex positions (millimetres, in the body's own coordinates) and
/// triangles of zero-based vertex indices, in the file's winding.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecordMesh {
    pub positions: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

/// [`ImportRecords::bodies`] as read, refused at the first body past [`super::MAX_BODIES`], which
/// [`records_import`] refuses in any case. A body takes a dozen bytes of text and over 500 in
/// memory, so records of millions of tiny bodies, small once compressed in a design, would
/// otherwise take tens of gigabytes before any check ran. Only this list is bounded as it is read.
/// The other lists of records cost what parsing their JSON costs.
fn bounded_bodies<'de, D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Vec<RecordBody>, D::Error> {
    struct Bodies;
    impl<'de> serde::de::Visitor<'de> for Bodies {
        type Value = Vec<RecordBody>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "a list of at most {} bodies", super::MAX_BODIES)
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Vec<RecordBody>, A::Error> {
            let mut bodies = Vec::new();
            while let Some(body) = seq.next_element::<RecordBody>()? {
                if bodies.len() == super::MAX_BODIES {
                    return Err(serde::de::Error::custom(format!("an import of more than {} bodies", super::MAX_BODIES)));
                }
                bodies.push(body);
            }
            // The list keeps no spare room, so it takes the memory of its bodies and no more (a
            // design reader bounds the bodies it keeps by their count).
            bodies.shrink_to_fit();
            Ok(bodies)
        }
    }
    deserializer.deserialize_seq(Bodies)
}

/// Most vertices of a mesh kept as a record (as an AP242 tessellated part read from STEP text).
const MAX_MESH_VERTICES: usize = 1_000_000;
/// Most triangles of a mesh kept as a record.
const MAX_MESH_TRIANGLES: usize = 2_000_000;

/// The pcurves an importer gives the shared assembly stage for edge uses with no closed form:
/// SMLib's own, as a STEP import's are. Readers outside the kernel (cadmpeg's neutral model, in
/// `solidcraft-io`) pass this, so only the kernel uses SMLib.
pub fn import_trims() -> &'static (dyn solidcore_ops::pcurve::Trims + Sync) {
    &solidcraft_smlib::pcurves::SmlibImportTrims
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
    // A part placed again is one part: its copies are not counted again. One root product: the
    // first name of every issue's path is the import's own component.
    let mut report = ImportReport {
        parts: records.bodies.iter().filter(|r| r.copy_of.is_none()).count(),
        single_root: records.tree.len() == 1,
        ..ImportReport::default()
    };
    let mut warnings = records.warnings.clone();
    for w in &records.warnings {
        report.issues.push(ImportIssue {
            kind: IssueKind::File,
            item: None,
            part: String::new(),
            paths: Vec::new(),
            faces: 0,
            times: 1,
            detail: w.clone(),
        });
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
            None => {
                // Faces and edges whose axes were well off square are squared to open; say so.
                let mut squared = Vec::new();
                let part = part_of(r, &mut squared);
                for what in squared {
                    let detail = format!("{label}: {what}");
                    report.issues.push(ImportIssue {
                        kind: IssueKind::Repaired,
                        item: Some(k as u64),
                        part: r.name.clone(),
                        paths: paths_of(r),
                        faces: 0,
                        times: 1,
                        detail: detail.clone(),
                    });
                    warnings.push(detail);
                }
                part
            }
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
                    paths: paths_of(r),
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
                paths: paths_of(r),
                faces: 0,
                times: 1,
                detail: detail.clone(),
            });
            warnings.push(detail);
            continue;
        };
        // The body's colour too is from a file: kept in 0..1, as its faces' are.
        let color = r.color.map(|c| c.map(unit));
        // A mesh body has no faces from the file to know.
        let body = if sources.is_empty() { body.with_color(color) } else { body.with_color(color).with_face_sources(Some(Arc::new(sources))) };
        let body = match &r.paint {
            Some(paint) => body.with_paint(Some(body_paint(paint, &part.face_origin))),
            None => body,
        };
        // A part is counted once, by the first of its bodies that comes in.
        if part.closed && counted.insert(source) {
            report.solids += 1;
        }
        kept[k] = Some(bodies.len());
        bodies.push(ImportedBody {
            source_item: source as u64,
            face_origin: part.face_origin,
            name: r.name.clone(),
            common_face_name: None,
            body,
            color,
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
    // Records are made into bodies as they are, whatever the heal preference
    // ([`crate::heals_tolerance_defects`]): never healed, so cached as a read that does not heal.
    if let Some(imp) = super::recent(key, false) {
        return Ok(imp);
    }
    let imp = Arc::new(records_import(records)?);
    super::remember(key, false, &imp, text.len() <= super::KEEP_SMALL_BYTES);
    Ok(imp)
}

/// `records` with `notes` about the file added to its warnings (what a check of the bodies its
/// import `imp` made found: `imp` is [`records_import`]'s of `records`), and that import with
/// them as file issues after the records' own, as [`records_import`] makes it of the new records.
/// It is kept in the recent imports under the new records, so the design's evaluation of them
/// does not make the bodies again.
pub fn records_noted(mut records: ImportRecords, imp: &StepImport, notes: Vec<String>) -> Result<(ImportRecords, Arc<StepImport>)> {
    if notes.is_empty() {
        let imp = records_import_shared(&records)?;
        return Ok((records, imp));
    }
    // The records' own warnings come first among the import's warnings and its issues.
    let at = records.warnings.len();
    let mut noted = imp.clone();
    let issues = notes.iter().map(|w| ImportIssue {
        kind: IssueKind::File,
        item: None,
        part: String::new(),
        paths: Vec::new(),
        faces: 0,
        times: 1,
        detail: w.clone(),
    });
    let issue_at = at.min(noted.report.issues.len());
    noted.report.issues.splice(issue_at..issue_at, issues);
    let warning_at = at.min(noted.warnings.len());
    noted.warnings.splice(warning_at..warning_at, notes.iter().cloned());
    records.warnings.extend(notes);
    let text = records_text(&records)?;
    let noted = Arc::new(noted);
    super::remember(super::text_key(&text), false, &noted, text.len() <= super::KEEP_SMALL_BYTES);
    Ok((records, noted))
}

/// The records' serialised form: what an import kept as records is, as STEP text is what a STEP
/// import is (its cache key and its bodies' certificates are of this).
pub fn records_text(records: &ImportRecords) -> Result<String> {
    serde_json::to_string(records).map_err(|e| KernelError::Invalid(format!("import records: {e}")))
}

/// Where a record body left out would have been, as [`ImportIssue::paths`] (none outside any
/// product).
fn paths_of(r: &RecordBody) -> Vec<Vec<String>> {
    if r.path.is_empty() { Vec::new() } else { vec![r.path.clone()] }
}

/// `paint` with each face colour on the body's faces that come from its file face (`face_origin`,
/// the file face of each body face), as a STEP import paints them; a colour on a face the body
/// does not have is dropped. Colours and opacities from a file are hostile: each is kept in 0..1.
fn body_paint(paint: &crate::Paint, face_origin: &[u64]) -> crate::Paint {
    let by_file: std::collections::HashMap<u64, &crate::FacePaint> = paint.faces.iter().map(|f| (f.face as u64, f)).collect();
    let faces = face_origin
        .iter()
        .enumerate()
        .filter_map(|(i, f)| {
            let p = by_file.get(f)?;
            Some(crate::FacePaint { face: i, color: p.color.map(unit), opacity: unit(p.opacity), name: p.name.clone() })
        })
        .collect();
    crate::Paint { opacity: unit(paint.opacity), faces, name: paint.name.clone() }
}

/// A colour channel or an opacity from a file, kept in 0..1 (1 when it is not a number).
fn unit(x: f32) -> f32 {
    if x.is_finite() { x.clamp(0.0, 1.0) } else { 1.0 }
}

/// The body SMLib makes of one record, checked first: a record from a file is hostile input.
/// Frames and planes a little off square (a record written before the writer squared them) are
/// squared first, so the body opens ([`solidcore_ops::record_check::square_axes`]); those further
/// off square than its tolerance are worded into `squared`, to warn of.
fn part_of(r: &RecordBody, squared: &mut Vec<String>) -> std::result::Result<Part, String> {
    if let (None, Some(mesh)) = (&r.brep, &r.mesh) {
        return mesh_part(mesh);
    }
    let brep = r.brep.as_ref().ok_or("no record")?;
    let square;
    let brep = if solidcore_ops::record_check::needs_squaring(brep) {
        let mut copy = brep.clone();
        for s in solidcore_ops::record_check::square_axes(&mut copy).iter().filter(|s| s.worth_a_warning()) {
            squared.push(match s.at {
                solidcore_ops::record_check::SquaredAt::Face(f) => {
                    format!("face {}'s axes were {:.1e} off square; squared to open", f + 1, s.deviation)
                }
                solidcore_ops::record_check::SquaredAt::Edge(e) => {
                    format!("edge {}'s axes were {:.1e} off square; squared to open", e + 1, s.deviation)
                }
            });
        }
        square = copy;
        &square
    } else {
        brep
    };
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

/// The mesh body of a body kept as triangles, checked first: no more vertices and triangles than
/// a STEP import reads of a tessellated part, every position finite and within 10⁹ mm, and every
/// index a vertex.
fn mesh_part(mesh: &RecordMesh) -> std::result::Result<Part, String> {
    let (nv, nt) = (mesh.positions.len(), mesh.triangles.len());
    if !(1..=MAX_MESH_VERTICES).contains(&nv) || !(1..=MAX_MESH_TRIANGLES).contains(&nt) {
        return Err(format!("a mesh of {nv} vertices and {nt} triangles (at most {MAX_MESH_VERTICES} and {MAX_MESH_TRIANGLES})"));
    }
    let mut positions = Vec::with_capacity(nv);
    for p in &mesh.positions {
        if p.iter().any(|x| !(x.abs() < 1e9)) {
            return Err("a mesh vertex that is not finite or is too far out".into());
        }
        positions.push(solidcore_record::Vec3::new(p[0], p[1], p[2]));
    }
    if mesh.triangles.iter().flatten().any(|&i| usize::try_from(i).map_or(true, |i| i >= nv)) {
        return Err("a mesh triangle that names a vertex the mesh does not have".into());
    }
    let body = crate::mesh_body(&positions, &mesh.triangles).map_err(|e| e.to_string())?;
    let closed = body.is_closed_mesh();
    Ok(Part { body, closed, file_faces: 0, face_origin: Vec::new(), refused: None, split_edges: 0 })
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
