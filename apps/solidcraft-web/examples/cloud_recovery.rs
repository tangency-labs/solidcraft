//! Real-browser persistence fixture. Build for wasm32, run `wasm-bindgen --target web`, and
//! call `cloud_recovery_phase` from a same-origin test page before and after browser restart.
//! Uses the actual IndexedDB/OPFS adapter and picture renderer, without launching the UI.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

#[cfg(target_arch = "wasm32")]
#[path = "../src/cloud_client.rs"]
pub mod cloud_client;
#[cfg(target_arch = "wasm32")]
#[path = "../src/cloud_host.rs"]
pub mod cloud_host;
#[cfg(target_arch = "wasm32")]
#[path = "../src/cloud_settings.rs"]
pub mod cloud_settings;
#[cfg(target_arch = "wasm32")]
#[path = "../src/cloud_storage.rs"]
pub mod cloud_storage;
#[cfg(target_arch = "wasm32")]
#[path = "../src/disk.rs"]
pub mod disk;
#[cfg(target_arch = "wasm32")]
#[path = "../src/files.rs"]
pub mod files;
#[cfg(target_arch = "wasm32")]
#[path = "cloud_recovery/lineage.rs"]
mod lineage;
#[cfg(target_arch = "wasm32")]
#[path = "cloud_recovery/previews.rs"]
mod previews;
#[cfg(target_arch = "wasm32")]
#[path = "cloud_recovery/worker.rs"]
mod worker;

#[cfg(target_arch = "wasm32")]
mod fixture {
    use std::collections::BTreeMap;

    use serde_json::{Value, json};
    use solidcraft_cloud::{Connection, Destination, Error, PendingSave, Published, Result, Snapshot};
    use wasm_bindgen::prelude::*;

    use super::cloud_storage::Store;

    fn require(condition: bool, message: &str) -> Result<()> {
        if condition { Ok(()) } else { Err(Error::Storage(format!("browser recovery test: {message}"))) }
    }

