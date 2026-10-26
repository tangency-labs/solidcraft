//! Prepare management captures without rebuilding geometry or decoding immutable assets.
use super::*;
use solidcraft_cloud::{Destination, management::Change};

pub fn prepare_change(mut snapshot: Snapshot, source: &Destination, change: &Change) -> Result<Snapshot> {
    change.validate(source)?;
    snapshot.manifest.write()?;
    check_document(&snapshot.document)?;
    if let Change::Rename { file } = change {
        let png = read_thumbnail(&snapshot.manifest, |path| snapshot.objects.get(path).cloned().ok_or_else(|| invalid("preview unavailable")))
            .ok()
            .flatten();
        let name = file.rsplit('/').next().unwrap_or(file);
        let mut metadata: serde_json::Value = serde_json::from_slice(&snapshot.document).map_err(invalid)?;
        metadata.as_object_mut().ok_or_else(|| invalid("invalid document metadata"))?.insert("name".into(), name.into());
        snapshot.document = serde_json::to_vec_pretty(&metadata).map_err(invalid)?;
        snapshot.manifest.document_hash = digest(&snapshot.document);
        snapshot.manifest.extra.insert("title".into(), name.into());
        if let Some(png) = png {
            let _ = solidcraft_cloud::attach_thumbnail(&mut snapshot, png);
        }
        let mut paths = snapshot.manifest.history_paths(source)?;
        paths.retain(|p| p != file);
        paths.truncate(16);
        snapshot.manifest.extra.insert("previous_paths".into(), serde_json::json!(paths));
    }
    if let Change::Restore { commit, file } = change {
        snapshot.manifest.extra.insert("restored_from".into(), serde_json::json!({"commit":commit,"file":file}));
    }
    if snapshot.manifest.deleted() {
        return Err(invalid("this revision is in Trash; choose an earlier revision to restore"));
    }
    snapshot.manifest.set_change(change)?;
    snapshot.manifest.extra.remove("revision");
    snapshot.manifest.extra.remove("parent_revision");
    snapshot.manifest.write()?;
    Ok(snapshot)
}
