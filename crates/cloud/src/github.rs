//! GitHub Git Database adapter. The application supplies HTTP and credentials;
//! Octocrab owns the GitHub protocol and typed models. This adapter
//! applies SolidCraft project, compatibility and publication policies to its Git Database API.
use std::collections::{BTreeMap, BTreeSet};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use octocrab::{
    Octocrab,
    models::{git::CreateTreeEntry, repos::Object},
    params::repos::Reference,
};
use sha2::Digest as _;

use crate::{Connection, Destination, Error, Manifest, PROJECT_MARKER, PendingSave, Project, Publication, Published, Result, Snapshot};

mod management;
#[cfg(test)]
mod tests;

pub struct GitHub {
    client: Octocrab,
    owner: String,
    repository: String,
    branch: String,
    connection: String,
    endpoint: Connection,
}

struct Tree {
    commit: String,
    sha: String,
    files: BTreeMap<String, String>,
    blocked: BTreeSet<String>,
}

impl Tree {
    fn check_path(&self, path: &str) -> Result<()> {
        crate::check_path(path)?;
        let mut prefix = String::new();
        for part in path.split('/') {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            if self.blocked.contains(&prefix) {
                return Err(invalid("project paths must not contain symbolic links or submodules"));
            }
        }
        Ok(())
    }
}

fn invalid(message: impl std::fmt::Display) -> Error {
    Error::Invalid(message.to_string())
}
fn storage(message: impl std::fmt::Display) -> Error {
    Error::Storage(message.to_string())
}
fn api_error(error: octocrab::Error) -> Error {
    match error {
        octocrab::Error::GitHub { source, .. } => match source.status_code.as_u16() {
            401 | 403 => storage("GitHub access denied; reconnect and check repository Contents read/write permission"),
            429 => storage("GitHub request limit reached; local saves retained for retry"),
            code => storage(format!("GitHub request failed (HTTP {code}); local saves retained")),
        },
        // Never forward arbitrary HTTP errors that might include credential-bearing URLs.
        _ => storage("GitHub request failed; check connectivity and credentials. Local saves retained"),
    }
}
fn check_sha(sha: &str) -> Result<()> {
    if !matches!(sha.len(), 40 | 64) || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid("invalid Git object hash"));
    }
    Ok(())
}

fn blob_hash(bytes: &[u8], hash_length: usize) -> Result<String> {
    let header = format!("blob {}\0", bytes.len());
    match hash_length {
        40 => {
            let mut hash = sha1::Sha1::new();
            hash.update(header.as_bytes());
            hash.update(bytes);
            Ok(format!("{:x}", hash.finalize()))
        }
        64 => {
            let mut hash = sha2::Sha256::new();
            hash.update(header.as_bytes());
            hash.update(bytes);
            Ok(format!("{:x}", hash.finalize()))
        }
        _ => Err(invalid("unsupported Git object hash")),
    }
}

pub use crate::setup::github_repository as repository_name;
mod setup;
pub use setup::{exists, prepare};

impl GitHub {
    /// The platform supplies an authenticated Octocrab client. Browser builds use its public
    /// service/executor extension points; no endpoint or response model is reimplemented here.
    pub fn new(client: Octocrab, connection: &Connection) -> Result<Self> {
        connection.validate()?;
        let branch = &connection.branch;
        if branch.is_empty()
            || branch.len() > 255
            || branch.contains([' ', '~', '^', ':', '?', '*', '[', '\\'])
            || branch.contains("..")
            || branch.contains("@{")
            || branch.split('/').any(|s| s.is_empty() || s.starts_with('.') || s.ends_with('.') || s.ends_with(".lock"))
        {
            return Err(invalid("invalid Git branch"));
        }
        let repository = repository_name(&connection.repository)?;
        let (owner, repository) = repository.split_once('/').ok_or_else(|| invalid("invalid GitHub repository"))?;
        Ok(Self {
            client,
            owner: owner.into(),
            repository: repository.into(),
            branch: connection.branch.clone(),
            connection: connection.id.clone(),
            endpoint: connection.clone(),
        })
    }

    async fn tree(&self) -> Result<Tree> {
        let git = self.client.git(&self.owner, &self.repository);
        let reference = git.get_ref(&Reference::Branch(self.branch.clone())).await.map_err(api_error)?;
        let Object::Commit { sha: commit, .. } = reference.object else {
            return Err(invalid("branch does not refer to a commit"));
        };
        self.tree_at(&commit).await
    }

