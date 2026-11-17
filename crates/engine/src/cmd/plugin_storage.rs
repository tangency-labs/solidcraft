//! The data a plugin keeps in a design (`storage.*`, capability `storage@0.1`). Reads work for
//! anyone; writes only from the plugin's own run, on its own data, as one undoable edit.
use serde_json::Value;

use super::CommandSpec;
use crate::plugin::storage::{self, MAX_READ_BYTES};
use crate::schema::plugin_storage::{
    BlobDeleted, BlobRead, BlobWritten, StorageData, StorageDeleteBlobParams, StorageGetBlobParams, StorageGetParams, StorageList, StorageListParams,
    StoragePatchParams, StoragePutBlobParams, StorageSetParams, StorageWritten,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session, params::bad};
use solidcraft_doc::canvas::{base64_decode, base64_encode};
use solidcraft_doc::plugin_data::{BLOBS, CACHE};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("storage.get", "Read Plugin Data", crate::checked!("storage.get", get)).icon("inspect").noundo().read_only().doc_only()
        .contract(&TypedContract::<StorageGetParams>::new()).output(output_schema_for::<StorageData>)
        .params("plugin?: plugin id (required outside a plugin's run; a plugin reads only its own) → the plugin's data in the open design: name, plugin_version, format (null: none), plugin_format and read_only inside its run, document (the main JSON document or null), blobs: [{name, cache, bytes}]"),
    CommandSpec::new("storage.set", "Set Plugin Data", crate::checked!("storage.set", set)).icon("edit").doc_only()
        .contract(&TypedContract::<StorageSetParams>::new()).output(output_schema_for::<StorageWritten>)
        .params("document: any JSON, at most 16 MiB → replaces the running plugin's main document in the design, saved in its data_format; only from a plugin's own run; one undoable edit"),
    CommandSpec::new("storage.patch", "Patch Plugin Data", crate::checked!("storage.patch", patch)).icon("edit").doc_only()
        .contract(&TypedContract::<StoragePatchParams>::new()).output(output_schema_for::<StorageWritten>)
        .params("patch: an RFC 7396 merge patch → applied to the running plugin's main document; only from a plugin's own run; one undoable edit"),
    CommandSpec::new("storage.put_blob", "Write Plugin File", crate::checked!("storage.put_blob", put_blob)).icon("edit").doc_only()
        .contract(&TypedContract::<StoragePutBlobParams>::new()).output(output_schema_for::<BlobWritten>)
        .params("name: plain relative path, data: base64 bytes, cache?: bool (a file the plugin can make again), append?: bool (add to the end) → writes one of the running plugin's files in the design (256 MiB in all); only from a plugin's own run; one undoable edit"),
    CommandSpec::new("storage.get_blob", "Read Plugin File", crate::checked!("storage.get_blob", get_blob)).icon("inspect").noundo().read_only().doc_only()
        .contract(&TypedContract::<StorageGetBlobParams>::new()).output(output_schema_for::<BlobRead>)
        .params("plugin?: plugin id (required outside a plugin's run), name, offset?: bytes (default 0), len?: 1..524288 (default 524288) → data: base64 bytes from offset, bytes: the file's size"),
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
    let p: StorageGetParams = parse("storage.get", p)?;
    let id = storage::reader(s, "storage.get", p.plugin.as_deref())?;
    reply("storage.get", storage::read(&s.doc, &id, s.plugin_state.storage_caller.as_ref()))
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

fn put_blob(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "storage.put_blob";
    let p: StoragePutBlobParams = parse(CMD, p)?;
    let caller = storage::writer(s)?;
    let data = base64_decode(&p.data).ok_or_else(|| bad(CMD, "`data` must be base64"))?;
    let path = format!("{}{}", if p.cache { CACHE } else { BLOBS }, p.name);
    let mut bytes = if p.append { storage::blob(&s.doc, &caller.id, &path).unwrap_or_default() } else { Vec::new() };
    bytes.extend_from_slice(&data);
    storage::put(s, &caller, &path, Some(&bytes))?;
    // One name is one file: writing it as a cache file (or not) replaces the other kind.
    let other = format!("{}{}", if p.cache { BLOBS } else { CACHE }, p.name);
    if storage::blob(&s.doc, &caller.id, &other).is_some() {
        storage::put(s, &caller, &other, None)?;
    }
    reply(CMD, BlobWritten { name: p.name, cache: p.cache, bytes: bytes.len() as u64 })
}

fn get_blob(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "storage.get_blob";
    let p: StorageGetBlobParams = parse(CMD, p)?;
    let id = storage::reader(s, CMD, p.plugin.as_deref())?;
    let len = p.len.unwrap_or(MAX_READ_BYTES);
    if !(1..=MAX_READ_BYTES).contains(&len) {
        return Err(bad(CMD, "`len` must be 1 to 524288"));
    }
    let (path, cache) = storage::blob_path(&s.doc, &id, &p.name).ok_or_else(|| bad(CMD, format!("{id} keeps no file named {}", p.name)))?;
    let bytes = storage::blob(&s.doc, &id, &path).ok_or_else(|| bad(CMD, format!("{} is not readable", p.name)))?;
    let start = usize::try_from(p.offset).unwrap_or(usize::MAX).min(bytes.len());
    let end = start.saturating_add(len as usize).min(bytes.len());
    reply(
        CMD,
        BlobRead { name: p.name, cache, data: base64_encode(bytes.get(start..end).unwrap_or_default()), offset: p.offset, bytes: bytes.len() as u64 },
    )
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
