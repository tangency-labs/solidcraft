//! Confirm received design paths without disturbing unrelated working or staged files.
use super::*;
use std::collections::BTreeSet;

impl GitRepository {
    pub(crate) fn confirm_synchronized(&self, destination: &Destination, snapshot: &Snapshot) -> Result<()> {
        let prefix = destination.design_path()?;
        let mut paths: BTreeSet<String> = snapshot.objects.keys().map(|path| format!("{}/{path}", destination.project)).collect();
        paths.extend([format!("{prefix}/manifest.json"), format!("{prefix}/document.json"), format!("{}/{PROJECT_MARKER}", destination.project)]);
        let tracked = self.run(&["ls-files", "-z"], None)?;
        let tracked: BTreeSet<&[u8]> = tracked.split(|byte| *byte == 0).collect();
        if paths.iter().any(|path| !tracked.contains(path.as_bytes())) {
            return Err(storage("commit this design's files before synchronizing; local copies are retained"));
        }
        self.confirm_paths("HEAD", &paths)?;
        if let Some(remote) = self.pinned_remote()? {
            for url in remote.push.iter().collect::<BTreeSet<_>>() {
                let reference = self.text(&["ls-remote", "--heads", "--", url, &self.connection.branch])?;
                let commit = reference
                    .split_whitespace()
                    .next()
                    .filter(|sha| matches!(sha.len(), 40 | 64) && sha.bytes().all(|b| b.is_ascii_hexdigit()))
                    .ok_or_else(|| storage("publish this design's branch before synchronizing; local copies are retained"))?;
                self.confirm_paths(commit, &paths)?;
            }
        }
        Ok(())
    }

    fn confirm_paths(&self, revision: &str, paths: &BTreeSet<String>) -> Result<()> {
        let changed = self.run(&["diff", "--name-only", "-z", revision, "--"], None)?;
        if changed.split(|byte| *byte == 0).filter_map(|path| std::str::from_utf8(path).ok()).any(|path| paths.contains(path)) {
            return Err(storage("this design has unpublished checkout changes; synchronize them in Git first. Local copies are retained"));
        }
        Ok(())
    }
}
