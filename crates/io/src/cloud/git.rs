//! Standard Git adapter. Commands receive separate arguments and literal pathspecs. The app
//! never changes global configuration, resets a checkout, stashes work, or force-pushes.
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub use solidcraft_cloud::Published;
use solidcraft_cloud::{Connection, Destination, Error, Manifest, PROJECT_MARKER, PendingSave, Publication, Result, Snapshot, SshIdentity};

mod remotes;

fn storage(error: impl std::fmt::Display) -> Error {
    Error::Storage(error.to_string())
}

pub struct GitRepository {
    checkout: PathBuf,
    executable: OsString,
    connection: Connection,
    job: Option<solidcraft_cloud::jobs::Job>,
    token: Option<String>,
}

impl GitRepository {
    /// Opening an existing checkout is read-only. Creation/cloning belongs to explicit setup.
    pub fn open(connection: Connection, executable: &str) -> Result<Self> {
        Self::open_inner(connection, executable, None, None)
    }

    pub fn open_cancellable(connection: Connection, executable: &str, job: solidcraft_cloud::jobs::Job) -> Result<Self> {
        Self::open_inner(connection, executable, Some(job), None)
    }

    pub(super) fn authenticated(connection: Connection, executable: &str, job: solidcraft_cloud::jobs::Job, token: Option<String>) -> Result<Self> {
        Self::open_inner(connection, executable, Some(job), token)
    }

    fn open_inner(connection: Connection, executable: &str, job: Option<solidcraft_cloud::jobs::Job>, token: Option<String>) -> Result<Self> {
        connection.validate()?;
        if connection.checkout.is_empty() {
            return Err(storage("choose a checkout location"));
        }
        crate::vfs::check(&connection.checkout).map_err(storage)?;
        let checkout = fs::canonicalize(&connection.checkout).map_err(storage)?;
        let repository = Self { checkout, executable: if executable.is_empty() { "git".into() } else { executable.into() }, connection, job, token };
        let root = repository.text(&["rev-parse", "--show-toplevel"])?;
        if fs::canonicalize(root.trim()).map_err(storage)? != repository.checkout {
            return Err(storage("checkout location must be the repository's top-level folder"));
        }
        if repository.token.is_some() {
            repository.check_token_destination()?;
        }
        repository.check_ready()?;
        Ok(repository)
    }

    fn check_token_destination(&self) -> Result<()> {
        let repository = solidcraft_cloud::setup::github_repository(&self.connection.repository)?;
        let expected = format!("https://github.com/{repository}.git");
        let remote = self.remote_identity()?.ok_or_else(|| storage("choose this GitHub repository's remote"))?;
        if remote.fetch != expected || remote.push.iter().any(|url| url != &expected) {
            return Err(storage("Git URL rewriting changes this GitHub destination; verify the remote in Cloud Saves settings"));
        }
        Ok(())
    }

    fn command(&self) -> Result<Command> {
        let mut command = Command::new(&self.executable);
        command.current_dir(&self.checkout).arg("--literal-pathspecs");
        if let SshIdentity::File(key) = &self.connection.ssh_identity {
            crate::vfs::check(key).map_err(storage)?;
            // Git uses its shell for this override, including Git for Windows. Reuse a shell
            // quoting library instead of maintaining another quoting implementation.
            #[cfg(windows)]
            let key = key.replace('\\', "/");
            let ssh = shlex::try_join(["ssh", "-i", key.as_str(), "-o", "IdentitiesOnly=yes"]).map_err(storage)?;
            command.env("GIT_SSH_COMMAND", ssh);
        }
        if let Some(token) = &self.token {
            super::auth::apply(&mut command, &self.connection, token)?;
        }
        Ok(command)
    }

