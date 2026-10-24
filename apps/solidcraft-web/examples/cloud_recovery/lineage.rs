//! Confirmed parent destinations survive browser restarts without rewriting captures.
use crate::cloud_storage::Store;
use serde_json::{Value, json};
use solidcraft_cloud::{Connection, Destination, Error, PendingSave, Published, Result, Snapshot};
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
