//! Local-first opening and resumable, content-addressed asset downloads.
use super::*;
use crate::DownloadCache;

impl GitHub {
    /// Unchanged branches need one small request; verified heads survive application restarts.
    pub async fn is_current(&self, cached: &crate::StoredDesign) -> Result<bool> {
        self.is_current_cached(cached, &crate::download::Uncached).await
    }

    pub async fn is_current_cached(&self, cached: &crate::StoredDesign, cache: &impl DownloadCache) -> Result<bool> {
        cached.validate(&self.connection)?;
        let reference = self.client.git(&self.owner, &self.repository).get_ref(&Reference::Branch(self.branch.clone())).await.map_err(api_error)?;
        let Object::Commit { sha: commit, .. } = reference.object else {
            return Err(invalid("branch does not refer to a commit"));
        };
        check_sha(&commit)?;
        let key = self.checked_path(&cached.destination)?;
        if let Some(bytes) = cache.load(&key).await?
            && let Ok(checked) = serde_json::from_slice::<CheckedHead>(&bytes)
            && checked.format == "solidcraft-checked-head/1"
            && checked.commit == commit
            && checked.document_hash == cached.document_hash
            && checked.manifest_hash == cached.manifest_hash
            && checked.revision == cached.revision
        {
            return Ok(true);
        }
        let tree = self.tree_at(&commit).await?;
        let destination = &cached.destination;
        let (tree, manifest, _) = self.head_in_tree(destination, |_| Ok(()), tree).await?;
        let current = !manifest.deleted() && cached.matches_manifest(&manifest)?;
        if current {
            self.checkpoint(cache, destination, &tree, &manifest).await?;
        }
        Ok(current)
    }

    fn checked_path(&self, destination: &Destination) -> Result<String> {
        Ok(format!("checks/{}", crate::digest(&serde_json::to_vec(&(&self.endpoint, destination)).map_err(invalid)?)))
    }

    async fn checkpoint(&self, cache: &impl DownloadCache, destination: &Destination, tree: &Tree, manifest: &Manifest) -> Result<()> {
        let checked = CheckedHead {
            format: "solidcraft-checked-head/1".into(),
            commit: tree.commit.clone(),
            document_hash: manifest.document_hash.clone(),
            manifest_hash: crate::manifest_digest(manifest)?,
            revision: crate::revision(manifest).map(str::to_owned),
        };
        cache.store(&self.checked_path(destination)?, &serde_json::to_vec(&checked).map_err(invalid)?).await
    }

    pub async fn open(&self, destination: &Destination, check_document: impl Fn(&[u8]) -> Result<()>) -> Result<Snapshot> {
        self.open_cached(destination, check_document, &crate::download::Uncached, |_, _| Ok(())).await
    }

    pub async fn open_cached(
        &self,
        destination: &Destination,
        check_document: impl Fn(&[u8]) -> Result<()>,
        cache: &impl DownloadCache,
        progress: impl Fn(usize, usize) -> Result<()>,
    ) -> Result<Snapshot> {
        let (tree, manifest, document) = self.head(destination, check_document).await?;
        self.open_head_cached(destination, tree, manifest, document, cache, progress).await
    }

    pub(super) async fn open_head(&self, destination: &Destination, tree: Tree, manifest: Manifest, document: Vec<u8>) -> Result<Snapshot> {
        self.open_head_cached(destination, tree, manifest, document, &crate::download::Uncached, |_, _| Ok(())).await
    }

    async fn open_head_cached(
        &self,
        destination: &Destination,
        tree: Tree,
        manifest: Manifest,
        document: Vec<u8>,
        cache: &impl DownloadCache,
        progress: impl Fn(usize, usize) -> Result<()>,
    ) -> Result<Snapshot> {
        let total = manifest.assets.values().flat_map(|a| a.chunks.iter().map(|c| &c.hash)).collect::<BTreeSet<_>>().len();
        let mut objects = BTreeMap::new();
        progress(0, total)?;
        for asset in manifest.assets.values() {
            for chunk in &asset.chunks {
                let path = chunk.path()?;
                if let std::collections::btree_map::Entry::Vacant(entry) = objects.entry(path.clone()) {
                    let bytes = match cache.load(&path).await? {
                        Some(bytes) => bytes,
                        None => {
                            let bytes = self
                                .read(&tree, &format!("{}/{path}", destination.project), crate::CHUNK_BYTES)
                                .await?
                                .ok_or_else(|| invalid("asset chunk missing"))?;
                            if crate::digest(&bytes) != chunk.hash {
                                return Err(invalid("asset chunk hash mismatch"));
                            }
                            cache.store(&path, &bytes).await?;
                            bytes
                        }
                    };
                    if bytes.len() > crate::CHUNK_BYTES || crate::digest(&bytes) != chunk.hash {
                        return Err(invalid("cached asset chunk hash mismatch"));
                    }
                    entry.insert(bytes);
                    progress(objects.len(), total)?;
                }
            }
        }
        Snapshot::hydrate(&manifest, &document, |path| objects.get(path).cloned().ok_or_else(|| invalid("asset chunk missing")))?;
        objects.extend(self.preview_objects(&tree, destination, &manifest).await?);
        self.checkpoint(cache, destination, &tree, &manifest).await?;
        Ok(Snapshot { manifest, document, objects })
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CheckedHead {
    format: String,
    commit: String,
    document_hash: String,
    manifest_hash: String,
    revision: Option<String>,
}
