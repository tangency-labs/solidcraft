//! Explicit setup owns a private checkout; ordinary Open never clones or writes a repo.
use super::GitRepository;
use solidcraft_cloud::{Connection, Error, Result, jobs::Job};
use std::{fs, path::Path};

pub(super) fn checkout(mut connection: Connection, executable: &str, root: &Path, token: Option<String>, job: &Job) -> Result<Connection> {
    let managed = root.join("checkout");
    crate::vfs::check(&managed.to_string_lossy()).map_err(|_| Error::Storage("checkout is outside the allowed folders".into()))?;
    if !connection.checkout.is_empty() && Path::new(&connection.checkout) != managed {
        return Err(Error::Invalid("guided setup uses an app-managed checkout; use advanced settings for an existing checkout".into()));
    }
    connection.checkout = managed.to_string_lossy().into_owned();
    if managed.exists() {
        if fs::symlink_metadata(&managed).map_err(storage)?.file_type().is_symlink() {
            return Err(storage("managed checkout must not be a symbolic link"));
        }
        let repo = GitRepository::authenticated(connection.clone(), executable, job.clone(), token)?;
        // Refuse unrelated/interrupted folders instead of deleting them or adopting a repo.
        if repo.setup_text(&["remote", "get-url", "--all", "origin"])? != connection.remote {
            return Err(storage("managed checkout has a different origin; choose another connection"));
        }
        repo.test_connection()?;
        return Ok(connection);
    }
    let temporary = tempfile::Builder::new().prefix("cloud-clone-").tempdir_in(root).map_err(storage)?;
    let target = temporary.path().join("checkout");
    let mut command = std::process::Command::new(if executable.is_empty() { "git" } else { executable });
    command.current_dir(root);
    if let Some(token) = &token {
        super::auth::apply(&mut command, &connection, token)?;
    }
    command.args(["clone", "--single-branch", "--branch", &connection.branch, "--", &connection.remote]).arg(&target);
    super::process::run(command, &[], || job.cancelled().unwrap_or(true), std::time::Duration::from_secs(300), 1024 * 1024)
        .map_err(|_| storage("Git clone failed; check connectivity, Git installation and token permissions, then retry"))?;
    // Validate before installing. Renaming only our successful temporary clone is atomic.
    let mut candidate = connection.clone();
    candidate.checkout = target.to_string_lossy().into_owned();
    GitRepository::authenticated(candidate, executable, job.clone(), token)?.test_connection()?;
    fs::rename(&target, &managed).map_err(storage)?;
    Ok(connection)
}

fn storage(message: impl std::fmt::Display) -> Error {
    Error::Storage(message.to_string())
}
