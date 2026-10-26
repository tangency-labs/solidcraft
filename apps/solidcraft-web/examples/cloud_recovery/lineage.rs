//! Confirmed parent destinations survive browser restarts without rewriting captures.
use crate::cloud_storage::Store;
use serde_json::{Value, json};
use solidcraft_cloud::{Connection, Destination, Error, PendingSave, Published, Result, Snapshot, management::Change};
use std::collections::BTreeMap;

fn require(condition: bool, message: &str) -> Result<()> {
    if condition { Ok(()) } else { Err(Error::Storage(message.into())) }
}
fn staged(connection: &Connection, name: &str, parent: Option<String>) -> Result<solidcraft_cloud::StagedSave> {
    PendingSave::stage(
        connection,
        "Robot",
        "parts/bracket",
        parent,
        Snapshot::prepare(solidcraft_engine::doc::Document::new(name).to_json().into_bytes(), BTreeMap::new())?,
    )
}
pub(super) async fn phase(name: &str, identity: String) -> Result<Value> {
    let connection = Connection { id: identity, repository: "fixture/lineage".into(), ..Default::default() };
    let store = Store::open(connection.clone()).await?;
    match name {
        "lineage-management-write" => {
            let first = store.stage(staged(&connection, "Custom title", None)?).await?;
            store
                .confirm(
                    first.clone(),
                    Published { destination: first.destination.clone(), revision: first.id.clone(), commit: "a".repeat(40), conflict: false },
                )
                .await?;
            let snapshot = solidcraft_engine::io::cloud::prepare_change(
                store.recover(&first).await?,
                &first.destination,
                &Change::Rename { file: "parts/renamed bracket".into() },
            )?;
            store.stage(PendingSave::stage(&connection, "Robot", "parts/bracket", Some(first.id), snapshot)?).await?;
        }
        "lineage-management-read" => {
            let pending = store.pending().await?;
            require(pending.len() == 1, "one rename survives browser restart")?;
            let rename = pending.first().ok_or_else(|| Error::Storage("queued rename missing".into()))?;
            let change = rename.manifest.change()?.ok_or_else(|| Error::Storage("rename intent missing".into()))?;
            require(matches!(change, Change::Rename { .. }), "queued rename intent retained")?;
            let destination = change.destination(&rename.destination);
            let published = Published { destination: destination.clone(), revision: rename.id.clone(), commit: "b".repeat(40), conflict: false };
            store.confirm(rename.clone(), published.clone()).await?;
            store.confirm(rename.clone(), published).await?;
            require(store.cached(&rename.destination).await?.is_none(), "rename removes the old cached path")?;
            let cached = store.cached(&destination).await?.ok_or_else(|| Error::Storage("renamed cache missing".into()))?;
            let snapshot = store.recover(&cached.save).await?;
            let metadata: Value = serde_json::from_slice(&snapshot.document).map_err(|e| Error::Storage(e.to_string()))?;
            require(metadata["name"] == "renamed bracket", "renamed document title retained")?;
            let snapshot = solidcraft_engine::io::cloud::prepare_change(snapshot, &destination, &Change::Delete)?;
            let deletion = store.stage(PendingSave::stage(&connection, "Robot", &destination.file, Some(rename.id.clone()), snapshot)?).await?;
            require(
                solidcraft_cloud::stored_designs(store.cached_entries().await?, store.pending().await?)?.is_empty(),
                "queued Trash hides the active design",
            )?;
            store.discard_management(deletion.clone()).await?;
            require(store.pending().await?.is_empty(), "discard removes only the queued intent")?;
            require(!store.recover(&deletion).await?.document.is_empty(), "discard preserves recovery objects")?;
            let retry =
                PendingSave::stage(&connection, "Robot", &destination.file, deletion.expected_revision.clone(), store.recover(&deletion).await?)?;
            require(retry.record.id == deletion.id, "retried Trash retains its immutable revision")?;
            store.stage(retry).await?;
        }
        "lineage-management-confirm" => {
            let pending = store.pending().await?;
            require(pending.len() == 1, "retried Trash survives browser restart without duplicates")?;
            let deletion = pending.first().ok_or_else(|| Error::Storage("queued Trash missing".into()))?;
            require(deletion.manifest.deleted(), "Trash intent retained")?;
            store
                .confirm(
                    deletion.clone(),
                    Published { destination: deletion.destination.clone(), revision: deletion.id.clone(), commit: "c".repeat(40), conflict: false },
                )
                .await?;
        }
        "lineage-management-confirmed-read" => {
            require(store.pending().await?.is_empty(), "confirmed Trash remains acknowledged after restart")?;
            require(
                solidcraft_cloud::stored_designs(store.cached_entries().await?, store.pending().await?)?.is_empty(),
                "confirmed Trash stays out of active designs",
            )?;
            let destination = Destination { connection: connection.id.clone(), project: "Robot".into(), file: "parts/renamed bracket".into() };
            let cached = store.cached(&destination).await?.ok_or_else(|| Error::Storage("recoverable Trash missing".into()))?;
            require(cached.save.manifest.deleted() && store.receipt(&cached.save.id).await?.is_some(), "Trash and receipt retained")?;
            store.recover(&cached.save).await?;
        }
        "lineage-write" => {
            let first = store.stage(staged(&connection, "Offline first", None)?).await?;
            let child = store.stage(staged(&connection, "Offline child", Some(first.id.clone()))?).await?;
            let destination = Destination {
                file: format!("conflicts/parts/bracket-{}", first.id.get(..12).ok_or_else(|| Error::Invalid("revision".into()))?),
                ..first.destination.clone()
            };
            store.confirm(first.clone(), Published { destination, revision: first.id, commit: "a".repeat(40), conflict: true }).await?;
            require(store.pending().await?.first().is_some_and(|save| save.id == child.id), "child remains queued after its parent publishes")?;
        }
        "lineage-read" => {
            let child = store.pending().await?.pop().ok_or_else(|| Error::Storage("offline child missing".into()))?;
            let parent = store
                .receipt(child.expected_revision.as_deref().ok_or_else(|| Error::Invalid("parent".into()))?)
                .await?
                .ok_or_else(|| Error::Storage("confirmed parent missing".into()))?;
            let destination = Published::target(&child, Some(&parent))?;
            require(destination != child.destination, "confirmed conflict destination restored")?;
            let published = Published { destination: destination.clone(), revision: child.id.clone(), commit: "b".repeat(40), conflict: true };
            store.confirm(child.clone(), published.clone()).await?;
            let cached = store.cached(&destination).await?.ok_or_else(|| Error::Storage("updated conflict copy missing".into()))?;
            require(cached.revision.as_deref() == Some(child.id.as_str()), "child replaces its parent at the actual destination")?;
            let snapshot = store.recover(&cached.save).await?;
            require(
                snapshot.document == solidcraft_engine::doc::Document::new("Offline child").to_json().as_bytes(),
                "exact child document retained",
            )?;
            require(store.pending().await?.is_empty(), "child queue acknowledged")?;
            store.confirm(child.clone(), published).await?;
            require(store.receipt(&child.id).await?.is_some(), "retry retains the same receipt")?;
            let retried = store.stage(staged(&connection, "Offline child", child.expected_revision.clone())?).await?;
            require(retried.id == child.id && store.pending().await?.is_empty(), "retry of a confirmed capture never queues a duplicate")?;
        }
        _ => return Err(Error::Invalid("unknown lineage fixture phase".into())),
    }
    Ok(json!({"phase":name}))
}