    fn run(&self, args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>> {
        let mut command = self.command()?;
        command.args(args);
        super::process::run(
            command,
            input.unwrap_or_default(),
            || self.job.as_ref().is_some_and(|job| job.cancelled().unwrap_or(true)),
            std::time::Duration::from_secs(300),
            solidcraft_cloud::MAX_MANIFEST_BYTES * 2,
        )
        .map_err(|error| storage(format!("Git {}: {error}; check the connection and checkout state", args.first().copied().unwrap_or("command"))))
    }

    fn lock(&self) -> Result<fs::File> {
        let path = self.checkout.join(self.text(&["rev-parse", "--git-path", "solidcraft-cloud.lock"])?);
        let lock = fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(path).map_err(storage)?;
        lock.try_lock().map_err(|_| storage("another SolidCraft process is synchronizing this checkout; try again shortly"))?;
        Ok(lock)
    }

    pub(super) fn setup_text(&self, args: &[&str]) -> Result<String> {
        self.text(args)
    }

    fn text(&self, args: &[&str]) -> Result<String> {
        String::from_utf8(self.run(args, None)?).map(|s| s.trim().to_owned()).map_err(storage)
    }

    fn path(&self, relative: &str) -> Result<PathBuf> {
        solidcraft_cloud::check_path(relative)?;
        let mut path = self.checkout.clone();
        for segment in relative.split('/') {
            path.push(segment);
            if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
                return Err(storage("project paths must not be symbolic links"));
            }
        }
        crate::vfs::check(&path.to_string_lossy()).map_err(storage)?;
        Ok(path)
    }

    fn read(&self, relative: &str) -> Result<Option<Vec<u8>>> {
        let path = self.path(relative)?;
        if !path.exists() {
            return Ok(None);
        }
        crate::vfs::read(&path.to_string_lossy(), solidcraft_cloud::MAX_MANIFEST_BYTES).map(Some).map_err(storage)
    }

    fn committed(&self, commit: &str, relative: &str) -> Result<Option<Vec<u8>>> {
        solidcraft_cloud::check_path(relative)?;
        let specification = format!("{commit}:{relative}");
        let exists = self.run(&["ls-tree", "-z", commit, "--", relative], None)?;
        if exists.is_empty() {
            return Ok(None);
        }
        self.run(&["show", &specification], None).map(Some)
    }

    fn check_ready(&self) -> Result<()> {
        for name in ["MERGE_HEAD", "CHERRY_PICK_HEAD", "REVERT_HEAD", "rebase-merge", "rebase-apply", "BISECT_LOG", "index.lock"] {
            let path = PathBuf::from(self.text(&["rev-parse", "--git-path", name])?);
            if self.checkout.join(path).exists() {
                return Err(storage("finish the external Git operation before synchronizing"));
            }
        }
        let branch = self.text(&["symbolic-ref", "--short", "HEAD"])?;
        if branch != self.connection.branch {
            return Err(storage(format!("checkout is on {branch}; select that branch in Cloud Saves settings or switch it in Git")));
        }
        self.run(&["check-ref-format", "--branch", &self.connection.branch], None)?;
        Ok(())
    }

    pub fn test_connection(&self) -> Result<()> {
        self.check_ready()?;
        if self.token.is_some() {
            self.check_token_destination()?;
        }
        if let Some(remote) = self.pinned_remote()? {
            for url in std::iter::once(&remote.fetch).chain(&remote.push) {
                self.run(&["ls-remote", "--", url], None)?;
            }
        }
        Ok(())
    }

    pub fn open_design(&self, destination: &Destination) -> Result<(solidcraft_doc::Document, Manifest)> {
        let snapshot = self.snapshot(destination)?;
        let opened = super::open_document(&snapshot.manifest, &snapshot.document, |path| {
            snapshot.objects.get(path).cloned().ok_or_else(|| storage("asset not found"))
        })?;
        Ok((opened, snapshot.manifest))
    }

    pub fn snapshot(&self, destination: &Destination) -> Result<Snapshot> {
        if destination.connection != self.connection.id {
            return Err(storage("repository connection mismatch"));
        }
        let prefix = destination.design_path()?;
        let marker = self.read(&format!("{}/{PROJECT_MARKER}", destination.project))?.ok_or_else(|| storage("project marker not found"))?;
        solidcraft_cloud::Project::read(&marker)?;
        let manifest = Manifest::read(&self.read(&format!("{prefix}/manifest.json"))?.ok_or_else(|| storage("design manifest not found"))?)?;
        let document = self.read(&format!("{prefix}/document.json"))?.ok_or_else(|| storage("document not found"))?;
        super::check_document(&document)?;
        let mut objects = std::collections::BTreeMap::new();
        Snapshot::hydrate(&manifest, &document, |path| {
            let bytes = self.read(&format!("{}/{path}", destination.project))?.ok_or_else(|| storage("asset not found"))?;
            objects.insert(path.into(), bytes.clone());
            Ok(bytes)
        })?;
        Ok(Snapshot { manifest, document, objects })
    }

