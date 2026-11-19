//! A body or component whose geometry came in from an import with flaws: the warning mark at
//! the end of its Browser row and what its tooltip says (`solidcraft_doc::import_defects`).
use std::collections::{BTreeMap, BTreeSet};

use solidcraft_engine::doc::import_defects;
use solidcraft_engine::doc::kernel::ImportIssue;
use solidcraft_engine::doc::{Document, Model, ModelBody};

use crate::ui_kit::StatusTip;

/// What a body's row says about the flaws its import noted in the part it was made from.
/// `None` when the import noted none (or no import made it).
pub(crate) fn body_tip(model: &Model, body: &ModelBody) -> Option<StatusTip> {
    let issues = import_defects::body_import_issues(model, body);
    (!issues.is_empty()).then(|| tip(&body.name, solidcraft_ui_text::tr("browser.defect_body").to_string(), &issues))
}

/// What each component's row says about the parts of it its import could not bring in, by
/// component id.
pub(crate) fn component_tips(doc: &Document, model: &Model) -> BTreeMap<u64, StatusTip> {
    import_defects::component_import_issues(doc, model)
        .into_iter()
        .filter_map(|(id, issues)| {
            let name = &doc.components.iter().find(|c| c.id == id)?.name;
            let parts = issues.iter().map(|i| i.item).collect::<BTreeSet<_>>().len();
            let key = if parts == 1 { "browser.defect_lost_one" } else { "browser.defect_lost_other" };
            let warning = solidcraft_ui_text::format(key, &[("count", &parts.to_string())]);
            Some((id, tip(name, warning, &issues)))
        })
        .collect()
}

/// The row's name, the sentence, then each note as the import words it.
fn tip(head: &str, warning: String, issues: &[&ImportIssue]) -> StatusTip {
    let mut tail = solidcraft_ui_text::tr("import_report.details").to_string();
    for issue in issues {
        tail.push('\n');
        tail.push_str(&issue.detail);
    }
    StatusTip { head: head.to_string(), error: None, warning: Some(warning), tail }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_engine::doc::kernel::IssueKind;

    #[test]
    fn a_tip_names_the_row_then_says_what_the_import_found() {
        let issue = ImportIssue {
            kind: IssueKind::OpenBody,
            item: Some(7),
            part: "Bracket".into(),
            paths: Vec::new(),
            faces: 0,
            times: 1,
            detail: "solid 'Bracket': imported as an open body (the shell is not closed)".into(),
        };
        let tip = tip("Bracket", "flawed".into(), &[&issue]);
        assert_eq!(tip.head, "Bracket");
        assert_eq!(tip.warning.as_deref(), Some("flawed"));
        assert!(tip.tail.ends_with("\nsolid 'Bracket': imported as an open body (the shell is not closed)"), "{}", tip.tail);
    }
}
