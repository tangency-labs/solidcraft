//! The open designs: list them and show one. The engine holds one design and no tabs, so what is
//! here is what a command line or headless server answers: its one design. The desktop window
//! answers `document.list` and `document.activate` itself, for every design tab it has open
//! (`ui-egui`'s `documents`), and never reaches these handlers.
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::CommandSpec;
use crate::params::bad;
use crate::schema::documents::{DocumentActivateParams, DocumentActivateResult, DocumentListParams, DocumentListResult, OpenDocument};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const LIST_CONTRACT: TypedContract<DocumentListParams> = TypedContract::new();
const ACTIVATE_CONTRACT: TypedContract<DocumentActivateParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("document.list", "List Designs", crate::checked!("document.list", list))
        .any_part()
        .icon("list")
        .noundo()
        .doc_only()
        .read_only()
        .contract(&LIST_CONTRACT)
        .output(output_schema_for::<DocumentListResult>)
        .params("no parameters → documents: [{document_id, name, path, active, dirty}] of the open designs in tab order; active_document_id: the design every other command acts on. The command line and headless servers have one design; read-only"),
    CommandSpec::new("document.activate", "Show Design", crate::checked!("document.activate", activate))
        .any_part()
        .icon("list")
        .noundo()
        .doc_only()
        .contract(&ACTIVATE_CONTRACT)
        .output(output_schema_for::<DocumentActivateResult>)
        .params("document_id: an open design's id, from document.list, document.revision or file.new with new_tab → document_id, name, path; shows that design's tab, and every command after this one acts on it. Never edits a design or opens a dialog; an id that no open design has is an error"),
];

/// `p` as the contract type `P`; no parameters at all (`null`) is the empty object.
fn parse<P: DeserializeOwned>(cmd: &str, p: &Value) -> Result<P> {
    let p = if p.is_null() { Value::Object(Default::default()) } else { p.clone() };
    serde_json::from_value(p).map_err(|e| bad(cmd, e.to_string()))
}

fn reply(cmd: &str, reply: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(reply).map_err(|e| bad(cmd, e.to_string()))
}

/// The most bytes of a `document_id` that can name a design: the ids this program makes are far
/// shorter, so a longer one names none.
const MAX_ID: usize = 128;

fn list(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "document.list";
    let DocumentListParams {} = parse(cmd, p)?;
    let document =
        OpenDocument { document_id: s.document_id.clone(), name: s.doc.name.clone(), path: s.path.clone(), active: true, dirty: s.is_dirty() };
    reply(cmd, DocumentListResult { active_document_id: document.document_id.clone(), documents: vec![document], param_warnings: Vec::new() })
}

fn activate(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "document.activate";
    let request: DocumentActivateParams = parse(cmd, p)?;
    if request.document_id.len() > MAX_ID || request.document_id != s.document_id {
        return Err(bad(cmd, "no open design has that document_id (document.list names the open designs)"));
    }
    reply(
        cmd,
        DocumentActivateResult { document_id: s.document_id.clone(), name: s.doc.name.clone(), path: s.path.clone(), param_warnings: Vec::new() },
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;
    use crate::schema::documents::{DocumentActivateResult, DocumentListResult};
    use crate::schema::output_schema_for;
    use crate::schema::tests::conforms;

    /// A host with one design lists it, as the active one, under the id `document.revision` gives.
    #[test]
    fn list_names_the_one_design() {
        let mut s = Session::default();
        s.execute("file.new", &json!({"name": "Sheet 1"})).unwrap();
        let revision = s.execute("document.revision", &json!({})).unwrap();
        let listed = s.execute("document.list", &json!({})).unwrap();
        assert_eq!(listed["active_document_id"], revision["document_id"], "{listed}");
        assert_eq!(
            listed["documents"],
            json!([{"document_id": revision["document_id"], "name": "Sheet 1", "path": null, "active": true, "dirty": false}])
        );
        s.execute("solid.box", &json!({"length": 2, "width": 2, "height": 2})).unwrap();
        assert_eq!(s.execute("document.list", &json!(null)).unwrap()["documents"][0]["dirty"], true);
        let schema = output_schema_for::<DocumentListResult>();
        assert!(conforms(schema.as_value(), schema.as_value(), &listed), "{listed}");
    }

    /// Showing the design that is shown is allowed; an id that names no open design, a missing one
    /// and a stray key are refused, and none of them changes anything.
    #[test]
    fn activate_takes_only_the_id_of_an_open_design() {
        let mut s = Session::default();
        let id = s.document_id.clone();
        let reply = s.execute("document.activate", &json!({"document_id": id})).unwrap();
        assert_eq!(reply["document_id"], json!(id), "{reply}");
        let schema = output_schema_for::<DocumentActivateResult>();
        assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
        for bad in [
            json!({"document_id": "no-such-design"}),
            json!({"document_id": "x".repeat(4096)}),
            json!({}),
            json!({"document_id": 7}),
            json!({"document_id": id, "index": 0}),
        ] {
            assert!(s.execute("document.activate", &bad).is_err(), "{bad}");
        }
        assert_eq!(s.document_id, id);
        assert!(s.execute("document.list", &json!({"all": true})).is_err(), "a stray key is refused");
    }
}
