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
use solidcraft_cloud::{CachedEntry, Connection, Destination, Error, PendingSave, Published, Result, Snapshot, StagedSave};
use wasm_bindgen::{JsCast as _, JsValue, closure::Closure};
use wasm_bindgen_futures::{JsFuture, future_to_promise};
use web_sys::{FileSystemDirectoryHandle, FileSystemFileHandle, FileSystemGetDirectoryOptions};

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
    pub async fn discard_management(&self, save: PendingSave) -> Result<()> {
        if save.manifest.change()?.is_none() {
            return Err(storage("ordinary saved edits cannot be discarded as management"));
        }
        let store = self.clone();
        with_lock(&self.lock, async move {
            let current = store.record(&save.id).await?.ok_or_else(|| storage("this action is no longer queued; refresh"))?;
            if serde_json::to_vec(&current).map_err(storage)? != serde_json::to_vec(&save).map_err(storage)? {
                return Err(Error::Conflict);
            }
            if store.receipt(&save.id).await?.is_some() {
                return Err(storage("this action is confirmed; synchronize before managing it again"));
            }
            let cached = store.cached(&save.destination).await?;
            if let Some(entry) = &cached {
                store.check_cached_document(entry).await?;
            }
            let key = cache_key(&save.destination)?;
            let transaction = store
                .database
                .transaction(["pending", "cached"])
                .with_mode(TransactionMode::Readwrite)
                .with_options(TransactionOptions::new().with_durability(TransactionDurability::Strict))
                .build()
                .map_err(storage)?;
            if cached.is_some_and(|e| e.save.id == save.id) {
                transaction.object_store("cached").map_err(storage)?.delete(key.as_str()).await.map_err(storage)?;
            }
            transaction.object_store("pending").map_err(storage)?.delete(save.id.as_str()).await.map_err(storage)?;
            transaction.commit().await.map_err(storage)
        })
        .await
    }
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
        crate::files::file_handle(&dir, name, create).await.map_err(storage)
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
            if store.receipt(&save.record.id).await?.is_some() {
                // An uncertain acknowledgement can be retried after a newer child was
                // confirmed. Its immutable receipt wins; never rewind the cached head.
                return Ok(save.record);
            }
            if let Some(current) = store.cached(&save.record.destination).await? {
                store.check_cached_document(&current).await?;
            }
            store.put_objects(save.objects).await?;
            let text = String::from_utf8(save.record_bytes).map_err(storage)?;
            let entry = CachedEntry::new(save.record.clone(), save.record.destination.clone(), Some(save.record.id.clone()), false)?;
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
        self.recover_checked(save, |_| Ok(())).await
    }

    pub async fn recover_checked(&self, save: &PendingSave, check_document: impl Fn(&[u8]) -> Result<()>) -> Result<Snapshot> {
        PendingSave::read(&serde_json::to_vec(save).map_err(storage)?)?;
        save.check_connection(&self.connection)?;
        let mut bytes = BTreeMap::new();
        let path = format!("documents/{}", save.manifest.document_hash);
        let document = self.read(&path).await?;
        if solidcraft_cloud::digest(&document) != save.manifest.document_hash {
            return Err(storage("captured document hash mismatch"));
        }
        check_document(&document)?;
        bytes.insert(path, document);
        for asset in save.manifest.assets.values() {
            for chunk in &asset.chunks {
                let path = chunk.path()?;
                if let std::collections::btree_map::Entry::Vacant(entry) = bytes.entry(path) {
                    let value = self.read(entry.key()).await?;
                    entry.insert(value);
                }
            }
        }
        if let Some(thumbnail) = solidcraft_cloud::Thumbnail::read(&save.manifest)? {
            for path in thumbnail.paths()? {
                if let Ok(value) = self.read(&path).await {
                    bytes.insert(path, value);
                }
            }
        }
        save.recover(|path| bytes.get(path).cloned().ok_or_else(|| storage("recovery object missing")))
    }

    pub async fn thumbnail(&self, save: &PendingSave) -> Result<Option<Vec<u8>>> {
        save.check_connection(&self.connection)?;
        let document = self.read(&format!("documents/{}", save.manifest.document_hash)).await?;
        solidcraft_engine::io::cloud::check_document(&document)?;
        if solidcraft_cloud::digest(&document) != save.manifest.document_hash {
            return Err(Error::Invalid("preview document hash mismatch".into()));
        }
        let mut objects = BTreeMap::new();
        if let Some(thumbnail) = solidcraft_cloud::Thumbnail::read(&save.manifest)? {
            for path in thumbnail.paths()? {
                if let Ok(bytes) = self.read(&path).await {
                    objects.insert(path, bytes);
                }
            }
        }
        solidcraft_engine::io::cloud::read_thumbnail(&save.manifest, |path| objects.get(path).cloned().ok_or_else(|| storage("preview unavailable")))
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

    async fn check_cached_document(&self, entry: &CachedEntry) -> Result<()> {
        let document = self.read(&format!("documents/{}", entry.save.manifest.document_hash)).await?;
        if solidcraft_cloud::digest(&document) != entry.save.manifest.document_hash {
            return Err(Error::Invalid("cached document hash mismatch".into()));
        }
        solidcraft_engine::io::cloud::check_document(&document)
    }

    pub async fn cached_entries(&self) -> Result<Vec<CachedEntry>> {
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
        let mut total_bytes = 0usize;
        for key in keys {
            let transaction = self.database.transaction("cached").build().map_err(storage)?;
            let text: Option<String> = transaction.object_store("cached").map_err(storage)?.get(key.as_str()).await.map_err(storage)?;
            transaction.commit().await.map_err(storage)?;
            if let Some(text) = text {
                total_bytes = total_bytes
                    .checked_add(text.len())
                    .filter(|n| *n <= MAX_QUEUE_METADATA)
                    .ok_or_else(|| storage("cached metadata exceeds the safe listing limit"))?;
                let entry = CachedEntry::read(&text)?;
                entry.save.check_connection(&self.connection)?;
                if cache_key(&entry.destination)? != key {
                    return Err(storage("cached file identifier mismatch"));
                }
                destinations.push(entry);
            }
        }
        Ok(destinations)
    }

    pub async fn mark_outdated(&self, observed: &solidcraft_cloud::StoredDesign) -> Result<()> {
        let store = self.clone();
        let observed = observed.clone();
        with_lock(&self.lock, async move {
            if let Some(mut current) = store.cached(&observed.destination).await?
                && current.mark_outdated(&observed)?
            {
                let text = current.write()?;
                let key = cache_key(&current.destination)?;
                let transaction = store
                    .database
                    .transaction("cached")
                    .with_mode(TransactionMode::Readwrite)
                    .with_options(TransactionOptions::new().with_durability(TransactionDurability::Strict))
                    .build()
                    .map_err(storage)?;
                transaction.object_store("cached").map_err(storage)?.put(text).with_key(key.as_str()).await.map_err(storage)?;
                transaction.commit().await.map_err(storage)?;
            }
            Ok(())
        })
        .await
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
            if let Some(current) = store.cached(&destination).await? {
                if store.record(&current.save.id).await?.is_some() {
                    // Another tab saved while the download was in flight. The pending capture
                    // remains the local head; a downloaded snapshot cannot replace it.
                    return Ok(());
                }
                store.check_cached_document(&current).await?;
            }
            store.put_objects(save.objects).await?;
            let entry = CachedEntry::new(save.record, destination, revision, true)?;
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
            if let Some(existing) = store.receipt(&save.id).await?
                && serde_json::to_value(existing).map_err(storage)? != serde_json::to_value(&published).map_err(storage)?
            {
                return Err(Error::Conflict);
            }
            let parent = match &save.expected_revision {
                Some(id) => store.receipt(id).await?,
                None => None,
            };
            published.validate(&save, parent.as_ref())?;
            let current = store.cached(&published.destination).await?;
            if let Some(current) = &current {
                store.check_cached_document(current).await?;
            }
            let update_cache = current.as_ref().is_none_or(|entry| entry.save.id == save.id)
                || parent.as_ref().is_some_and(|parent| current.as_ref().and_then(|entry| entry.revision.as_ref()) == Some(&parent.revision));
            let old_key = if published.destination != save.destination
                && store.cached(&save.destination).await?.is_some_and(|entry| entry.save.id == save.id)
            {
                Some(cache_key(&save.destination)?)
            } else {
                None
            };
            let mut entry = CachedEntry::new(save, published.destination.clone(), Some(published.revision.clone()), true)?;
            entry.conflict = published.conflict;
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

impl solidcraft_cloud::DownloadCache for Store {
    async fn load(&self, path: &str) -> Result<Option<Vec<u8>>> {
        if !path.starts_with("objects/") && !path.starts_with("checks/") {
            return Err(storage("invalid download object path"));
        }
        // Missing files are cache misses; other storage failures are reported by writes.
        match self.file(path, false).await {
            Ok(file) => crate::files::read_handle_bounded(&file, solidcraft_cloud::CHUNK_BYTES).await.map(Some).map_err(storage),
            Err(_) => Ok(None),
        }
    }
    async fn store(&self, path: &str, bytes: &[u8]) -> Result<()> {
        if !path.starts_with("objects/") && !path.starts_with("checks/") || bytes.len() > solidcraft_cloud::CHUNK_BYTES {
            return Err(storage("invalid download object"));
        }
        let store = self.clone();
        let (path, bytes) = (path.to_owned(), bytes.to_vec());
        with_lock(&self.lock, async move {
            if path.starts_with("checks/") {
                let file = store.file(&path, true).await?;
                return crate::files::write_handle(&file, &bytes).await.map_err(storage);
            }
            store.put_objects(BTreeMap::from([(path, bytes)])).await
        })
        .await
    }
}