    async fn phase(phase: &str, identity: String) -> Result<Value> {
        if phase.starts_with("lineage-") {
            return super::lineage::phase(phase, identity).await;
        }
        if phase.starts_with("preview-") {
            return super::previews::phase(phase, identity).await;
        }
        if phase.starts_with("worker-") {
            return super::worker::phase(phase, identity).await;
        }
        let connection = Connection { id: identity, repository: "fixture/offline".into(), ..Default::default() };
        let store = Store::open(connection.clone()).await?;
        let destination = Destination { connection: connection.id.clone(), project: "Robot".into(), file: "parts/bracket".into() };
        match phase {
            "write" | "large-write" => {
                let size = if phase == "large-write" { 101 << 20 } else { 256 << 10 };
                let assets = BTreeMap::from([("blobs/example.bin".into(), vec![42; size])]);
                let first = PendingSave::stage(
                    &connection,
                    "Robot",
                    "parts/bracket",
                    None,
                    Snapshot::prepare(solidcraft_engine::doc::Document::new("first offline edit").to_json().into_bytes(), assets.clone())?,
                )?;
                let first = store.stage(first).await?;
                let second = PendingSave::stage(
                    &connection,
                    "Robot",
                    "parts/bracket",
                    Some(first.id),
                    Snapshot::prepare(solidcraft_engine::doc::Document::new("latest offline edit").to_json().into_bytes(), assets)?,
                )?;
                let second = store.stage(second).await?;
                let pending = store.pending().await?;
                require(pending.len() == 2, "both captures are durable")?;
                Ok(json!({"phase":phase,"revision":second.id,"persistent":store.persistent,"asset_bytes":size,
                    "objects":second.manifest.assets.values().flat_map(|asset|asset.chunks.iter()).map(|chunk|chunk.path()).collect::<Result<Vec<_>>>()?}))
            }
            "read" => {
                let pending = store.pending().await?;
                require(pending.len() == 2, "queue survived restart")?;
                let latest = pending.last().ok_or_else(|| Error::Storage("latest capture missing".into()))?;
                let snapshot = store.recover(latest).await?;
                require(
                    snapshot.document == solidcraft_engine::doc::Document::new("latest offline edit").to_json().as_bytes(),
                    "recovered the captured revision",
                )?;
                let assets = Snapshot::hydrate(&snapshot.manifest, &snapshot.document, |path| {
                    snapshot.objects.get(path).cloned().ok_or_else(|| Error::Storage("missing fixture object".into()))
                })?;
                let asset = assets.get("blobs/example.bin").ok_or_else(|| Error::Storage("fixture asset missing".into()))?;
                require(asset.iter().all(|byte| *byte == 42), "asset hashes and content survived")?;
                let cached = store.cached(&destination).await?.ok_or_else(|| Error::Storage("cached capture missing".into()))?;
                require(cached.revision.as_deref() == Some(latest.id.as_str()), "cached head matches the queued child")?;
                Ok(json!({"phase":phase,"revision":latest.id,"asset_bytes":asset.len(),"objects":snapshot.objects.len()}))
            }
            "confirm" => {
                let pending = store.pending().await?;
                let first = pending.first().ok_or_else(|| Error::Storage("first capture missing".into()))?;
                let latest = pending.last().ok_or_else(|| Error::Storage("latest capture missing".into()))?;
                // A stub publication acknowledgement tests the storage transaction only; no
                // network publication is performed or claimed by this fixture.
                store
                    .confirm(
                        first.clone(),
                        Published { destination: destination.clone(), revision: first.id.clone(), commit: "a".repeat(40), conflict: false },
                    )
                    .await?;
                let cached = store.cached(&destination).await?.ok_or_else(|| Error::Storage("newer capture missing".into()))?;
                require(cached.save.id == latest.id, "acknowledging a parent retained its newer queued child")?;
                let conflict = Destination {
                    file: format!("conflicts/parts/bracket-{}", latest.id.get(..12).ok_or_else(|| Error::Storage("short revision".into()))?),
                    ..destination.clone()
                };
                store
                    .confirm(
                        latest.clone(),
                        Published { destination: conflict.clone(), revision: latest.id.clone(), commit: "b".repeat(40), conflict: true },
                    )
                    .await?;
                require(store.pending().await?.is_empty(), "confirmed queue entries removed")?;
                require(store.cached(&destination).await?.is_none(), "conflict copy does not masquerade as the original")?;
                Ok(json!({"phase":phase,"destination":conflict,"revision":latest.id}))
            }
            "confirmed-read" => {
                require(store.pending().await?.is_empty(), "acknowledgement survived restart")?;
                let catalog = solidcraft_cloud::stored_designs(store.cached_entries().await?, store.pending().await?)?;
                require(
                    catalog.iter().all(|entry| entry.phase == solidcraft_cloud::jobs::Phase::Synced && entry.conflict),
                    "confirmed conflict status survived restart",
                )?;
                let destinations: Vec<_> = catalog.into_iter().map(|entry| entry.destination).collect();
                require(destinations.len() == 1, "only the actual conflict destination remains")?;
                let destination = destinations.first().ok_or_else(|| Error::Storage("cached destination missing".into()))?;
                let cached = store.cached(destination).await?.ok_or_else(|| Error::Storage("cached snapshot missing".into()))?;
                let snapshot = store.recover(&cached.save).await?;
                require(
                    snapshot.document == solidcraft_engine::doc::Document::new("latest offline edit").to_json().as_bytes(),
                    "confirmed capture remains recoverable",
                )?;
                require(store.receipt(&cached.save.id).await?.is_some(), "publication receipt survived restart")?;
                Ok(json!({"phase":phase,"destination":destination}))
            }
            _ => Err(Error::Invalid("unknown browser fixture phase".into())),
        }
    }

    #[wasm_bindgen]
    pub async fn cloud_recovery_phase(name: String, identity: String) -> std::result::Result<String, JsValue> {
        let result = phase(&name, identity).await.map_err(|error| JsValue::from(js_sys::Error::new(&error.to_string())))?;
        serde_json::to_string(&result).map_err(|error| JsValue::from(js_sys::Error::new(&error.to_string())))
    }
}