    async fn tree_at(&self, commit: &str) -> Result<Tree> {
        let git = self.client.git(&self.owner, &self.repository);
        check_sha(commit)?;
        let commit_value = git.get_commit(commit).await.map_err(api_error)?;
        if commit_value.sha != commit {
            return Err(invalid("repository returned a different commit"));
        }
        let tree_sha = commit_value.tree.sha;
        check_sha(&tree_sha)?;
        let tree = git.get_tree(&tree_sha).recursive(true).send().await.map_err(api_error)?;
        if tree.truncated || tree.tree.len() > 100_000 {
            return Err(storage("repository tree is too large to list safely"));
        }
        let mut files = BTreeMap::new();
        let mut blocked = BTreeSet::new();
        for entry in tree.tree {
            if entry.r#type == "blob" && matches!(entry.mode.as_str(), "100644" | "100755") {
                check_sha(&entry.sha)?;
                files.insert(entry.path, entry.sha);
            } else if entry.r#type != "tree" {
                blocked.insert(entry.path);
            }
        }
        Ok(Tree { commit: commit.into(), sha: tree_sha, files, blocked })
    }

    async fn read(&self, tree: &Tree, path: &str, max: usize) -> Result<Option<Vec<u8>>> {
        tree.check_path(path)?;
        let Some(hash) = tree.files.get(path) else {
            return Ok(None);
        };
        let blob = self.client.git(&self.owner, &self.repository).get_blob(hash).await.map_err(api_error)?;
        if blob.size > max || blob.encoding != "base64" {
            return Err(invalid("unsupported or oversized GitHub blob"));
        }
        if blob.content.len() > max.div_ceil(3).saturating_mul(4).saturating_add(max / 32 + 1024) {
            return Err(invalid("oversized GitHub blob encoding"));
        }
        let encoded: String = blob.content.chars().filter(|c| !matches!(c, '\r' | '\n')).collect();
        let bytes = STANDARD.decode(encoded).map_err(invalid)?;
        if bytes.len() != blob.size {
            return Err(invalid("GitHub blob length mismatch"));
        }
        Ok(Some(bytes))
    }

    pub async fn test_connection(&self) -> Result<()> {
        self.tree().await.map(|_| ())
    }

    pub async fn list(&self) -> Result<Vec<Destination>> {
        self.list_with_status(&mut []).await
    }

    pub async fn list_with_status(&self, stored: &mut [crate::StoredDesign]) -> Result<Vec<Destination>> {
        self.list_visibility(stored, false).await
    }

    pub async fn list_deleted(&self) -> Result<Vec<Destination>> {
        self.list_visibility(&mut [], true).await
    }

    async fn list_visibility(&self, stored: &mut [crate::StoredDesign], deleted: bool) -> Result<Vec<Destination>> {
        if stored.len() > 10_000 {
            return Err(invalid("stored catalog is too large"));
        }
        for entry in stored.iter() {
            entry.validate(&self.connection)?;
        }
        let tree = self.tree().await?;
        let mut projects = Vec::new();
        for path in tree.files.keys() {
            if let Some(project) = path.strip_suffix(&format!("/{PROJECT_MARKER}")) {
                let bytes = self.read(&tree, path, crate::MAX_MANIFEST_BYTES).await?.ok_or_else(|| invalid("project marker missing"))?;
                Project::read(&bytes)?;
                projects.push(project.to_string());
            }
        }
        let mut designs = Vec::new();
        for path in tree.files.keys() {
            if let Some(prefix) = path.strip_suffix("/manifest.json")
                && let Some(project) = projects.iter().filter(|p| prefix.starts_with(&format!("{p}/"))).max_by_key(|p| p.len())
            {
                let file = prefix.strip_prefix(&format!("{project}/")).ok_or_else(|| invalid("invalid design path"))?;
                let destination = Destination { connection: self.connection.clone(), project: project.clone(), file: file.into() };
                destination.validate()?;
                let manifest = self.read(&tree, path, crate::MAX_MANIFEST_BYTES).await?.and_then(|bytes| Manifest::read(&bytes).ok());
                let in_trash = manifest.as_ref().is_some_and(|m| m.deleted() && !m.moved());
                let moved = manifest.as_ref().is_some_and(Manifest::moved);
                if !moved && in_trash == deleted {
                    designs.push(destination);
                }
            }
        }
        for entry in
            stored.iter_mut().filter(|entry| matches!(entry.phase, crate::jobs::Phase::Ready | crate::jobs::Phase::Synced) && !entry.outdated)
        {
            let path = format!("{}/manifest.json", entry.destination.design_path()?);
            let manifest = self.read(&tree, &path, crate::MAX_MANIFEST_BYTES).await?.and_then(|bytes| Manifest::read(&bytes).ok());
            if !manifest.as_ref().map(|manifest| entry.matches_manifest(manifest)).transpose()?.unwrap_or(false) {
                entry.phase = crate::jobs::Phase::Ready;
                entry.outdated = true;
            }
        }
        Ok(designs)
    }

    async fn head(&self, destination: &Destination, check_document: impl Fn(&[u8]) -> Result<()>) -> Result<(Tree, Manifest, Vec<u8>)> {
        destination.validate()?;
        if destination.connection != self.connection {
            return Err(invalid("repository connection mismatch"));
        }
        let tree = self.tree().await?;
        self.head_in_tree(destination, check_document, tree).await
    }

    async fn head_in_tree(
        &self,
        destination: &Destination,
        check_document: impl Fn(&[u8]) -> Result<()>,
        tree: Tree,
    ) -> Result<(Tree, Manifest, Vec<u8>)> {
        destination.validate()?;
        if destination.connection != self.connection {
            return Err(invalid("repository connection mismatch"));
        }
        let marker = self
            .read(&tree, &format!("{}/{PROJECT_MARKER}", destination.project), crate::MAX_MANIFEST_BYTES)
            .await?
            .ok_or_else(|| invalid("project marker missing"))?;
        Project::read(&marker)?;
        let prefix = destination.design_path()?;
        let bytes = self
            .read(&tree, &format!("{prefix}/manifest.json"), crate::MAX_MANIFEST_BYTES)
            .await?
            .ok_or_else(|| invalid("design manifest missing"))?;
        let manifest = Manifest::read(&bytes)?;
        let document =
            self.read(&tree, &format!("{prefix}/document.json"), crate::MAX_MANIFEST_BYTES).await?.ok_or_else(|| invalid("document missing"))?;
        check_document(&document)?;
        if crate::digest(&document) != manifest.document_hash {
            return Err(invalid("document hash mismatch"));
        }
        Ok((tree, manifest, document))
    }

    async fn preview_objects(&self, tree: &Tree, destination: &Destination, manifest: &Manifest) -> Result<BTreeMap<String, Vec<u8>>> {
        let mut bytes = BTreeMap::new();
        if let Some(thumbnail) = crate::Thumbnail::read(manifest)? {
            for path in thumbnail.paths()? {
                if let Ok(Some(value)) = self.read(tree, &format!("{}/{path}", destination.project), crate::MAX_THUMBNAIL_BYTES).await {
                    bytes.insert(path, value);
                }
            }
        }
        crate::Thumbnail::objects(manifest, |path| bytes.get(path).cloned().ok_or_else(|| storage("preview unavailable")))
    }

    pub async fn thumbnail(&self, destination: &Destination, check_document: impl Fn(&[u8]) -> Result<()>) -> Result<Option<Vec<u8>>> {
        let (tree, manifest, _) = self.head(destination, check_document).await?;
        let objects = self.preview_objects(&tree, destination, &manifest).await?;
        crate::Thumbnail::png(&manifest, |path| objects.get(path).cloned().ok_or_else(|| storage("preview unavailable")))
    }

    pub async fn open(&self, destination: &Destination, check_document: impl Fn(&[u8]) -> Result<()>) -> Result<Snapshot> {
        let (tree, manifest, document) = self.head(destination, check_document).await?;
        self.open_head(destination, tree, manifest, document).await
    }

    async fn open_head(&self, destination: &Destination, tree: Tree, manifest: Manifest, document: Vec<u8>) -> Result<Snapshot> {
        let mut objects = BTreeMap::new();
        for asset in manifest.assets.values() {
            for chunk in &asset.chunks {
                let path = chunk.path()?;
                if let std::collections::btree_map::Entry::Vacant(entry) = objects.entry(path.clone()) {
                    let bytes = self
                        .read(&tree, &format!("{}/{path}", destination.project), crate::CHUNK_BYTES)
                        .await?
                        .ok_or_else(|| invalid("asset chunk missing"))?;
                    entry.insert(bytes);
                }
            }
        }
        Snapshot::hydrate(&manifest, &document, |path| objects.get(path).cloned().ok_or_else(|| invalid("asset chunk missing")))?;
        objects.extend(self.preview_objects(&tree, destination, &manifest).await?);
        Ok(Snapshot { manifest, document, objects })
    }

    pub async fn publish(
        &self,
        save: &PendingSave,
        snapshot: impl Fn() -> Result<Snapshot>,
        check_document: impl Fn(&[u8]) -> Result<()>,
    ) -> Result<Published> {
        self.publish_async(save, || std::future::ready(snapshot()), check_document).await
    }

    /// Browser recovery is asynchronous. Re-read captured objects for a retry instead of
    /// retaining and cloning another entire snapshot during each upload attempt.
    pub async fn publish_async<F: std::future::Future<Output = Result<Snapshot>>>(
        &self,
        save: &PendingSave,
        snapshot: impl Fn() -> F,
        check_document: impl Fn(&[u8]) -> Result<()>,
    ) -> Result<Published> {
        self.publish_with_parent(save, snapshot, check_document, None).await
    }

    pub async fn publish_with_parent<F: std::future::Future<Output = Result<Snapshot>>>(
        &self,
        save: &PendingSave,
        snapshot: impl Fn() -> F,
        check_document: impl Fn(&[u8]) -> Result<()>,
        parent: Option<&Published>,
    ) -> Result<Published> {
        save.check_connection(&self.endpoint)?;
        if save.destination.connection != self.connection {
            return Err(invalid("repository connection mismatch"));
        }
        let target = Published::target(save, parent)?;
        for _ in 0..3 {
            let snapshot = snapshot().await?;
            check_document(&snapshot.document)?;
            let tree = self.tree().await?;
            let marker = self.read(&tree, &format!("{}/{PROJECT_MARKER}", save.destination.project), crate::MAX_MANIFEST_BYTES).await?;
            let prefix = target.design_path()?;
            let existing = self.read(&tree, &format!("{prefix}/manifest.json"), crate::MAX_MANIFEST_BYTES).await?;
            if let Some(bytes) = &existing {
                Manifest::read(bytes)?;
            }
            if let Some(document) = self.read(&tree, &format!("{prefix}/document.json"), crate::MAX_MANIFEST_BYTES).await? {
                check_document(&document)?;
            }
            let publication = Publication::prepare_with_parent(save, snapshot, marker.as_deref(), existing.as_deref(), parent)?;
            if publication.already_published {
                return Ok(Published {
                    destination: publication.destination,
                    revision: save.id.clone(),
                    commit: tree.commit,
                    conflict: publication.conflict,
                });
            }
            if matches!(save.manifest.change()?, Some(crate::management::Change::Rename { .. })) {
                let target = publication.destination.design_path()?;
                let manifest = self.read(&tree, &format!("{target}/manifest.json"), crate::MAX_MANIFEST_BYTES).await?;
                if let Some(bytes) = &manifest {
                    Manifest::read(bytes)?;
                }
                let document = self.read(&tree, &format!("{target}/document.json"), crate::MAX_MANIFEST_BYTES).await?;
                if let Some(bytes) = &document {
                    check_document(bytes)?;
                }
                if manifest.is_some() || document.is_some() {
                    return Err(Error::Conflict);
                }
            }
            if publication.conflict && publication.destination != target {
                let path = format!("{}/manifest.json", publication.destination.design_path()?);
                if let Some(bytes) = self.read(&tree, &path, crate::MAX_MANIFEST_BYTES).await? {
                    let previous = Manifest::read(&bytes)?;
                    if crate::revision(&previous) == Some(save.id.as_str()) {
                        return Ok(Published {
                            destination: publication.destination,
                            revision: save.id.clone(),
                            commit: tree.commit,
                            conflict: true,
                        });
                    }
                    return Err(Error::Conflict);
                }
            }
            let mut changes = Vec::new();
            let object_prefix = format!("{}/objects/", publication.destination.project);
            for (path, bytes) in publication.files {
                tree.check_path(&path)?;
                if path.starts_with(&object_prefix)
                    && let Some(existing) = tree.files.get(&path)
                {
                    // Verify reuse against Git's blob identity without downloading large assets.
                    if blob_hash(&bytes, existing.len())? != *existing {
                        return Err(invalid("existing repository asset does not match its content address; local save retained"));
                    }
                    continue;
                }
                let blob = self
                    .client
                    .git(&self.owner, &self.repository)
                    .create_blob(STANDARD.encode(bytes))
                    .encoding("base64")
                    .send()
                    .await
                    .map_err(api_error)?;
                check_sha(&blob.sha)?;
                changes.push(CreateTreeEntry::new(path, "100644", "blob").with_sha(blob.sha));
            }
            let git = self.client.git(&self.owner, &self.repository);
            let new_tree = git.create_tree(changes).base_tree(tree.sha).send().await.map_err(api_error)?;
            let action = save.manifest.change()?.map(|c| c.message()).unwrap_or("Save");
            let commit = git
                .create_commit(format!("{action} {} ({})", save.destination.file, save.id), new_tree.sha)
                .parents(vec![tree.commit])
                .send()
                .await
                .map_err(api_error)?;
            let commit = commit.sha;
            check_sha(&commit)?;
            match git.update_ref(&Reference::Branch(self.branch.clone()), &commit).force(false).send().await {
                Ok(_) => {}
                Err(octocrab::Error::GitHub { source, .. }) if matches!(source.status_code.as_u16(), 409 | 422) => continue,
                Err(error) => return Err(api_error(error)),
            }
            // A successful non-forced ref update confirms publication even if another client
            // advances the branch immediately afterwards.
            return Ok(Published { destination: publication.destination, revision: save.id.clone(), commit, conflict: publication.conflict });
        }
        Err(Error::Conflict)
    }
}
