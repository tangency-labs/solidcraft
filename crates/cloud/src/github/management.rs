//! Provider history reuses Octocrab's typed commit and Git Database APIs.
use super::*;
use crate::management::{History, Revision};

impl GitHub {
    pub async fn history(&self, destination: &Destination, check_document: impl Fn(&[u8]) -> Result<()>) -> Result<History> {
        let (_, manifest, _) = self.head(destination, check_document).await?;
        let mut revisions = Vec::new();
        let mut seen = BTreeSet::new();
        for file in manifest.history_paths(destination)? {
            let path = Destination { file: file.clone(), ..destination.clone() }.design_path()?;
            let commits = self
                .client
                .repos(&self.owner, &self.repository)
                .list_commits()
                .sha(&self.branch)
                .path(format!("{path}/manifest.json"))
                .per_page(50)
                .send()
                .await
                .map_err(api_error)?;
            for commit in commits.items {
                let date = commit.commit.committer.and_then(|a| a.date).or_else(|| commit.commit.author.and_then(|a| a.date));
                let revision = Revision {
                    commit: commit.sha,
                    file: file.clone(),
                    date: date.map(|d| d.to_rfc3339()).unwrap_or_default(),
                    timestamp: date.map(|d| d.timestamp()).unwrap_or_default(),
                    message: commit.commit.message.lines().next().unwrap_or_default().chars().take(512).collect(),
                };
                revision.validate(destination)?;
                if seen.insert(revision.commit.clone()) {
                    revisions.push(revision);
                }
            }
        }
        revisions.sort_by_key(|revision| std::cmp::Reverse(revision.timestamp));
        revisions.truncate(50);
        Ok(History { revision: crate::revision(&manifest).map(str::to_owned), revisions, deleted: manifest.deleted(), queued: false })
    }

    pub async fn open_at(&self, destination: &Destination, commit: &str, check_document: impl Fn(&[u8]) -> Result<()>) -> Result<Snapshot> {
        crate::management::check_commit(commit)?;
        let tree = self.tree_at(commit).await?;
        let (tree, manifest, document) = self.head_in_tree(destination, check_document, tree).await?;
        self.open_head(destination, tree, manifest, document).await
    }
}
