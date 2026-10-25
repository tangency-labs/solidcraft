//! Provider setup is separate from repository publication. Only opaque credential ids cross
//! the command boundary; platform hosts accept secrets through `Host::store_credential`.
use crate::{Authentication, Connection, Error, Result};

pub fn check_token(token: &str) -> Result<()> {
    if token.is_empty() || token.len() > 8192 || !token.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(Error::Invalid("invalid repository token".into()));
    }
    Ok(())
}

/// A stable identity for this repository, independent of UI labels and token renewals.
/// An empty override asks provider setup to discover the account's default repository.
pub fn github_connection(repository: &str) -> Result<Connection> {
    if repository.trim().is_empty() {
        return Ok(Connection {
            id: "github-automatic".into(),
            name: "solidcraft-designs".into(),
            repository: String::new(),
            remote: String::new(),
            credential: "cloud:github-automatic".into(),
            authentication: Authentication::Https,
            ..Default::default()
        });
    }
    // This parser is also available without the optional HTTP adapter.
    let repository = github_repository(repository)?;
    let id = format!("github-{}", crate::digest(repository.to_ascii_lowercase().as_bytes()));
    Ok(Connection {
        credential: format!("cloud:{id}"),
        id,
        name: format!("github.com/{repository}"),
        remote: format!("https://github.com/{repository}.git"),
        repository,
        authentication: Authentication::Https,
        ..Default::default()
    })
}

pub fn github_repository(value: &str) -> Result<String> {
    let value = value.trim().strip_prefix("https://github.com/").unwrap_or(value.trim()).trim_end_matches('/');
    let value = value.strip_suffix(".git").unwrap_or(value);
    let Some((owner, name)) = value.split_once('/') else {
        return Err(Error::Invalid("enter a GitHub repository as owner/name".into()));
    };
    if owner.is_empty()
        || name.is_empty()
        || owner.len() > 100
        || name.len() > 100
        || !owner.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        || !name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        || matches!(name, "." | "..")
    {
        return Err(Error::Invalid("enter a GitHub repository as owner/name".into()));
    }
    Ok(value.into())
}

/// GitHub cannot preselect repositories through a token URL. The wizard explicitly asks
/// for "Only select repositories" and the chosen repository instead of requesting all.
pub fn github_token_url(repository: &str) -> Result<String> {
    if repository.trim().is_empty() {
        return Ok(crate::github_setup().create_token_url().into());
    }
    let repository = github_repository(repository)?;
    let owner = repository.split('/').next().ok_or_else(|| Error::Invalid("missing owner".into()))?;
    Ok(format!("{}&target_name={owner}", crate::github_setup().create_token_url()))
}

pub fn github_create_repository_url(repository: &str) -> Result<String> {
    if repository.trim().is_empty() {
        return Ok(crate::github_setup().create_repository_url().into());
    }
    let repository = github_repository(repository)?;
    let name = repository.rsplit('/').next().ok_or_else(|| Error::Invalid("missing repository name".into()))?;
    Ok(format!("https://github.com/new?name={name}&visibility=private"))
}

pub fn github_website(connection: &Connection) -> Result<String> {
    let candidate = if connection.provider == "github" {
        connection.repository.as_str()
    } else {
        let remote = connection.git_remote.as_ref().map(|r| r.fetch.as_str()).unwrap_or(&connection.remote);
        if !crate::github_setup().matches_remote(remote) {
            return Err(Error::Invalid("this connection has no GitHub website".into()));
        }
        remote.strip_prefix("git@github.com:").or_else(|| remote.strip_prefix("ssh://git@github.com/")).unwrap_or(remote)
    };
    let repository = github_repository(candidate)?;
    Ok(format!("https://github.com/{repository}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn additional_repository_links_and_websites_keep_connections_separate() {
        let first = github_connection("Owner/one").unwrap();
        let second = github_connection("Owner/two").unwrap();
        assert_ne!(first.id, second.id);
        assert_ne!(first.credential, second.credential);
        assert!(github_create_repository_url(&second.repository).unwrap().contains("name=two&visibility=private"));
        assert_eq!(github_website(&second).unwrap(), "https://github.com/Owner/two");
        let generic = Connection { provider: String::new(), remote: "git@github.com:Owner/two.git".into(), ..first.clone() };
        assert_eq!(github_website(&generic).unwrap(), "https://github.com/Owner/two");
        for remote in ["local/folder", "file:///tmp/repo", "https://evil.example/Owner/two"] {
            assert!(github_website(&Connection { provider: String::new(), remote: remote.into(), ..first.clone() }).is_err());
        }
    }

    #[test]
    fn setup_ids_are_isolated_and_links_never_accept_arbitrary_urls() {
        let a = github_connection("https://github.com/Owner/designs.git").unwrap();
        assert_eq!(a, github_connection("Owner/designs").unwrap());
        assert_eq!(a.id, github_connection("owner/DESIGNS").unwrap().id);
        assert_ne!(a.credential, github_connection("Owner/other").unwrap().credential);
        assert!(github_token_url("owner/designs").unwrap().ends_with("target_name=owner"));
        for bad in ["https://evil.test/a/b", "user/repo?token=secret", "user/repo/extra", "../repo", "user/.."] {
            assert!(github_connection(bad).is_err());
        }
        assert!(check_token("bad\nsecret").is_err());
    }

    #[test]
    fn github_token_template_prefills_minimum_permissions_without_expiration() {
        let automatic = github_connection("").unwrap();
        automatic.validate().unwrap();
        assert!(automatic.repository.is_empty());
        assert!(automatic.remote.is_empty());
        assert!(!github_token_url("").unwrap().contains("target_name="), "GitHub selects the signed-in account");
        let url = github_token_url("https://github.com/Example/solidcraft-designs.git").unwrap();
        let (page, query) = url.split_once('?').unwrap();
        assert_eq!(page, "https://github.com/settings/personal-access-tokens/new");
        let parameters: std::collections::BTreeMap<_, _> = query.split('&').map(|p| p.split_once('=').unwrap()).collect();
        assert_eq!(parameters.get("name"), Some(&"SolidCraft"));
        assert_eq!(parameters.get("description"), Some(&"Save+SolidCraft+designs"));
        assert_eq!(parameters.get("target_name"), Some(&"Example"));
        assert_eq!(parameters.get("expires_in"), Some(&"none"));
        assert_eq!(parameters.get("contents"), Some(&"write"));
        assert_eq!(parameters.get("metadata"), Some(&"read"));
        assert_eq!(parameters.len(), 6, "setup must not silently request extra permissions");
    }
}
