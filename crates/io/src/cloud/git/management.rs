//! Read-only history and historical snapshots from an ordinary Git checkout.
use super::*;
use solidcraft_cloud::management::{History, Revision, check_commit};

impl GitRepository {
    pub fn history(&self, destination: &Destination) -> Result<History> {
        let (manifest, _) = self.head(destination)?;
        let mut revisions = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for file in manifest.history_paths(destination)? {
            let path = Destination { file: file.clone(), ..destination.clone() }.design_path()?;
            let bytes = self.run(&["log", "-50", "--format=%H%x00%cI%x00%ct%x00%s", "--", &format!("{path}/manifest.json")], None)?;
            let text = String::from_utf8(bytes).map_err(storage)?;
            for line in text.lines() {
                let mut fields = line.splitn(4, '\0');
                let revision = Revision {
                    commit: fields.next().unwrap_or_default().into(),
                    date: fields.next().unwrap_or_default().into(),
                    timestamp: fields.next().unwrap_or_default().parse().map_err(storage)?,
                    message: fields.next().unwrap_or_default().chars().take(512).collect(),
                    file: file.clone(),
                };
                revision.validate(destination)?;
                if seen.insert(revision.commit.clone()) {
                    revisions.push(revision);
                }
            }
        }
        revisions.sort_by_key(|revision| std::cmp::Reverse(revision.timestamp));
        revisions.truncate(50);
        Ok(History { revision: solidcraft_cloud::revision(&manifest).map(str::to_owned), revisions, deleted: manifest.deleted(), queued: false })
    }

    pub fn snapshot_at(&self, destination: &Destination, commit: &str) -> Result<Snapshot> {
        check_commit(commit)?;
        destination.validate()?;
        if destination.connection != self.connection.id {
            return Err(storage("repository connection mismatch"));
        }
        self.run(&["merge-base", "--is-ancestor", commit, "HEAD"], None)?;
        let marker = self
            .committed(commit, &format!("{}/{PROJECT_MARKER}", destination.project))?
            .ok_or_else(|| storage("project marker missing from revision"))?;
        solidcraft_cloud::Project::read(&marker)?;
        let prefix = destination.design_path()?;
        let manifest =
            Manifest::read(&self.committed(commit, &format!("{prefix}/manifest.json"))?.ok_or_else(|| storage("design missing from revision"))?)?;
        let document = self.committed(commit, &format!("{prefix}/document.json"))?.ok_or_else(|| storage("document missing from revision"))?;
        super::super::check_document(&document)?;
        let mut objects = std::collections::BTreeMap::new();
        Snapshot::hydrate(&manifest, &document, |path| {
            let bytes = self.committed(commit, &format!("{}/{path}", destination.project))?.ok_or_else(|| storage("asset missing from revision"))?;
            objects.insert(path.into(), bytes.clone());
            Ok(bytes)
        })?;
        objects.extend(solidcraft_cloud::Thumbnail::objects(&manifest, |path| {
            self.committed(commit, &format!("{}/{path}", destination.project))?.ok_or_else(|| storage("preview missing from revision"))
        })?);
        Ok(Snapshot { manifest, document, objects })
    }
}
