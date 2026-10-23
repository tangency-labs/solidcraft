//! Browser-local cloud captures. OPFS closes precede short, strictly durable IndexedDB
//! transactions. Web Locks coordinate this origin's tabs, independently of editing leases.
use std::collections::BTreeMap;
use std::future::Future;
use std::rc::Rc;

use futures::channel::oneshot;
use indexed_db_futures::{
    database::Database,
    prelude::*,
    transaction::{TransactionDurability, TransactionMode, TransactionOptions},
};
use js_sys::{Function, Promise, Reflect};
use solidcraft_cloud::{Connection, Destination, Error, PendingSave, Published, Result, Snapshot, StagedSave};
use wasm_bindgen::{JsCast as _, JsValue, closure::Closure};
use wasm_bindgen_futures::{JsFuture, future_to_promise};
use web_sys::{FileSystemDirectoryHandle, FileSystemFileHandle, FileSystemGetDirectoryOptions, FileSystemGetFileOptions};

const MAX_QUEUE_METADATA: usize = 128 << 20;

fn storage(error: impl std::fmt::Display) -> Error {
    Error::Storage(format!("browser storage: {error}; local captures are retained"))
}

fn js_error(error: JsValue) -> Error {
    storage(crate::files::err(error))
}

/// Hold a local origin lock until the operation's promise settles. No collaboration or
/// repository file lock is created. Unsupported browsers fail rather than risking two writers.
pub async fn with_lock<T: 'static>(name: &str, operation: impl Future<Output = Result<T>> + 'static) -> Result<T> {
    let window = web_sys::window().ok_or_else(|| storage("window unavailable"))?;
    let manager = Reflect::get(&window.navigator(), &"locks".into()).map_err(js_error)?;
    let request = Reflect::get(&manager, &"request".into())
        .map_err(js_error)?
        .dyn_into::<Function>()
        .map_err(|_| storage("this browser needs Web Locks for safe cloud storage"))?;
    let (sender, receiver) = oneshot::channel();
    let callback = Closure::once(move |_lock: JsValue| -> Promise {
        future_to_promise(async move {
            let _ = sender.send(operation.await);
            Ok(JsValue::UNDEFINED)
        })
    });
    let promise = request.call2(&manager, &name.into(), callback.as_ref()).map_err(js_error)?;
    let result = JsFuture::from(Promise::resolve(&promise)).await.map_err(js_error);
    drop(callback);
    result?;
    receiver.await.map_err(|_| storage("storage operation was interrupted"))?
}

#[derive(Clone)]
pub struct Store {
    database: Rc<Database>,
    directory: FileSystemDirectoryHandle,
    connection: Connection,
    lock: String,
    /// Denial is reported by the host; a successful write still checks close and commit.
    pub persistent: bool,
}

