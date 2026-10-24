//! Resolve configured remotes once, then operate on and confirm those destinations.
use super::*;
use solidcraft_cloud::GitRemote;

impl GitRepository {
    fn remote_names(&self) -> Result<std::collections::BTreeSet<String>> {
        let text = self.text(&["remote"])?;
        let names: std::collections::BTreeSet<_> = text.lines().take(10_001).map(str::to_owned).collect();
        if names.len() > 10_000 {
            return Err(storage("too many Git remotes"));
        }
        Ok(names)
    }

    pub(crate) fn remote_identity(&self) -> Result<Option<GitRemote>> {
        let remote = &self.connection.remote;
        if remote.is_empty() {
            return Ok(None);
        }
        let names = self.remote_names()?;
        let named = names.contains(remote.as_str());
        let (fetch, push) = if named {
            let fetch = self.text(&["remote", "get-url", remote])?;
            let push = self.text(&["remote", "get-url", "--push", "--all", remote])?.lines().map(str::to_owned).collect();
            (fetch, push)
        } else {
            let url = self.text(&["ls-remote", "--get-url", remote])?;
            // A literal remote is used for both reads and writes. An installed push rewrite
            // can change that write destination, so let Git resolve it using a temporary
            // per-command named remote. No repository configuration is written.
            let synthetic = remote_alias(&names, remote)?;
            // `remote -v` includes command-line aliases; `get-url` requires a saved alias.
            let listing = self.text(&["-c", &format!("remote.{synthetic}.url={remote}"), "remote", "-v"])?;
            let prefix = format!("{synthetic}\t");
            let push = listing
                .lines()
                .filter_map(|line| line.strip_prefix(&prefix).and_then(|url| url.strip_suffix(" (push)")))
                .map(str::to_owned)
                .collect();
            (url, push)
        };
        let identity = GitRemote { fetch, push };
        identity.validate()?;
        // Supplying resolved URLs to Git must not apply a second, different rewrite.
        for url in std::iter::once(&identity.fetch).chain(&identity.push) {
            if self.text(&["ls-remote", "--get-url", url])? != *url {
                return Err(storage("nested Git URL rewriting changes a verified destination; choose its final URL"));
            }
        }
        Ok(Some(identity))
    }

    pub(super) fn pinned_remote(&self) -> Result<Option<GitRemote>> {
        let resolved = self.remote_identity()?;
        if self.connection.git_remote.is_some() && self.connection.git_remote != resolved {
            return Err(storage("Git remote destinations changed; verify the connection in Cloud Saves settings before publishing"));
        }
        Ok(resolved)
    }

    pub(crate) fn verified_connection(&self) -> Result<Connection> {
        let remote = self.pinned_remote()?;
        self.test_connection()?;
        if remote != self.remote_identity()? {
            return Err(storage("Git remote changed during the connection test; retry"));
        }
        let mut connection = self.connection.clone();
        connection.checkout = self.checkout.to_str().ok_or_else(|| storage("checkout path must be UTF-8"))?.into();
        connection.git_remote = remote;
        // The initial setup uses a placeholder; the verified checkout supplies a stable id.
        if connection.id == "git" && connection.provider.is_empty() {
            let identity =
                serde_json::to_vec(&(&connection.checkout, &connection.remote, &connection.branch, &connection.git_remote)).map_err(storage)?;
            connection.id = format!("git-{}", solidcraft_cloud::digest(&identity));
        }
        connection.validate()?;
        Ok(connection)
    }

    pub(super) fn fetch_destinations(&self, remote: &GitRemote) -> Result<()> {
        let mut seen = std::collections::BTreeSet::new();
        for url in std::iter::once(&remote.fetch).chain(&remote.push) {
            if !seen.insert(url) {
                continue;
            }
            // An empty destination can receive the first publication from this checkout.
            let branch = self.text(&["ls-remote", "--heads", "--", url, &self.connection.branch])?;
            if branch.is_empty() {
                continue;
            }
            self.run(&["fetch", "--no-tags", "--", url, &self.connection.branch], None)?;
            self.run(&["merge", "--ff-only", "FETCH_HEAD"], None)?;
        }
        Ok(())
    }

    pub(super) fn push_destinations(&self, remote: &GitRemote, commit: &str) -> Result<()> {
        // Recheck before the first remote mutation, even after a long local capture/commit.
        if self.remote_identity()?.as_ref() != Some(remote) {
            return Err(storage("Git remote changed before publication; queued save retained"));
        }
        let refspec = format!("{commit}:refs/heads/{}", self.connection.branch);
        let mut seen = std::collections::BTreeSet::new();
        for url in &remote.push {
            if !seen.insert(url) {
                continue;
            }
            // Explicit pushurl suppresses pushInsteadOf. Preserve that behavior after
            // resolving a named remote by using an isolated per-command alias.
            let alias = remote_alias(&self.remote_names()?, url)?;
            self.run(
                &[
                    "-c",
                    "push.followTags=false",
                    "-c",
                    &format!("remote.{alias}.url={url}"),
                    "-c",
                    &format!("remote.{alias}.pushurl={url}"),
                    "push",
                    "--",
                    &alias,
                    &refspec,
                ],
                None,
            )?;
            let confirmed = self.text(&["ls-remote", "--heads", "--", url, &self.connection.branch])?;
            if confirmed.split_whitespace().next() != Some(commit) {
                return Err(storage("remote publication could not be confirmed; queued copy retained"));
            }
        }
        Ok(())
    }
}

fn remote_alias(names: &std::collections::BTreeSet<String>, url: &str) -> Result<String> {
    let prefix = format!("solidcraft-resolve-{}", solidcraft_cloud::digest(url.as_bytes()));
    (0..128)
        .map(|index| format!("{prefix}-{index}"))
        .find(|candidate| !names.contains(candidate))
        .ok_or_else(|| storage("too many Git remote aliases"))
}