    pub fn list(&self) -> Result<Vec<Destination>> {
        let files = self.run(&["ls-files", "-z"], None)?;
        let files: Vec<&str> = files
            .split(|b| *b == 0)
            .filter(|bytes| !bytes.is_empty())
            .map(std::str::from_utf8)
            .collect::<std::result::Result<_, _>>()
            .map_err(storage)?;
        if files.len() > 100_000 {
            return Err(storage("repository file listing is too large"));
        }
        let mut projects = Vec::new();
        for path in &files {
            if let Some(project) = path.strip_suffix(&format!("/{PROJECT_MARKER}")) {
                solidcraft_cloud::check_path(project)?;
                let marker = self.read(path)?.ok_or_else(|| storage("project marker missing from checkout"))?;
                solidcraft_cloud::Project::read(&marker)?;
                projects.push(project);
            }
        }
        let mut designs = Vec::new();
        for path in files {
            if let Some(prefix) = path.strip_suffix("/manifest.json")
                && let Some(project) = projects
                    .iter()
                    .filter(|project| prefix.strip_prefix(**project).is_some_and(|rest| rest.starts_with('/')))
                    .max_by_key(|project| project.len())
            {
                let file = prefix.strip_prefix(&format!("{project}/")).ok_or_else(|| storage("invalid design path"))?;
                let destination = Destination { connection: self.connection.id.clone(), project: (*project).into(), file: file.into() };
                destination.validate()?;
                designs.push(destination);
            }
        }
        Ok(designs)
    }

