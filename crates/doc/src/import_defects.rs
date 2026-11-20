//! What is wrong with the geometry an import brought in, by the body or component it concerns.
//!
//! A STEP import notes what it could not read or had to repair, part by part (the file's solid
//! item: [`kernel::ImportIssue::item`]). A body keeps the item it was read from on its faces
//! ([`kernel::FaceSource::Imported`], with the import feature as producer), so each note finds
//! the body made of that part, and every placement shows the same body. A part that was not
//! imported has no body: its note goes to the component that holds it, or the nearest one above
//! it that the import made (a component whose only part failed is not made), at every place the
//! file puts it.

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
            for at in holders(doc, feature.component, issue, report.single_root) {
                out.entry(at).or_default().push(issue);
            }
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

/// The components holding `issue`'s part, one per place the file puts it (several places can
/// share one): `top`, the import's own component, when the issue names none.
fn holders(doc: &Document, top: u64, issue: &ImportIssue, single_root: bool) -> BTreeSet<u64> {
    let mut out: BTreeSet<u64> = issue.paths.iter().map(|path| holder(doc, top, path, single_root)).collect();
    if out.is_empty() {
        out.insert(top);
    }
    out
}

/// The component the import made for the product at `path` (root first) under `top`, the
/// import's own component, or the nearest one above it that was made. With `single_root` the
/// file's one root product is `top` itself, so the path's first name is passed over; otherwise
/// each root product is a component below `top`.
fn holder(doc: &Document, top: u64, path: &[String], single_root: bool) -> u64 {
    let below = if single_root { path.get(1..).unwrap_or_default() } else { path };
    let mut at = top;
    for name in below {
        let name = name.trim();
        match doc.components.iter().find(|c| c.parent == at && c.id != at && c.name == name) {
            Some(child) => at = child.id,
            None => break,
        }
    }
    at
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn a_part_that_was_not_imported_marks_the_nearest_component_made_for_it() {
        let mut doc = Document::default();
        let top = doc.add_component(Some("Printer"), 0).unwrap();
        let frame = doc.add_component(Some("Frame"), top).unwrap();
        // One root product is the import's own component; the parts below it are its children.
        assert_eq!(holder(&doc, top, &path(&["Printer", "Frame"]), true), frame);
        // Several root products are each a component below it.
        assert_eq!(holder(&doc, top, &path(&["Frame"]), false), frame);
        // A product the import made no component for (its only part failed): the one above.
        assert_eq!(holder(&doc, top, &path(&["Printer", "Frame", "Bracket"]), true), frame);
        assert_eq!(holder(&doc, top, &path(&["Printer", "Fan"]), true), top);
        assert_eq!(holder(&doc, top, &[], true), top);
        assert_eq!(holder(&doc, top, &[], false), top);
    }

    /// A root product whose child has its name: the root is passed over by the report's flag,
    /// not by whether a child's name matches.
    #[test]
    fn a_root_named_as_its_child_is_passed_over() {
        let mut doc = Document::default();
        let top = doc.add_component(Some("Imported"), 0).unwrap();
        let housing = doc.add_component(Some("Housing"), top).unwrap();
        let bracket = doc.add_component(Some("Bracket"), housing).unwrap();
        assert_eq!(holder(&doc, top, &path(&["Housing", "Housing", "Bracket"]), true), bracket);
        assert_eq!(holder(&doc, top, &path(&["Housing", "Bracket"]), false), bracket);
    }

    /// A part placed under two assemblies, whose only product failed: both assemblies are marked.
    #[test]
    fn a_part_not_imported_marks_every_component_it_is_placed_in() {
        let mut doc = Document::default();
        let top = doc.add_component(Some("Imported"), 0).unwrap();
        let left = doc.add_component(Some("Left"), top).unwrap();
        let right = doc.add_component(Some("Right"), top).unwrap();
        let issue = ImportIssue {
            kind: IssueKind::NotImported,
            item: Some(7),
            part: "Pin".into(),
            paths: vec![path(&["Root", "Left", "Pin"]), path(&["Root", "Right", "Pin"])],
            faces: 0,
            times: 2,
            detail: "solid 'Pin': not imported".into(),
        };
        assert_eq!(holders(&doc, top, &issue, true), BTreeSet::from([left, right]));
        let file = ImportIssue { kind: IssueKind::File, item: None, paths: Vec::new(), ..issue };
        assert_eq!(holders(&doc, top, &file, true), BTreeSet::from([top]));
    }
}
