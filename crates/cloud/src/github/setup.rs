//! Guided GitHub setup, shared by desktop and browser. Octocrab owns API encoding/models.
use super::*;

/// One read-only metadata request. GitHub deliberately uses 404 for both missing
/// repositories and private repositories the supplied credential cannot access.
pub async fn exists(client: Octocrab, repository: &str) -> Result<bool> {
    let repository = repository_name(repository)?;
    let (owner, name) = repository.split_once('/').ok_or_else(|| invalid("invalid repository"))?;
    match client.repos(owner, name).get().await {
        Ok(info) => Ok(info.full_name.as_deref().is_some_and(|name| name.eq_ignore_ascii_case(&repository))),
        Err(octocrab::Error::GitHub { source, .. }) if source.status_code.as_u16() == 404 => Ok(false),
        Err(error) => Err(api_error(error)),
    }
}

/// Inspect the chosen repository and initialize only an empty repository. A failed/repeated
/// request never overwrites an existing README or branch. Setup is explicit, never an Open.
pub async fn prepare(client: Octocrab, mut connection: Connection) -> Result<Connection> {
    connection.validate()?;
    if connection.provider != "github" {
        return Err(invalid("unsupported setup provider"));
    }
    if connection.repository.is_empty() {
        let account = client.current().user().await.map_err(api_error)?;
        let discovered = crate::setup::github_connection(&format!("{}/solidcraft-designs", account.login))?;
        connection = Connection { credential: connection.credential, ..discovered };
    }
    let name = repository_name(&connection.repository)?;
    let (owner, repo) = name.split_once('/').ok_or_else(|| invalid("invalid repository"))?;
    let info = client.repos(owner, repo).get().await.map_err(api_error)?;
    if info.archived == Some(true) || info.disabled == Some(true) || info.permissions.as_ref().is_some_and(|p| !p.push) {
        return Err(storage("this GitHub repository does not allow pushes; choose a writable repository"));
    }
    // Do not follow renamed/transferred repositories silently: captured destinations must
    // continue to identify the repository the user selected.
    if info.full_name.as_deref().is_none_or(|canonical| !canonical.eq_ignore_ascii_case(&name)) {
        return Err(storage("repository moved; reconnect using its current owner/name"));
    }
    connection.repository = name.clone();
    connection.remote = format!("https://github.com/{name}.git");
    connection.branch = info.default_branch.filter(|s| !s.is_empty()).unwrap_or_else(|| "main".into());
    GitHub::new(client.clone(), &connection)?;
    if info.size == Some(0) {
        // Size alone is not proof of an empty repository (GitHub's statistics can lag).
        let refs = client.git(owner, repo).get_ref(&Reference::Branch(connection.branch.clone())).await;
        match refs {
            Ok(_) => {}
            Err(octocrab::Error::GitHub { source, .. }) if matches!(source.status_code.as_u16(), 404 | 409) => {
                let created = client
                    .repos(owner, repo)
                    .create_file("README.md", "Initialize SolidCraft cloud storage", b"# SolidCraft designs\n\nProjects saved with SolidCraft.\n")
                    .branch(&connection.branch)
                    .send()
                    .await;
                if let Err(error) = created {
                    // A concurrent setup can have won. Accept only a now-readable branch.
                    client.git(owner, repo).get_ref(&Reference::Branch(connection.branch.clone())).await.map_err(|_| api_error(error))?;
                }
            }
            Err(error) => return Err(api_error(error)),
        }
    }
    let adapter = GitHub::new(client.clone(), &connection)?;
    adapter.test_connection().await?;
    // A no-op, non-forced ref update checks the token's actual write permission. Account
    // permissions in GET /repos can say push=true even for a Contents read-only token.
    // Reusing the observed SHA creates no revision; a concurrent advance refuses the update.
    let git = client.git(owner, repo);
    let reference = Reference::Branch(connection.branch.clone());
    let current = git.get_ref(&reference).await.map_err(api_error)?;
    let Object::Commit { sha, .. } = current.object else {
        return Err(invalid("branch does not refer to a commit"));
    };
    check_sha(&sha)?;
    git.update_ref(&reference, &sha).force(false).send().await.map_err(api_error)?;
    Ok(connection)
}