impl Drop for Store {
    fn drop(&mut self) {
        if Rc::strong_count(&self.database) == 1 {
            self.database.as_ref().clone().close();
        }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct CachedEntry {
    format: String,
    pub save: PendingSave,
    pub destination: Destination,
    pub revision: Option<String>,
}

impl CachedEntry {
    fn read(text: &str) -> Result<Self> {
        if text.len() > solidcraft_cloud::MAX_MANIFEST_BYTES {
            return Err(storage("cached entry too large"));
        }
        let value: serde_json::Value = serde_json::from_str(text).map_err(storage)?;
        if value.get("format").and_then(serde_json::Value::as_str) != Some("solidcraft-cache/1") {
            return Err(Error::UpgradeRequired("browser cached entry format".into()));
        }
        let save = value.get("save").ok_or_else(|| storage("cached capture missing"))?;
        PendingSave::read(&serde_json::to_vec(save).map_err(storage)?)?;
        let entry: Self = serde_json::from_value(value).map_err(storage)?;
        entry.destination.validate()?;
        if entry.destination.connection != entry.save.destination.connection
            || entry.destination.project != entry.save.destination.project
            || entry.revision.as_ref().is_some_and(|revision| revision.is_empty() || revision.len() > 128 || revision.chars().any(char::is_control))
        {
            return Err(storage("cached destination or revision mismatch"));
        }
        Ok(entry)
    }

    fn write(&self) -> Result<String> {
        let text = serde_json::to_string(self).map_err(storage)?;
        Self::read(&text)?;
        Ok(text)
    }
}

fn cache_key(destination: &Destination) -> Result<String> {
    destination.validate()?;
    Ok(solidcraft_cloud::digest(&serde_json::to_vec(destination).map_err(storage)?))
}

async fn directory(parent: &FileSystemDirectoryHandle, name: &str, create: bool) -> Result<FileSystemDirectoryHandle> {
    let options = FileSystemGetDirectoryOptions::new();
    options.set_create(create);
    JsFuture::from(parent.get_directory_handle_with_options(name, &options))
        .await
        .map_err(js_error)?
        .dyn_into()
        .map_err(|_| storage("not a directory"))
}

impl Store {
    pub async fn open(connection: Connection) -> Result<Self> {
        connection.validate()?;
        let identity = solidcraft_cloud::digest(connection.id.as_bytes());
        let name = format!("solidcraft-cloud-{identity}");
        // No requested version: first creation is version 1; an existing future version can
        // be identified and refused without attempting a downgrade or changing its stores.
        let database = Database::open(name.as_str())
            .with_on_upgrade_needed(|_, db| {
                db.create_object_store("pending").build()?;
                db.create_object_store("receipts").build()?;
                db.create_object_store("cached").build()?;
                Ok(())
            })
            .await
            .map_err(storage)?;
        if database.version() != 1.0 {
            database.close();
            return Err(Error::UpgradeRequired("browser cloud database version".into()));
        }
        let root = crate::files::root().await.map_err(storage)?;
        let cloud = directory(&root, "cloud", true).await?;
        let directory = directory(&cloud, &identity, true).await?;
        let manager = web_sys::window().ok_or_else(|| storage("window unavailable"))?.navigator().storage();
        let persistent = JsFuture::from(manager.persist().map_err(js_error)?).await.map_err(js_error)?.as_bool().unwrap_or(false);
        Ok(Self { database: Rc::new(database), directory, connection, lock: format!("solidcraft-cloud-io-{identity}"), persistent })
    }

    async fn file(&self, path: &str, create: bool) -> Result<FileSystemFileHandle> {
        solidcraft_cloud::check_path(path)?;
        let (parent, name) = path.rsplit_once('/').ok_or_else(|| storage("missing object directory"))?;
        let mut dir = self.directory.clone();
        for component in parent.split('/') {
            dir = directory(&dir, component, create).await?;
        }
        let options = FileSystemGetFileOptions::new();
        options.set_create(create);
        JsFuture::from(dir.get_file_handle_with_options(name, &options)).await.map_err(js_error)?.dyn_into().map_err(|_| storage("not a file"))
    }

    async fn read(&self, path: &str) -> Result<Vec<u8>> {
        let max = if path.starts_with("objects/") { solidcraft_cloud::CHUNK_BYTES } else { solidcraft_cloud::MAX_MANIFEST_BYTES };
        crate::files::read_handle_bounded(&self.file(path, false).await?, max).await.map_err(storage)
    }

    async fn put_objects(&self, objects: BTreeMap<String, Vec<u8>>) -> Result<()> {
        for (path, bytes) in objects {
            let file = self.file(&path, true).await?;
            let max = if path.starts_with("objects/") { solidcraft_cloud::CHUNK_BYTES } else { solidcraft_cloud::MAX_MANIFEST_BYTES };
            let existing = crate::files::read_handle_bounded(&file, max).await.map_err(storage)?;
            if existing == bytes {
                continue;
            }
            // getFileHandle(create=true) can leave an empty file if the browser stops before
            // the first stream close. Repair that incomplete creation from verified bytes.
            if !existing.is_empty() {
                return Err(storage("an existing recovery object differs; it was not overwritten"));
            }
            crate::files::write_handle(&file, &bytes).await.map_err(storage)?;
        }
        Ok(())
    }

    pub async fn stage(&self, save: StagedSave) -> Result<PendingSave> {
        save.validate()?;
        save.record.check_connection(&self.connection)?;
        let store = self.clone();
        with_lock(&self.lock, async move {
            // Preflight a resumed record before writing anything.
            if let Some(existing) = store.record(&save.record.id).await?
                && serde_json::to_vec(&existing).map_err(storage)? != serde_json::to_vec(&save.record).map_err(storage)?
            {
                return Err(Error::Conflict);
            }
            store.receipt(&save.record.id).await?;
            store.cached(&save.record.destination).await?;
            store.put_objects(save.objects).await?;
            let text = String::from_utf8(save.record_bytes).map_err(storage)?;
            let entry = CachedEntry {
                format: "solidcraft-cache/1".into(),
                save: save.record.clone(),
                destination: save.record.destination.clone(),
                revision: Some(save.record.id.clone()),
            };
            let cached = entry.write()?;
            let key = cache_key(&entry.destination)?;
            // No OPFS or network awaits occur after the transaction begins.
            let transaction = store
                .database
                .transaction(["pending", "cached"])
                .with_mode(TransactionMode::Readwrite)
                .with_options(TransactionOptions::new().with_durability(TransactionDurability::Strict))
                .build()
                .map_err(storage)?;
            transaction.object_store("pending").map_err(storage)?.put(text).with_key(save.record.id.as_str()).await.map_err(storage)?;
            transaction.object_store("cached").map_err(storage)?.put(cached).with_key(key.as_str()).await.map_err(storage)?;
            transaction.commit().await.map_err(storage)?;
            Ok(save.record)
        })
        .await
    }

    async fn record(&self, id: &str) -> Result<Option<PendingSave>> {
        let transaction = self.database.transaction("pending").build().map_err(storage)?;
        let value: Option<String> = transaction.object_store("pending").map_err(storage)?.get(id).await.map_err(storage)?;
        transaction.commit().await.map_err(storage)?;
        let record = value.map(|text| PendingSave::read(text.as_bytes())).transpose()?;
        if let Some(record) = &record {
            if record.id != id {
                return Err(storage("queued save identifier mismatch"));
            }
            record.check_connection(&self.connection)?;
        }
        Ok(record)
    }

    pub async fn pending(&self) -> Result<Vec<PendingSave>> {
        let transaction = self.database.transaction("pending").build().map_err(storage)?;
        let keys = transaction
            .object_store("pending")
            .map_err(storage)?
            .get_all_keys::<String>()
            .with_limit(10_001)
            .await
            .map_err(storage)?
            .collect::<indexed_db_futures::Result<Vec<_>>>()
            .map_err(storage)?;
        transaction.commit().await.map_err(storage)?;
        if keys.len() > 10_000 {
            return Err(storage("too many queued saves"));
        }
        let mut records = Vec::new();
        let mut size = 0usize;
        for key in keys {
            if let Some(record) = self.record(&key).await? {
                size = size
                    .checked_add(serde_json::to_vec(&record).map_err(storage)?.len())
                    .filter(|size| *size <= MAX_QUEUE_METADATA)
                    .ok_or_else(|| storage("queue metadata exceeds the safe listing limit"))?;
                records.push(record);
            }
        }
        PendingSave::ordered(records)
    }

    pub async fn recover(&self, save: &PendingSave) -> Result<Snapshot> {
        PendingSave::read(&serde_json::to_vec(save).map_err(storage)?)?;
        save.check_connection(&self.connection)?;
        let mut bytes = BTreeMap::new();
        let path = format!("documents/{}", save.manifest.document_hash);
        bytes.insert(path.clone(), self.read(&path).await?);
        for asset in save.manifest.assets.values() {
            for chunk in &asset.chunks {
                let path = chunk.path()?;
                if let std::collections::btree_map::Entry::Vacant(entry) = bytes.entry(path) {
                    let value = self.read(entry.key()).await?;
                    entry.insert(value);
                }
            }
        }
        save.recover(|path| bytes.get(path).cloned().ok_or_else(|| storage("recovery object missing")))
    }

    pub async fn cached(&self, destination: &Destination) -> Result<Option<CachedEntry>> {
        let key = cache_key(destination)?;
        let transaction = self.database.transaction("cached").build().map_err(storage)?;
        let text: Option<String> = transaction.object_store("cached").map_err(storage)?.get(key.as_str()).await.map_err(storage)?;
        transaction.commit().await.map_err(storage)?;
        let entry = text.as_deref().map(CachedEntry::read).transpose()?;
        if let Some(entry) = &entry {
            entry.save.check_connection(&self.connection)?;
            if &entry.destination != destination {
                return Err(storage("cached file identifier mismatch"));
            }
        }
        Ok(entry)
    }

    pub async fn cached_destinations(&self) -> Result<Vec<Destination>> {
        let transaction = self.database.transaction("cached").build().map_err(storage)?;
        let keys = transaction
            .object_store("cached")
            .map_err(storage)?
            .get_all_keys::<String>()
            .with_limit(10_001)
            .await
            .map_err(storage)?
            .collect::<indexed_db_futures::Result<Vec<_>>>()
            .map_err(storage)?;
        transaction.commit().await.map_err(storage)?;
        if keys.len() > 10_000 {
            return Err(storage("too many cached files"));
        }
        let mut destinations = Vec::new();
        for key in keys {
            let transaction = self.database.transaction("cached").build().map_err(storage)?;
            let text: Option<String> = transaction.object_store("cached").map_err(storage)?.get(key.as_str()).await.map_err(storage)?;
            transaction.commit().await.map_err(storage)?;
            if let Some(text) = text {
                let entry = CachedEntry::read(&text)?;
                entry.save.check_connection(&self.connection)?;
                if cache_key(&entry.destination)? != key {
                    return Err(storage("cached file identifier mismatch"));
                }
                destinations.push(entry.destination);
            }
        }
        Ok(destinations)
    }

    pub async fn cache_remote(&self, destination: Destination, snapshot: Snapshot) -> Result<()> {
        destination.validate()?;
        if destination.connection != self.connection.id {
            return Err(storage("repository connection mismatch"));
        }
        let revision = solidcraft_cloud::revision(&snapshot.manifest).map(str::to_owned);
        let save = PendingSave::stage(&self.connection, &destination.project, &destination.file, revision.clone(), snapshot)?;
        save.validate()?;
        let store = self.clone();
        with_lock(&self.lock, async move {
            if let Some(current) = store.cached(&destination).await?
                && store.record(&current.save.id).await?.is_some()
            {
                // Another tab saved while the download was in flight. The pending capture
                // remains the local head; a downloaded snapshot cannot replace it.
                return Ok(());
            }
            store.put_objects(save.objects).await?;
            let entry = CachedEntry { format: "solidcraft-cache/1".into(), save: save.record, destination, revision };
            let text = entry.write()?;
            let key = cache_key(&entry.destination)?;
            let transaction = store
                .database
                .transaction("cached")
                .with_mode(TransactionMode::Readwrite)
                .with_options(TransactionOptions::new().with_durability(TransactionDurability::Strict))
                .build()
                .map_err(storage)?;
            transaction.object_store("cached").map_err(storage)?.put(text).with_key(key.as_str()).await.map_err(storage)?;
            transaction.commit().await.map_err(storage)
        })
        .await
    }

    /// Receipt and cache updates precede queue removal in the same durable transaction.
    pub async fn confirm(&self, save: PendingSave, published: Published) -> Result<()> {
        PendingSave::read(&serde_json::to_vec(&save).map_err(storage)?)?;
        save.check_connection(&self.connection)?;
        published.destination.validate()?;
        if published.revision != save.id
            || published.destination.connection != save.destination.connection
            || published.destination.project != save.destination.project
            || !matches!(published.commit.len(), 40 | 64)
            || !published.commit.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(storage("publication acknowledgement mismatch"));
        }
        let store = self.clone();
        with_lock(&self.lock, async move {
            store.record(&save.id).await?;
            store.receipt(&save.id).await?;
            let current = store.cached(&published.destination).await?;
            let update_cache = current.as_ref().is_none_or(|entry| entry.save.id == save.id);
            let old_key = if published.destination != save.destination
                && store.cached(&save.destination).await?.is_some_and(|entry| entry.save.id == save.id)
            {
                Some(cache_key(&save.destination)?)
            } else {
                None
            };
            let entry = CachedEntry {
                format: "solidcraft-cache/1".into(),
                save,
                destination: published.destination.clone(),
                revision: Some(published.revision.clone()),
            };
            let cached = entry.write()?;
            let key = cache_key(&entry.destination)?;
            let receipt = serde_json::to_string(&serde_json::json!({"format":"solidcraft-receipt/1","published":published})).map_err(storage)?;
            let transaction = store
                .database
                .transaction(["pending", "cached", "receipts"])
                .with_mode(TransactionMode::Readwrite)
                .with_options(TransactionOptions::new().with_durability(TransactionDurability::Strict))
                .build()
                .map_err(storage)?;
            transaction.object_store("receipts").map_err(storage)?.put(receipt).with_key(entry.save.id.as_str()).await.map_err(storage)?;
            if update_cache {
                transaction.object_store("cached").map_err(storage)?.put(cached).with_key(key.as_str()).await.map_err(storage)?;
            }
            if let Some(old_key) = old_key {
                transaction.object_store("cached").map_err(storage)?.delete(old_key.as_str()).await.map_err(storage)?;
            }
            transaction.object_store("pending").map_err(storage)?.delete(entry.save.id.as_str()).await.map_err(storage)?;
            transaction.commit().await.map_err(storage)
        })
        .await
    }

    pub async fn receipt(&self, id: &str) -> Result<Option<Published>> {
        let transaction = self.database.transaction("receipts").build().map_err(storage)?;
        let text: Option<String> = transaction.object_store("receipts").map_err(storage)?.get(id).await.map_err(storage)?;
        transaction.commit().await.map_err(storage)?;
        let Some(text) = text else { return Ok(None) };
        if text.len() > solidcraft_cloud::MAX_MANIFEST_BYTES {
            return Err(storage("publication receipt too large"));
        }
        let value: serde_json::Value = serde_json::from_str(&text).map_err(storage)?;
        if value.get("format").and_then(serde_json::Value::as_str) != Some("solidcraft-receipt/1") {
            return Err(Error::UpgradeRequired("browser publication receipt format".into()));
        }
        let published: Published =
            serde_json::from_value(value.get("published").cloned().ok_or_else(|| storage("publication receipt missing"))?).map_err(storage)?;
        published.destination.validate()?;
        if published.revision != id || published.destination.connection != self.connection.id {
            return Err(storage("publication receipt identifier mismatch"));
        }
        Ok(Some(published))
    }
}
