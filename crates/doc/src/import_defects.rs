//! What is wrong with the geometry an import brought in, by the body or component it concerns.
//!
//! A STEP import notes what it could not read or had to repair, part by part (the file's solid
//! item: [`kernel::ImportIssue::item`]). A body keeps the item it was read from on its faces
//! ([`kernel::FaceSource::Imported`], with the import feature as producer), so each note finds
//! the body made of that part, and every placement shows the same body. A part that was not
//! imported has no body: its note goes to the component that holds it, or the nearest one above
//! it that the import made (a component whose only part failed is not made).

use std::collections::{BTreeMap, BTreeSet};

use crate::{Document, Model, ModelBody, kernel};
use kernel::{FaceSource, ImportIssue, IssueKind};

/// The import notes about the parts a body was made from (none for a body that no import made).
pub fn body_import_issues<'a>(model: &'a Model, body: &ModelBody) -> Vec<&'a ImportIssue> {
    let mut out = Vec::new();
    for (producer, item) in imported_items(&body.body) {
        let Some(report) = model.result(producer).and_then(|r| r.import_report.as_deref()) else { continue };
        out.extend(report.issues.iter().filter(|i| i.item == Some(item) && body_kind(i.kind)));
    }
    out
}

/// The import notes about parts that were not imported, by the component that holds them (the
/// nearest one the import made).
pub fn component_import_issues<'a>(doc: &Document, model: &'a Model) -> BTreeMap<u64, Vec<&'a ImportIssue>> {
    let mut out: BTreeMap<u64, Vec<&ImportIssue>> = BTreeMap::new();
    for result in &model.results {
        let Some(report) = result.import_report.as_deref() else { continue };
        let Some(feature) = doc.feature(result.id) else { continue };
        for issue in report.issues.iter().filter(|i| lost_kind(i.kind)) {
            out.entry(holder(doc, feature.component, &issue.path)).or_default().push(issue);
        }
    }
    out
}

/// A note about a part that came in as a body (what was read is there, with this flaw).
pub fn body_kind(kind: IssueKind) -> bool {
    !lost_kind(kind) && kind != IssueKind::File
}

/// A note about a part that is not in the design.
pub fn lost_kind(kind: IssueKind) -> bool {
    matches!(kind, IssueKind::NotImported | IssueKind::Refused)
}

/// The (import feature, STEP item) pairs a body's faces were read from.
fn imported_items(body: &kernel::Body) -> BTreeSet<(u64, u64)> {
    body.face_sources()
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|s| match s {
            FaceSource::Imported { item, producer: Some(producer), .. } => Some((*producer, *item)),
            _ => None,
        })
        .collect()
}

/// The component the import made for the product at `path` (root first) under `top`, the
/// import's own component, or the nearest one above it that was made. The file's root product
/// is the import's own component, so a first name that names no child is passed over.
fn holder(doc: &Document, top: u64, path: &[String]) -> u64 {
    let mut at = top;
    for (k, name) in path.iter().enumerate() {
        let name = name.trim();
        match doc.components.iter().find(|c| c.parent == at && c.id != at && c.name == name) {
            Some(child) => at = child.id,
            None if k == 0 => {}
            None => break,
        }
    }
    at
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_part_that_was_not_imported_marks_the_nearest_component_made_for_it() {
        let mut doc = Document::default();
        let top = doc.add_component(Some("Printer"), 0).unwrap();
        let frame = doc.add_component(Some("Frame"), top).unwrap();
        let path = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        // The root product is the import's own component; the parts below it are its children.
        assert_eq!(holder(&doc, top, &path(&["Printer", "Frame"])), frame);
        assert_eq!(holder(&doc, top, &path(&["Frame"])), frame);
        // A product the import made no component for (its only part failed): the one above.
        assert_eq!(holder(&doc, top, &path(&["Printer", "Frame", "Bracket"])), frame);
        assert_eq!(holder(&doc, top, &path(&["Printer", "Fan"])), top);
        assert_eq!(holder(&doc, top, &[]), top);
    }
}
