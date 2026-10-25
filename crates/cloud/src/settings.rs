//! Local connection configuration. None of these fields belong in shared design manifests.
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "path", rename_all = "snake_case")]
pub enum SshIdentity {
    #[default]
    System,
    File(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Authentication {
    #[default]
    System,
    Https,
    Ssh,
}

/// Verified native Git destinations. These are local connection metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitRemote {
    pub fetch: String,
    pub push: Vec<String>,
}

impl GitRemote {
    pub fn validate(&self) -> Result<()> {
        if self.push.is_empty() || self.push.len() > 16 {
            return Err(Error::Invalid("choose between one and sixteen Git push destinations".into()));
        }
        for endpoint in std::iter::once(&self.fetch).chain(&self.push) {
            if endpoint.is_empty() || endpoint.starts_with('-') || endpoint.len() > 4096 || endpoint.chars().any(char::is_control) {
                return Err(Error::Invalid("invalid Git destination".into()));
            }
            // Usernames in SSH URLs are ordinary Git configuration; passwords and HTTP
            // userinfo belong in credential storage rather than recovery metadata.
            if let Some((scheme, rest)) = endpoint.split_once("://") {
                let authority = rest.split('/').next().unwrap_or("");
                if let Some((userinfo, _)) = authority.rsplit_once('@')
                    && (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") || userinfo.contains(':'))
                {
                    return Err(Error::Invalid("keep credentials out of Git URLs".into()));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Connection {
    pub id: String,
    pub name: String,
    /// Optional setup adapter; generic Git does not require a hosting provider.
    pub provider: String,
    pub repository: String,
    pub checkout: String,
    pub remote: String,
    pub branch: String,
    pub authentication: Authentication,
    pub ssh_identity: SshIdentity,
    /// An opaque host credential identifier, never the token itself.
    pub credential: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_remote: Option<GitRemote>,
}

impl Default for Connection {
    fn default() -> Self {
        Self {
            id: "github".into(),
            name: "GitHub".into(),
            provider: "github".into(),
            repository: String::new(),
            checkout: String::new(),
            remote: "origin".into(),
            branch: "main".into(),
            authentication: Authentication::Https,
            ssh_identity: SshIdentity::System,
            credential: "cloud:github".into(),
            git_remote: None,
        }
    }
}

impl Connection {
    /// Repository identity for display, derived from the verified remote rather than a user label.
    pub fn repository_label(&self) -> String {
        let remote = self.git_remote.as_ref().map(|r| r.fetch.as_str()).unwrap_or(&self.remote);
        if self.provider == "github" && !self.repository.is_empty() {
            return format!("github.com/{}", self.repository);
        }
        if let Some(path) = remote.strip_prefix("file://") {
            return path.into();
        }
        if let Some((_, address)) = remote.split_once("://") {
            let (host, path) = address.split_once('/').unwrap_or((address, ""));
            let host = host.rsplit('@').next().unwrap_or(host);
            let path = path.split(['?', '#']).next().unwrap_or_default().trim_end_matches('/').trim_end_matches(".git");
            return if path.is_empty() { host.into() } else { format!("{host}/{path}") };
        }
        if let Some((host, path)) = remote.split_once(':')
            && host.len() > 1
            && !host.contains(['/', '\\'])
        {
            return format!("{}/{}", host.rsplit('@').next().unwrap_or(host), path.trim_end_matches('/').trim_end_matches(".git"));
        }
        if remote.is_empty() || (remote == "origin" && self.git_remote.is_none()) {
            return self.checkout.clone();
        }
        remote.into()
    }

    /// Credentials may be renewed without redirecting captured saves to another repository.
    /// Legacy unpinned captures can acquire verified endpoints; captured pins stay immutable.
    pub fn same_repository(&self, other: &Self) -> bool {
        self.id == other.id
            && self.provider == other.provider
            && self.repository == other.repository
            && self.checkout == other.checkout
            && self.remote == other.remote
            && self.branch == other.branch
            && (self.git_remote.is_none() || self.git_remote == other.git_remote)
    }

    pub fn validate(&self) -> Result<()> {
        if let Some(remote) = &self.git_remote {
            remote.validate()?;
        }
        if self.id.is_empty() || self.id.len() > 128 || !self.id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')) {
            return Err(Error::Invalid("invalid connection identifier".into()));
        }
        for value in [&self.name, &self.repository, &self.checkout, &self.remote, &self.branch, &self.credential] {
            if value.len() > 4096 || value.chars().any(char::is_control) {
                return Err(Error::Invalid("invalid connection setting".into()));
            }
        }
        if self.remote.starts_with('-') || self.repository.starts_with('-') || self.branch.starts_with('-') {
            return Err(Error::Invalid("repository and branch must not start with a dash".into()));
        }
        if let SshIdentity::File(path) = &self.ssh_identity
            && (path.is_empty() || path.len() > 4096 || path.chars().any(char::is_control))
        {
            return Err(Error::Invalid("choose an SSH private key file".into()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub connections: Vec<Connection>,
    pub default_connection: Option<String>,
    pub sync_after_save: bool,
    pub retry_pending: bool,
    /// Empty means discover Git on PATH.
    pub git_executable: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self { connections: Vec::new(), default_connection: None, sync_after_save: true, retry_pending: true, git_executable: String::new() }
    }
}

impl Settings {
    /// Connections remain independent even when they share a hosting provider or account.
    pub fn validate(&self) -> Result<()> {
        if self.connections.len() > 128 {
            return Err(Error::Invalid("too many repository connections".into()));
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut credentials = std::collections::BTreeSet::new();
        for connection in &self.connections {
            connection.validate()?;
            if !ids.insert(&connection.id) {
                return Err(Error::Invalid("repository connections must have unique identifiers".into()));
            }
            if !connection.credential.is_empty() && !credentials.insert(&connection.credential) {
                return Err(Error::Invalid("repository connections must have independent credential identifiers".into()));
            }
        }
        if self.default_connection.as_ref().is_some_and(|id| self.connection(id).is_none()) {
            return Err(Error::Invalid("default repository connection does not exist".into()));
        }
        if self.git_executable.len() > 4096 || self.git_executable.chars().any(char::is_control) {
            return Err(Error::Invalid("invalid Git executable".into()));
        }
        Ok(())
    }

    pub fn connection(&self, id: &str) -> Option<&Connection> {
        self.connections.iter().find(|connection| connection.id == id)
    }

    pub fn destination(&self, requested: Option<&str>) -> Result<&Connection> {
        let id = requested.or(self.default_connection.as_deref()).ok_or_else(|| Error::Invalid("choose a repository to save into".into()))?;
        self.connection(id).ok_or_else(|| Error::Invalid("repository connection does not exist".into()))
    }

    /// Restore behavior preferences without removing connections or changing their default.
    pub fn restore_behavior_defaults(&mut self) {
        self.sync_after_save = true;
        self.retry_pending = true;
        self.git_executable.clear();
    }
}

/// Host setup is independent of Git access; another provider supplies the same small interface.
pub trait HostSetup {
    fn id(&self) -> &'static str;
    fn create_repository_url(&self) -> &'static str;
    fn create_token_url(&self) -> &'static str;
    fn matches_remote(&self, remote: &str) -> bool;
    fn ssh_settings_url(&self) -> &'static str;
}

struct GitHub;
impl HostSetup for GitHub {
    fn id(&self) -> &'static str {
        "github"
    }
    fn create_repository_url(&self) -> &'static str {
        "https://github.com/new?name=solidcraft-designs&visibility=private"
    }
    fn create_token_url(&self) -> &'static str {
        "https://github.com/settings/personal-access-tokens/new?name=SolidCraft&description=Save+SolidCraft+designs&contents=write&metadata=read&expires_in=none"
    }
    fn matches_remote(&self, remote: &str) -> bool {
        remote.starts_with("git@github.com:") || remote.starts_with("ssh://git@github.com/") || remote.starts_with("https://github.com/")
    }
    fn ssh_settings_url(&self) -> &'static str {
        "https://github.com/settings/ssh/new"
    }
}

/// Host-specific convenience actions are selected here, independently of Git access.
pub fn host_setup(provider: &str, remote: &str) -> Option<&'static dyn HostSetup> {
    [github_setup()].into_iter().find(|host| host.id() == provider || host.matches_remote(remote))
}

pub fn github_setup() -> &'static dyn HostSetup {
    &GitHub
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_need_no_ssh_key_and_preserve_old_settings() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(settings.sync_after_save && settings.retry_pending);
        assert!(settings.connections.is_empty());
        assert_eq!(Connection::default().ssh_identity, SshIdentity::System);
        let connection = Connection { provider: String::new(), repository: "/tmp/designs.git".into(), ..Default::default() };
        assert!(connection.validate().is_ok());
    }
    #[test]
    fn connection_values_never_become_command_options() {
        for repository in ["--upload-pack=bad", "repo\nother"] {
            assert!(Connection { repository: repository.into(), ..Default::default() }.validate().is_err());
        }
    }

    #[test]
    fn pinned_endpoints_reject_credentials_control_characters_and_excessive_fanout() {
        for url in [
            "https://secret@example.test/repo",
            "HTTPS://secret@example.test/repo",
            "ssh://git:secret@example.test/repo",
            "--upload-pack=bad",
            "repo\nother",
        ] {
            assert!(GitRemote { fetch: url.into(), push: vec![url.into()] }.validate().is_err());
        }
        for url in ["ssh://git@example.test/repo", "git@example.test:repo", "../path with spaces.git", "file:///path with spaces.git"] {
            GitRemote { fetch: url.into(), push: vec![url.into()] }.validate().unwrap();
        }
        assert!(GitRemote { fetch: "repo".into(), push: vec!["repo".into(); 17] }.validate().is_err());
    }

    #[test]
    fn multiple_repositories_keep_independent_credentials_and_destinations() {
        let first = Connection::default();
        let second = Connection {
            id: "work".into(),
            name: "Work designs".into(),
            repository: "company/designs".into(),
            credential: "cloud:work".into(),
            ..Default::default()
        };
        let mut settings = Settings { connections: vec![first, second], default_connection: Some("work".into()), ..Default::default() };
        settings.validate().unwrap();
        assert_eq!(settings.destination(None).unwrap().id, "work");
        assert_eq!(settings.destination(Some("github")).unwrap().id, "github");
        assert!(settings.destination(Some("unknown")).is_err());
        settings.restore_behavior_defaults();
        assert_eq!(settings.connections.len(), 2);
        assert_eq!(settings.default_connection.as_deref(), Some("work"));
        let restored: Settings = serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert_eq!(restored, settings);
        settings.connections[1].credential = "cloud:github".into();
        assert!(settings.validate().is_err());
        settings.connections[1].credential = "cloud:work".into();
        settings.connections[1].id = "github".into();
        assert!(settings.validate().is_err());
    }

    #[test]
    fn repository_labels_derive_from_remote_urls_and_local_paths() {
        assert_eq!(
            crate::setup::github_connection("fpoussin/solidcraft-designs").unwrap().repository_label(),
            "github.com/fpoussin/solidcraft-designs"
        );
        for (remote, expected) in [
            ("https://gitlab.com/team/designs.git", "gitlab.com/team/designs"),
            ("git@github.com:owner/designs.git", "github.com/owner/designs"),
            ("ssh://git@host.test:2222/team/designs.git", "host.test:2222/team/designs"),
            ("file:///path with spaces/designs.git", "/path with spaces/designs.git"),
            ("../designs.git", "../designs.git"),
            ("C:\\designs.git", "C:\\designs.git"),
        ] {
            let connection = Connection { name: "Old custom name".into(), provider: String::new(), remote: remote.into(), ..Default::default() };
            assert_eq!(connection.repository_label(), expected);
            let pinned =
                Connection { remote: "origin".into(), git_remote: Some(GitRemote { fetch: remote.into(), push: vec![remote.into()] }), ..connection };
            assert_eq!(pinned.repository_label(), expected);
        }
    }
}
