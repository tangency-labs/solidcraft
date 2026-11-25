//! The data a plugin keeps in a design (`storage.*`, capability `storage@0.1`). Reads work for
//! anyone; writes only from the plugin's own run, on its own data, as one undoable edit.
use serde_json::Value;

use super::CommandSpec;
use crate::plugin::storage;
use crate::schema::plugin_storage::{
    BlobDeleted, StorageData, StorageDeleteBlobParams, StorageGetParams, StorageList, StorageListParams, StoragePatchParams, StorageSetParams,
    StorageWritten,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session, params::bad};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("storage.get", "Read Plugin Data", crate::checked!("storage.get", get)).icon("inspect").noundo().read_only().doc_only()
        .contract(&TypedContract::<StorageGetParams>::new()).output(output_schema_for::<StorageData>)
        .params("plugin?: plugin id (required outside a plugin's run; a plugin reads only its own) → name, plugin_version, format (null: none), plugin_format, read_only, document (main JSON or null), blobs: [{name, cache, bytes}]. A plugin reads a document larger than its 1 MiB result, and its files' bytes, through the storage interface (get-document, get-blob), as bytes"),
    CommandSpec::new("storage.set", "Set Plugin Data", crate::checked!("storage.set", set)).icon("edit").doc_only()
        .contract(&TypedContract::<StorageSetParams>::new()).output(output_schema_for::<StorageWritten>)
        .params("document: any JSON, at most 16 MiB → replaces the running plugin's main document in the design, saved in its data_format; only from a plugin's own run; one undoable edit"),
    CommandSpec::new("storage.patch", "Patch Plugin Data", crate::checked!("storage.patch", patch)).icon("edit").doc_only()
        .contract(&TypedContract::<StoragePatchParams>::new()).output(output_schema_for::<StorageWritten>)
        .params("patch: an RFC 7396 merge patch → applied to the running plugin's main document; only from a plugin's own run; one undoable edit"),
    CommandSpec::new("storage.delete_blob", "Delete Plugin File", crate::checked!("storage.delete_blob", delete_blob)).icon("delete").doc_only()
        .contract(&TypedContract::<StorageDeleteBlobParams>::new()).output(output_schema_for::<BlobDeleted>)
        .params("name → deletes one of the running plugin's files in the design; only from a plugin's own run; one undoable edit"),
    CommandSpec::new("storage.list", "List Plugin Data", crate::checked!("storage.list", list)).icon("list").noundo().read_only().doc_only()
        .contract(&TypedContract::<StorageListParams>::new()).output(output_schema_for::<StorageList>)
        .params("→ plugins: [{plugin, name, plugin_version, format, installed_version?, status: ready|migrate|missing|disabled|older, read_only, bytes}], the plugins that keep data in the open design; reads plugin manifests, runs no plugin code"),
];

fn parse<T: serde::de::DeserializeOwned>(cmd: &str, p: &Value) -> Result<T> {
    serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))
}
fn reply<T: serde::Serialize>(cmd: &str, value: T) -> Result<Value> {
    serde_json::to_value(value).map_err(|e| bad(cmd, e.to_string()))
}

fn get(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "storage.get";
    let p: StorageGetParams = parse(CMD, p)?;
    let id = storage::reader(s, CMD, p.plugin.as_deref())?;
    reply(CMD, storage::read(&s.doc, &id, s.plugin_state.storage_caller.as_ref()))
}

fn set(s: &mut Session, p: &Value) -> Result<Value> {
    let p: StorageSetParams = parse("storage.set", p)?;
    let caller = storage::writer(s)?;
    let bytes = storage::put_document(s, &caller, &p.document)?;
    reply("storage.set", StorageWritten { plugin: caller.id, format: caller.data_format, bytes })
}

fn patch(s: &mut Session, p: &Value) -> Result<Value> {
    let p: StoragePatchParams = parse("storage.patch", p)?;
    let caller = storage::writer(s)?;
    let mut document = storage::read(&s.doc, &caller.id, Some(&caller)).document;
    storage::merge_patch(&mut document, &p.patch);
    let bytes = storage::put_document(s, &caller, &document)?;
    reply("storage.patch", StorageWritten { plugin: caller.id, format: caller.data_format, bytes })
}

fn delete_blob(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "storage.delete_blob";
    let p: StorageDeleteBlobParams = parse(CMD, p)?;
    let caller = storage::writer(s)?;
    let Some((path, _)) = storage::blob_path(&s.doc, &caller.id, &p.name) else {
        return reply(CMD, BlobDeleted { name: p.name, deleted: false });
    };
    storage::put(s, &caller, &path, None)?;
    reply(CMD, BlobDeleted { name: p.name, deleted: true })
}

fn list(s: &mut Session, p: &Value) -> Result<Value> {
    let _: StorageListParams = parse("storage.list", p)?;
    reply("storage.list", StorageList { plugins: storage::list(s) })
}