    /// The durable outbox remains authoritative until this method succeeds.
    pub fn publish(&self, save: &PendingSave, snapshot: Snapshot) -> Result<Published> {
        save.check_connection(&self.connection)?;
        if save.destination.connection != self.connection.id {
            return Err(storage("repository connection mismatch"));
        }
        let _lock = self.lock()?;
        self.check_ready()?;
        if self.token.is_some() {
            self.check_token_destination()?;
        }
        super::check_document(&snapshot.document)?;
        let remote = self.pinned_remote()?;
        if let Some(remote) = &remote {
            self.fetch_destinations(remote)?;
        }
        let base = self.text(&["rev-parse", "HEAD"])?;
        let prefix = save.destination.design_path()?;
        let marker_path = format!("{}/{PROJECT_MARKER}", save.destination.project);
        if let Some(bytes) = self.read(&marker_path)? {
            solidcraft_cloud::Project::read(&bytes)?;
        }
        let marker = self.committed(&base, &marker_path)?;
        let manifest_path = format!("{prefix}/manifest.json");
        // Reject newer working-copy formats even if that file has not been committed yet.
        if let Some(bytes) = self.read(&manifest_path)? {
            Manifest::read(&bytes)?;
        }
        if let Some(bytes) = self.read(&format!("{prefix}/document.json"))? {
            super::check_document(&bytes)?;
        }
        let existing = self.committed(&base, &manifest_path)?;
        let publication = Publication::prepare(save, snapshot, marker.as_deref(), existing.as_deref())?;
        let published_path = format!("{}/manifest.json", publication.destination.design_path()?);
        let existing_conflict =
            if publication.conflict { self.committed(&base, &published_path)?.map(|bytes| Manifest::read(&bytes)).transpose()? } else { None };
        if existing_conflict.as_ref().is_some_and(|manifest| solidcraft_cloud::revision(manifest) != Some(save.id.as_str())) {
            return Err(Error::Conflict);
        }
        if !publication.already_published && existing_conflict.is_none() {
            let mut files = publication.files;
            // Already-present immutable objects are neither rewritten nor staged.
            let object_prefix = format!("{}/objects/", publication.destination.project);
            let mut reusable = Vec::new();
            for (path, bytes) in &files {
                if path.starts_with(&object_prefix)
                    && self.read(path)?.as_deref() == Some(bytes.as_slice())
                    && self.committed(&base, path)?.as_deref() == Some(bytes.as_slice())
                {
                    reusable.push(path.clone());
                }
            }
            for path in reusable {
                files.remove(&path);
            }
            let mut paths = Vec::new();
            for (relative, bytes) in &files {
                let status = self.run(&["status", "--porcelain=v1", "-z", "--", relative], None)?;
                if !status.is_empty() {
                    // Resume our interrupted write only when working and staged bytes are
                    // identical to this captured snapshot (or the index still equals HEAD).
                    let working = self.read(relative)?;
                    let index_entry = self.run(&["ls-files", "--stage", "-z", "--", relative], None)?;
                    let index = if index_entry.is_empty() { None } else { Some(self.run(&["show", &format!(":{relative}")], None)?) };
                    let head = self.committed(&base, relative)?;
                    if working.as_deref() != Some(bytes.as_slice()) || (index != head && index.as_deref() != Some(bytes.as_slice())) {
                        return Err(storage(format!("{relative} has local changes; preserve or commit them before synchronizing")));
                    }
                }
                if self.path(relative)?.exists()
                    && self.committed(&base, relative)?.is_none()
                    && self.read(relative)?.as_deref() != Some(bytes.as_slice())
                {
                    return Err(storage(format!("{relative} already exists outside Git; preserve it before synchronizing")));
                }
                paths.extend_from_slice(relative.as_bytes());
                paths.push(0);
                // Validate all paths before changing any of them.
                if bytes.len() > solidcraft_cloud::MAX_MANIFEST_BYTES {
                    return Err(storage("project file too large"));
                }
            }
            if self.text(&["rev-parse", "HEAD"])? != base {
                return Err(Error::Conflict);
            }
            for (relative, bytes) in &files {
                let path = self.path(relative)?;
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(storage)?;
                }
                crate::safe_write::write_atomic(&path, bytes, false).map_err(storage)?;
            }
            self.run(&["add", "--pathspec-from-file=-", "--pathspec-file-nul"], Some(&paths))?;
            let mut identity = Vec::new();
            if !self.text(&["config", "--get", "user.name"]).is_ok_and(|value| !value.is_empty()) {
                identity.extend(["-c", "user.name=SolidCraft"]);
            }
            if !self.text(&["config", "--get", "user.email"]).is_ok_and(|value| !value.is_empty()) {
                identity.extend(["-c", "user.email=cloud@solidcraft.invalid"]);
            }
            let message = format!("Save {} ({})", save.destination.file, save.id);
            let commit_args = ["commit", "--only", "--pathspec-from-file=-", "--pathspec-file-nul", "-m", &message];
            identity.extend(commit_args);
            self.run(&identity, Some(&paths))?;
        }
        // Find the commit that captured this revision instead of pushing later unrelated edits.
        let commit = self.text(&["log", "-1", "--format=%H", "--", &published_path])?;
        let confirmed = self.committed(&commit, &published_path)?.ok_or_else(|| storage("saved revision is missing from its commit"))?;
        if solidcraft_cloud::revision(&Manifest::read(&confirmed)?) != Some(save.id.as_str()) {
            return Err(Error::Conflict);
        }
        if let Some(remote) = &remote {
            self.push_destinations(remote, &commit)?;
        }
        Ok(Published { destination: publication.destination, revision: save.id.clone(), commit, conflict: publication.conflict })
    }

    pub fn checkout(&self) -> &Path {
        &self.checkout
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    struct Fixture {
        root: PathBuf,
        checkout: PathBuf,
    }
    impl Fixture {
        fn git(path: &Path, args: &[&str]) -> Vec<u8> {
            let output = Command::new("git").current_dir(path).args(args).output().unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            output.stdout
        }
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "solidcraft cloud git {} {}",
                std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
            ));
            fs::create_dir_all(&root).unwrap();
            Self::git(&root, &["init", "--bare", "--initial-branch=main", "remote.git"]);
            Self::git(&root, &["clone", "remote.git", "checkout"]);
            let checkout = root.join("checkout");
            Self::git(&checkout, &["config", "user.name", "Cloud tests"]);
            Self::git(&checkout, &["config", "user.email", "cloud@example.invalid"]);
            Self::git(&checkout, &["config", "commit.gpgSign", "false"]);
            fs::write(checkout.join("README.md"), "Project repository\n").unwrap();
            Self::git(&checkout, &["add", "README.md"]);
            Self::git(&checkout, &["commit", "-m", "Initial repository"]);
            Self::git(&checkout, &["push", "origin", "main"]);
            Self { root, checkout }
        }
        fn connection(&self) -> Connection {
            Connection { checkout: self.checkout.to_string_lossy().into_owned(), ..Default::default() }
        }
        fn stage(&self) -> solidcraft_cloud::StagedSave {
            let doc = solidcraft_doc::Document::new("Bracket");
            PendingSave::stage(&self.connection(), "Robot project", "parts/bracket", None, super::super::prepare_document(&doc).unwrap()).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn checkout_lock_prevents_overlapping_publication() {
        let fixture = Fixture::new();
        let repository = GitRepository::open(fixture.connection(), "").unwrap();
        let held = repository.lock().unwrap();
        let save = fixture.stage();
        assert!(repository.publish(&save.record, save.record.recover(|path| Ok(save.objects[path].clone())).unwrap()).is_err());
        assert!(!fixture.checkout.join("Robot project").exists());
        drop(held);
        repository.publish(&save.record, save.record.recover(|path| Ok(save.objects[path].clone())).unwrap()).unwrap();
    }

    #[test]
    fn native_git_publishes_exact_paths_and_preserves_unrelated_work() {
        let fixture = Fixture::new();
        fs::write(fixture.checkout.join("staged.txt"), b"unrelated staged").unwrap();
        Fixture::git(&fixture.checkout, &["add", "staged.txt"]);
        fs::write(fixture.checkout.join("README.md"), b"unrelated unstaged").unwrap();
        let repository = GitRepository::open(fixture.connection(), "").unwrap();
        let staged = fixture.stage();
        let published = repository.publish(&staged.record, staged.record.recover(|path| Ok(staged.objects[path].clone())).unwrap()).unwrap();
        assert!(!published.conflict);
        assert_eq!(Fixture::git(&fixture.checkout, &["diff", "--cached", "--name-only"]), b"staged.txt\n");
        assert_eq!(fs::read(fixture.checkout.join("README.md")).unwrap(), b"unrelated unstaged");
        assert!(Fixture::git(&fixture.checkout, &["ls-tree", "HEAD", "--", "staged.txt"]).is_empty());
        let reopened = repository.open_design(&published.destination).unwrap().0;
        assert_eq!(reopened.name, "Bracket");
        let again = repository.publish(&staged.record, staged.record.recover(|path| Ok(staged.objects[path].clone())).unwrap()).unwrap();
        assert_eq!(again.commit, published.commit);
        Fixture::git(&fixture.root, &["clone", "remote.git", "other checkout"]);
        let other = GitRepository::open(
            Connection { checkout: fixture.root.join("other checkout").to_string_lossy().into_owned(), ..Default::default() },
            "",
        )
        .unwrap();
        assert_eq!(other.open_design(&published.destination).unwrap().0.name, "Bracket");
    }

    #[test]
    fn failed_publication_keeps_the_durable_local_snapshot_and_is_connection_scoped() {
        let fixture = Fixture::new();
        let save = fixture.stage();
        let outbox = super::super::LocalOutbox::new(fixture.root.join("recovery")).unwrap();
        outbox.stage(&save).unwrap();
        let mut connection = fixture.connection();
        connection.remote = fixture.root.join("unavailable remote.git").to_string_lossy().into_owned();
        let failed = GitRepository::open(connection, "").unwrap().publish(&save.record, outbox.recover(&save.record).unwrap());
        assert!(failed.is_err());
        assert_eq!(outbox.pending().unwrap().len(), 1);
        assert_eq!(
            super::super::open_document(&save.record.manifest, &outbox.recover(&save.record).unwrap().document, |_| Err(storage(
                "no assets expected"
            )))
            .unwrap()
            .name,
            "Bracket"
        );
        let good = GitRepository::open(fixture.connection(), "").unwrap();
        let published = good.publish(&save.record, outbox.recover(&save.record).unwrap()).unwrap();
        outbox.published(&save.record, &published).unwrap();
        assert!(outbox.pending().unwrap().is_empty());
        let other = Connection { id: "other".into(), credential: "cloud:other".into(), ..fixture.connection() };
        assert!(GitRepository::open(other, "").unwrap().publish(&save.record, Snapshot::prepare(b"{}".to_vec(), BTreeMap::new()).unwrap()).is_err());
    }

    #[test]
    #[cfg(unix)]
    fn custom_ssh_paths_are_quoted_and_system_auth_is_untouched() {
        let fixture = Fixture::new();
        let system = GitRepository::open(fixture.connection(), "").unwrap();
        assert!(system.command().unwrap().get_envs().all(|(name, _)| name != "GIT_SSH_COMMAND"));
        let key = fixture.root.join("someone's key;$(echo bad)");
        let chosen =
            GitRepository::open(Connection { ssh_identity: SshIdentity::File(key.to_string_lossy().into_owned()), ..fixture.connection() }, "")
                .unwrap();
        let command = chosen.command().unwrap();
        let ssh = command.get_envs().find(|(name, _)| *name == "GIT_SSH_COMMAND").unwrap().1.unwrap().to_string_lossy();
        let parsed = Command::new("sh").args(["-c", &format!("set -- {ssh}; printf '%s\\0' \"$@\"")]).output().unwrap();
        assert!(parsed.status.success());
        let parts: Vec<_> = parsed.stdout.split(|b| *b == 0).filter(|s| !s.is_empty()).collect();
        assert_eq!(parts, [b"ssh".as_slice(), b"-i", key.to_str().unwrap().as_bytes(), b"-o", b"IdentitiesOnly=yes"]);
    }
    #[test]
    fn guided_https_refuses_git_url_rewrites_before_network_access() {
        let fixture = Fixture::new();
        let mut connection = solidcraft_cloud::setup::github_connection("owner/designs").unwrap();
        connection.checkout = fixture.checkout.to_string_lossy().into_owned();
        let job = solidcraft_cloud::jobs::Job::new(connection.id.clone()).unwrap();
        // A named origin is irrelevant: the guided connection uses an explicit destination.
        let safe = GitRepository::authenticated(connection.clone(), "", job.clone(), Some("test-token".into())).unwrap();
        assert_eq!(safe.setup_text(&["ls-remote", "--get-url", &connection.remote]).unwrap(), connection.remote);
        Fixture::git(&fixture.checkout, &["config", "remote.guided.url", &connection.remote]);
        let named = Connection { remote: "guided".into(), ..connection.clone() };
        GitRepository::authenticated(named.clone(), "", job.clone(), Some("test-token".into())).unwrap();
        Fixture::git(&fixture.checkout, &["config", "remote.guided.pushurl", "ssh://evil.test/other.git"]);
        assert!(GitRepository::authenticated(named, "", job.clone(), Some("test-token".into())).is_err());
        Fixture::git(&fixture.checkout, &["config", "url.ssh://evil.test/.pushInsteadOf", "https://github.com/"]);
        assert!(GitRepository::authenticated(connection.clone(), "", job.clone(), Some("test-token".into())).is_err());
        Fixture::git(&fixture.checkout, &["config", "--unset-all", "url.ssh://evil.test/.pushInsteadOf"]);
        Fixture::git(&fixture.checkout, &["config", "url.ssh://evil.test/.insteadOf", "https://github.com/"]);
        assert!(GitRepository::authenticated(connection, "", job, Some("test-token".into())).is_err());
    }

    #[test]
    fn named_remotes_confirm_every_push_destination_and_pin_their_configuration() {
        let fixture = Fixture::new();
        let generic = Connection { id: "git".into(), provider: String::new(), credential: String::new(), ..fixture.connection() };
        let verified = GitRepository::open(generic.clone(), "").unwrap().verified_connection().unwrap();
        assert!(verified.id.starts_with("git-"));
        assert_eq!(GitRepository::open(generic, "").unwrap().verified_connection().unwrap(), verified);
        Fixture::git(&fixture.root, &["clone", "--bare", "remote.git", "mirror.git"]);
        let first = fixture.root.join("remote.git").to_string_lossy().into_owned();
        let second = fixture.root.join("mirror.git").to_string_lossy().into_owned();
        for url in [&first, &second] {
            Fixture::git(&fixture.checkout, &["config", "--add", "remote.origin.pushurl", url]);
        }
        let connection = GitRepository::open(fixture.connection(), "").unwrap().verified_connection().unwrap();
        assert_eq!(connection.git_remote.as_ref().unwrap().push, vec![first.clone(), second.clone()]);
        let snapshot = super::super::prepare_document(&solidcraft_doc::Document::new("Pinned mirrors")).unwrap();
        let staged = PendingSave::stage(&connection, "Robot project", "parts/bracket", None, snapshot).unwrap();
        let repository = GitRepository::open(connection.clone(), "").unwrap();
        let published = repository.publish(&staged.record, staged.record.recover(|p| Ok(staged.objects[p].clone())).unwrap()).unwrap();
        for path in ["remote.git", "mirror.git"] {
            assert_eq!(String::from_utf8(Fixture::git(&fixture.root.join(path), &["rev-parse", "main"])).unwrap().trim(), published.commit);
        }
        Fixture::git(&fixture.checkout, &["config", "--unset-all", "remote.origin.pushurl"]);
        Fixture::git(&fixture.checkout, &["config", "remote.origin.pushurl", &second]);
        assert!(repository.test_connection().unwrap_err().to_string().contains("destinations changed"));
        let next = PendingSave::stage(
            &connection,
            "Robot project",
            "parts/second",
            None,
            super::super::prepare_document(&solidcraft_doc::Document::new("Keep queued")).unwrap(),
        )
        .unwrap();
        assert!(repository.publish(&next.record, next.record.recover(|p| Ok(next.objects[p].clone())).unwrap()).is_err());
        assert!(!fixture.checkout.join("Robot project/parts/second").exists());
    }

    #[test]
    fn a_distinct_push_destination_receives_its_first_branch_and_fetch_stays_intact() {
        let fixture = Fixture::new();
        Fixture::git(&fixture.root, &["init", "--bare", "--initial-branch=main", "empty push.git"]);
        let push = format!("file://{}", fixture.root.join("empty push.git").display());
        Fixture::git(&fixture.checkout, &["config", "remote.origin.pushurl", &push]);
        let rewritten = format!("url.file://{}/redirect/.pushInsteadOf", fixture.root.display());
        Fixture::git(&fixture.checkout, &["config", &rewritten, &format!("file://{}/", fixture.root.display())]);
        let before = Fixture::git(&fixture.root.join("remote.git"), &["rev-parse", "main"]);
        let connection = GitRepository::open(fixture.connection(), "").unwrap().verified_connection().unwrap();
        let staged = PendingSave::stage(
            &connection,
            "Robot project",
            "parts/bracket",
            None,
            super::super::prepare_document(&solidcraft_doc::Document::new("First push")).unwrap(),
        )
        .unwrap();
        let repository = GitRepository::open(connection, "").unwrap();
        let publication = repository.publish(&staged.record, staged.record.recover(|p| Ok(staged.objects[p].clone())).unwrap()).unwrap();
        assert_eq!(Fixture::git(&fixture.root.join("remote.git"), &["rev-parse", "main"]), before);
        assert_eq!(String::from_utf8(Fixture::git(&fixture.root.join("empty push.git"), &["rev-parse", "main"])).unwrap().trim(), publication.commit);
        let again = repository.publish(&staged.record, staged.record.recover(|p| Ok(staged.objects[p].clone())).unwrap()).unwrap();
        assert_eq!(again.commit, publication.commit);
    }
}
