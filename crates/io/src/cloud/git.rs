//! Standard Git adapter. Commands receive separate arguments and literal pathspecs. The app
//! never changes global configuration, resets a checkout, stashes work, or force-pushes.
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub use solidcraft_cloud::Published;
use solidcraft_cloud::{Connection, Destination, Error, Manifest, PROJECT_MARKER, PendingSave, Publication, Result, Snapshot, SshIdentity};

mod management;
mod receive;
mod remotes;

fn storage(error: impl std::fmt::Display) -> Error {
    Error::Storage(error.to_string())
}

pub struct GitRepository {
    checkout: PathBuf,
    executable: OsString,
    connection: Connection,
    job: Option<solidcraft_cloud::jobs::Job>,
    token: Option<zeroize::Zeroizing<String>>,
    ssh_askpass: Option<PathBuf>,
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
        let mut repository = Self {
            checkout,
            executable: if executable.is_empty() { "git".into() } else { executable.into() },
            connection,
            job,
            token: None,
            ssh_askpass: None,
        };
        let root = repository.text(&["rev-parse", "--show-toplevel"])?;
        if fs::canonicalize(root.trim()).map_err(storage)? != repository.checkout {
            return Err(storage("checkout location must be the repository's top-level folder"));
        }
        // Resolve local Git configuration without supplying any credentials. Pin its
        // endpoints before a helper can offer a password to a network operation.
        if token.is_some() && repository.connection.provider.is_empty() {
            repository.connection.git_remote = repository.pinned_remote()?;
            super::auth::check_git_destinations(&repository.connection)?;
        }
        repository.token = token.map(zeroize::Zeroizing::new);
        if repository.token.is_some() {
            repository.check_token_destination()?;
        }
        repository.check_ready()?;
        Ok(repository)
    }

    pub(super) fn with_ssh_askpass(mut self, executable: Option<PathBuf>) -> Self {
        self.ssh_askpass = executable;
        self
    }

    fn check_token_destination(&self) -> Result<()> {
        if self.connection.provider.is_empty() {
            self.pinned_remote()?;
            super::auth::check_git_destinations(&self.connection)?;
            return Ok(());
        }
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
            let credential = solidcraft_cloud::jobs::ssh_credential(key);
            #[cfg(windows)]
            let key = key.replace('\\', "/");
            let ssh = shlex::try_join(["ssh", "-i", key.as_str(), "-o", "IdentitiesOnly=yes"]).map_err(storage)?;
            command.env("GIT_SSH_COMMAND", ssh);
            if let Some(helper) = &self.ssh_askpass {
                command
                    .env("SSH_ASKPASS", helper)
                    .env("SSH_ASKPASS_REQUIRE", "force")
                    .env("SOLIDCRAFT_CLOUD_SSH_KEY", key.as_str())
                    .env("LC_ALL", "C")
                    .env("SOLIDCRAFT_CLOUD_SSH_ASKPASS", credential);
            }
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

    pub(super) fn lock(&self) -> Result<super::local::ProcessLock> {
        let path = self.checkout.join(self.text(&["rev-parse", "--git-path", "solidcraft-cloud.lock"])?);
        super::local::ProcessLock::acquire(&path, "another SolidCraft process is synchronizing this checkout; try again shortly")
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

    fn current_commit(&self) -> Result<Option<String>> {
        let reference = format!("refs/heads/{}", self.connection.branch);
        let prefix = format!("{reference} ");
        // An unborn branch has no ref. Enumerate it rather than treating every failed
        // rev-parse (including corrupt repositories or a cancelled job) as an empty repo.
        let references = self.text(&["for-each-ref", "--format=%(refname) %(objectname)", "--", &reference])?;
        if references.lines().any(|line| line.starts_with(&prefix)) {
            self.text(&["rev-parse", "--verify", "HEAD^{commit}"]).map(Some)
        } else {
            Ok(None)
        }
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

    fn head(&self, destination: &Destination) -> Result<(Manifest, Vec<u8>)> {
        if destination.connection != self.connection.id {
            return Err(storage("repository connection mismatch"));
        }
        let prefix = destination.design_path()?;
        let marker = self.read(&format!("{}/{PROJECT_MARKER}", destination.project))?.ok_or_else(|| storage("project marker not found"))?;
        solidcraft_cloud::Project::read(&marker)?;
        let manifest = Manifest::read(&self.read(&format!("{prefix}/manifest.json"))?.ok_or_else(|| storage("design manifest not found"))?)?;
        let document = self.read(&format!("{prefix}/document.json"))?.ok_or_else(|| storage("document not found"))?;
        super::check_document(&document)?;
        if solidcraft_cloud::digest(&document) != manifest.document_hash {
            return Err(Error::Invalid("document hash mismatch".into()));
        }
        Ok((manifest, document))
    }

    pub(super) fn thumbnail(&self, destination: &Destination) -> Result<Option<Vec<u8>>> {
        let (manifest, _) = self.head(destination)?;
        super::read_thumbnail(&manifest, |path| self.read(&format!("{}/{path}", destination.project))?.ok_or_else(|| storage("preview unavailable")))
    }

    pub fn snapshot(&self, destination: &Destination) -> Result<Snapshot> {
        let (manifest, document) = self.head(destination)?;
        let mut objects = std::collections::BTreeMap::new();
        Snapshot::hydrate(&manifest, &document, |path| {
            let bytes = self.read(&format!("{}/{path}", destination.project))?.ok_or_else(|| storage("asset not found"))?;
            objects.insert(path.into(), bytes.clone());
            Ok(bytes)
        })?;
        objects.extend(solidcraft_cloud::Thumbnail::objects(&manifest, |path| {
            self.read(&format!("{}/{path}", destination.project))?.ok_or_else(|| storage("preview unavailable"))
        })?);
        Ok(Snapshot { manifest, document, objects })
    }

    pub(super) fn matches_stored(&self, entry: &solidcraft_cloud::StoredDesign) -> Result<bool> {
        let prefix = entry.destination.design_path()?;
        let Some(bytes) = self.read(&format!("{prefix}/manifest.json"))? else { return Ok(false) };
        // An unsupported design remains listed so Open can show its upgrade message.
        let Ok(manifest) = Manifest::read(&bytes) else { return Ok(false) };
        if !entry.matches_manifest(&manifest)? {
            return Ok(false);
        }
        Ok(self.read(&format!("{prefix}/document.json"))?.is_some_and(|bytes| solidcraft_cloud::digest(&bytes) == entry.document_hash))
    }

    pub fn list(&self) -> Result<Vec<Destination>> {
        self.list_visibility(false)
    }

    pub fn list_deleted(&self) -> Result<Vec<Destination>> {
        self.list_visibility(true)
    }

    fn list_visibility(&self, deleted: bool) -> Result<Vec<Destination>> {
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
                let manifest = self.read(path)?.and_then(|bytes| Manifest::read(&bytes).ok());
                let in_trash = manifest.as_ref().is_some_and(|m| m.deleted() && !m.moved());
                let moved = manifest.as_ref().is_some_and(Manifest::moved);
                if !moved && in_trash == deleted {
                    designs.push(destination);
                }
            }
        }
        Ok(designs)
    }

    /// The durable outbox remains authoritative until this method succeeds.
    pub fn publish(&self, save: &PendingSave, snapshot: Snapshot) -> Result<Published> {
        self.publish_with_parent(save, snapshot, None)
    }

    pub fn publish_with_parent(&self, save: &PendingSave, snapshot: Snapshot, parent: Option<&Published>) -> Result<Published> {
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
        let captured_objects: Vec<_> = snapshot.objects.keys().cloned().collect();
        let remote = self.pinned_remote()?;
        if let Some(remote) = &remote {
            self.fetch_destinations(remote)?;
        }
        self.check_ready()?;
        let previous_commit = self.current_commit()?;
        // The empty tree is only a comparison baseline. The normal commit command
        // creates the first design revision directly, preserving hooks and signing.
        let base = match &previous_commit {
            Some(commit) => commit.clone(),
            None => String::from_utf8(self.run(&["hash-object", "-w", "-t", "tree", "--stdin"], None)?).map_err(storage)?.trim().to_owned(),
        };
        let target = Published::target(save, parent)?;
        let prefix = target.design_path()?;
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
        let resolution_target = if let Some(destination) = solidcraft_cloud::conflicts::target(save)? {
            let prefix = destination.design_path()?;
            if let Some(bytes) = self.read(&format!("{prefix}/manifest.json"))? {
                Manifest::read(&bytes)?;
            }
            if let Some(bytes) = self.read(&format!("{prefix}/document.json"))? {
                super::check_document(&bytes)?;
            }
            if let Some(bytes) = self.committed(&base, &format!("{prefix}/document.json"))? {
                super::check_document(&bytes)?;
            }
            self.committed(&base, &format!("{prefix}/manifest.json"))?
        } else {
            None
        };
        let publication =
            Publication::prepare_with_target(save, snapshot, marker.as_deref(), existing.as_deref(), parent, resolution_target.as_deref())?;
        if matches!(save.manifest.change()?, Some(solidcraft_cloud::management::Change::Rename { .. })) && !publication.already_published {
            let target = publication.destination.design_path()?;
            let manifest = self.committed(&base, &format!("{target}/manifest.json"))?;
            if let Some(bytes) = &manifest {
                Manifest::read(bytes)?;
            }
            let document = self.committed(&base, &format!("{target}/document.json"))?;
            if let Some(bytes) = &document {
                super::check_document(bytes)?;
            }
            if manifest.is_some() || document.is_some() {
                return Err(Error::Conflict);
            }
        }
        let published_path = format!("{}/manifest.json", publication.destination.design_path()?);
        let existing_conflict = if publication.conflict && publication.destination != target {
            self.committed(&base, &published_path)?.map(|bytes| Manifest::read(&bytes)).transpose()?
        } else {
            None
        };
        if existing_conflict.as_ref().is_some_and(|manifest| solidcraft_cloud::revision(manifest) != Some(save.id.as_str())) {
            return Err(Error::Conflict);
        }
        if !publication.already_published && existing_conflict.is_none() {
            let mut files = publication.files;
            let attributes_path = format!("{}/.gitattributes", publication.destination.project);
            if self.read(&attributes_path)?.is_some_and(|bytes| bytes != Publication::GIT_ATTRIBUTES) {
                return Err(storage("cloud project .gitattributes has custom changes; preserve them before synchronizing"));
            }
            // Already-present immutable objects are neither rewritten nor staged.
            let object_prefix = format!("{}/objects/", publication.destination.project);
            let mut reusable = Vec::new();
            for (path, bytes) in &files {
                if (path.starts_with(&object_prefix) || path == &attributes_path)
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
            self.check_ready()?;
            if self.current_commit()? != previous_commit {
                return Err(Error::Conflict);
            }
            for (relative, bytes) in &files {
                let path = self.path(relative)?;
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(storage)?;
                }
                crate::safe_write::write_atomic(&path, bytes, false).map_err(storage)?;
            }
            // The explicit save owns these paths even if a repository ignores the project.
            self.run(&["add", "--force", "--pathspec-from-file=-", "--pathspec-file-nul"], Some(&paths))?;
            let mut identity = Vec::new();
            if !self.text(&["config", "--get", "user.name"]).is_ok_and(|value| !value.is_empty()) {
                identity.extend(["-c", "user.name=SolidCraft"]);
            }
            if !self.text(&["config", "--get", "user.email"]).is_ok_and(|value| !value.is_empty()) {
                identity.extend(["-c", "user.email=cloud@solidcraft.invalid"]);
            }
            let action = save.manifest.change()?.map(|c| c.message()).unwrap_or("Save");
            let message = format!("{action} {} ({})", save.destination.file, save.id);
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
        // A higher-priority Git attribute or hook can transform bytes despite the project's
        // rules. Check the actual commit on retries too, before any remote publication.
        let prefix = publication.destination.design_path()?;
        let document = self.committed(&commit, &format!("{prefix}/document.json"))?.ok_or_else(|| storage("saved document is missing"))?;
        if solidcraft_cloud::digest(&document) != save.manifest.document_hash {
            return Err(storage("Git changed captured document bytes; check repository attributes and filters. Local save retained"));
        }
        for path in captured_objects {
            let bytes =
                self.committed(&commit, &format!("{}/{path}", publication.destination.project))?.ok_or_else(|| storage("saved asset is missing"))?;
            if path.rsplit('/').next() != Some(solidcraft_cloud::digest(&bytes).as_str()) {
                return Err(storage("Git changed captured asset bytes; check repository attributes and filters. Local save retained"));
            }
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
        branch: String,
    }
    impl Fixture {
        fn git(path: &Path, args: &[&str]) -> Vec<u8> {
            let output = Command::new("git").current_dir(path).args(args).output().unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            output.stdout
        }
        fn new() -> Self {
            let fixture = Self::empty();
            fs::write(fixture.checkout.join("README.md"), "Project repository\n").unwrap();
            Self::git(&fixture.checkout, &["add", "README.md"]);
            Self::git(&fixture.checkout, &["commit", "-m", "Initial repository"]);
            Self::git(&fixture.checkout, &["push", "origin", "main"]);
            fixture
        }
        fn empty() -> Self {
            Self::empty_on("main", "sha1")
        }
        fn empty_on(branch: &str, object_format: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "solidcraft cloud git {} {}",
                std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
            ));
            fs::create_dir_all(&root).unwrap();
            Self::git(&root, &["init", "--bare", &format!("--initial-branch={branch}"), &format!("--object-format={object_format}"), "remote.git"]);
            Self::git(&root, &["clone", "remote.git", "checkout"]);
            let checkout = root.join("checkout");
            Self::git(&checkout, &["config", "user.name", "Cloud tests"]);
            Self::git(&checkout, &["config", "user.email", "cloud@example.invalid"]);
            Self::git(&checkout, &["config", "commit.gpgSign", "false"]);
            Self { root, checkout, branch: branch.into() }
        }
        fn connection(&self) -> Connection {
            Connection { checkout: self.checkout.to_string_lossy().into_owned(), branch: self.branch.clone(), ..Default::default() }
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
    fn empty_checkout_publishes_first_design_without_committing_unrelated_staged_files() {
        for (branch, object_format, remote) in [("main", "sha1", ""), ("main", "sha1", "origin"), ("designs/work", "sha256", "origin")] {
            let fixture = Fixture::empty_on(branch, object_format);
            fs::write(fixture.checkout.join("staged.txt"), b"unrelated staged").unwrap();
            Fixture::git(&fixture.checkout, &["add", "staged.txt"]);
            fs::write(fixture.checkout.join("staged.txt"), b"unrelated working changes").unwrap();
            let connection = Connection { remote: remote.into(), ..fixture.connection() };
            let repository = GitRepository::open(connection.clone(), "").unwrap();
            repository.refresh().unwrap();
            // Another client can connect before the first publication exists too.
            Fixture::git(&fixture.root, &["clone", "remote.git", "waiting checkout"]);
            let save = PendingSave::stage(
                &connection,
                "Robot project",
                "parts/bracket",
                None,
                super::super::prepare_document(&solidcraft_doc::Document::new("Bracket")).unwrap(),
            )
            .unwrap();
            let snapshot = || save.record.recover(|path| Ok(save.objects[path].clone())).unwrap();
            let published = repository.publish(&save.record, snapshot()).unwrap();
            assert!(!published.conflict);
            assert_eq!(Fixture::git(&fixture.checkout, &["rev-list", "--count", "HEAD"]), b"1\n");
            assert_eq!(Fixture::git(&fixture.checkout, &["diff", "--cached", "--name-only"]), b"staged.txt\n");
            assert_eq!(Fixture::git(&fixture.checkout, &["show", ":staged.txt"]), b"unrelated staged");
            assert_eq!(fs::read(fixture.checkout.join("staged.txt")).unwrap(), b"unrelated working changes");
            assert!(Fixture::git(&fixture.checkout, &["ls-tree", "HEAD", "--", "staged.txt"]).is_empty());
            assert_eq!(repository.open_design(&published.destination).unwrap().0.name, "Bracket");
            assert_eq!(repository.publish(&save.record, snapshot()).unwrap().commit, published.commit);
            if !remote.is_empty() {
                assert_eq!(Fixture::git(&fixture.root, &["--git-dir=remote.git", "rev-parse", branch]), format!("{}\n", published.commit).as_bytes());
                let waiting = fixture.root.join("waiting checkout");
                fs::write(waiting.join("peer-staged.txt"), b"peer work").unwrap();
                Fixture::git(&waiting, &["add", "peer-staged.txt"]);
                let peer = GitRepository::open(Connection { checkout: waiting.to_string_lossy().into_owned(), ..connection }, "").unwrap();
                peer.refresh().unwrap();
                assert_eq!(peer.open_design(&published.destination).unwrap().0.name, "Bracket");
                assert_eq!(Fixture::git(&waiting, &["diff", "--cached", "--name-only"]), b"peer-staged.txt\n");
            }
        }
    }

    #[test]
    #[cfg(unix)]
    fn rejected_initial_push_resumes_the_same_commit_from_the_durable_queue() {
        use std::os::unix::fs::PermissionsExt;
        for mirror in [false, true] {
            let fixture = Fixture::empty();
            let rejected_remote = if mirror {
                Fixture::git(&fixture.root, &["init", "--bare", "--initial-branch=main", "mirror.git"]);
                for remote in ["remote.git", "mirror.git"] {
                    Fixture::git(&fixture.checkout, &["config", "--add", "remote.origin.pushurl", fixture.root.join(remote).to_str().unwrap()]);
                }
                "mirror.git"
            } else {
                "remote.git"
            };
            let hook = fixture.root.join(rejected_remote).join("hooks/pre-receive");
            fs::write(&hook, b"#!/bin/sh\nexit 1\n").unwrap();
            fs::set_permissions(&hook, fs::Permissions::from_mode(0o700)).unwrap();
            let outbox = super::super::LocalOutbox::new(fixture.root.join("recovery")).unwrap();
            let save = fixture.stage();
            outbox.stage(&save).unwrap();
            let repository = GitRepository::open(fixture.connection(), "").unwrap();
            assert!(repository.publish(&save.record, outbox.recover(&save.record).unwrap()).is_err());
            assert_eq!(outbox.pending().unwrap().len(), 1);
            let commit = Fixture::git(&fixture.checkout, &["rev-parse", "HEAD"]);
            if mirror {
                assert_eq!(Fixture::git(&fixture.root.join("remote.git"), &["rev-parse", "main"]), commit);
            }
            // An unrelated later commit must remain local when retrying this save.
            Fixture::git(&fixture.checkout, &["commit", "--allow-empty", "-m", "Later unrelated change"]);
            fs::remove_file(hook).unwrap();
            let restarted_outbox = super::super::LocalOutbox::new(fixture.root.join("recovery")).unwrap();
            let published = repository.publish(&save.record, restarted_outbox.recover(&save.record).unwrap()).unwrap();
            assert_eq!(format!("{}\n", published.commit).as_bytes(), commit);
            assert_eq!(Fixture::git(&fixture.checkout, &["rev-list", "--count", "HEAD"]), b"2\n");
            restarted_outbox.published(&save.record, &published).unwrap();
            assert!(restarted_outbox.pending().unwrap().is_empty());
            assert_eq!(Fixture::git(&fixture.root.join(rejected_remote), &["rev-parse", "main"]), commit);
            assert_eq!(Fixture::git(&fixture.root.join(rejected_remote), &["rev-list", "--count", "main"]), b"1\n");
        }
    }

    #[test]
    fn external_git_operations_and_wrong_branches_pause_before_writing() {
        let fixture = Fixture::new();
        let repository = GitRepository::open(fixture.connection(), "").unwrap();
        let save = fixture.stage();
        let snapshot = || save.record.recover(|path| Ok(save.objects[path].clone())).unwrap();
        for marker in ["MERGE_HEAD", "CHERRY_PICK_HEAD", "REVERT_HEAD", "rebase-merge", "rebase-apply", "BISECT_LOG", "index.lock"] {
            let path = fixture.checkout.join(".git").join(marker);
            fs::write(&path, b"external operation").unwrap();
            assert!(repository.publish(&save.record, snapshot()).unwrap_err().to_string().contains("external Git operation"));
            assert!(!fixture.checkout.join("Robot project").exists());
            fs::remove_file(path).unwrap();
        }
        Fixture::git(&fixture.checkout, &["checkout", "-b", "other"]);
        assert!(repository.publish(&save.record, snapshot()).is_err());
        Fixture::git(&fixture.checkout, &["checkout", "--detach"]);
        assert!(repository.publish(&save.record, snapshot()).is_err());
        assert!(!fixture.checkout.join("Robot project").exists());
    }

    #[test]
    #[cfg(unix)]
    fn divergent_histories_and_commit_checks_preserve_local_work_for_retry() {
        use std::os::unix::fs::PermissionsExt;
        for check in ["divergence", "hook", "signing"] {
            let fixture = Fixture::new();
            let executable = fixture.root.join("reject commit");
            if check == "divergence" {
                Fixture::git(&fixture.root, &["clone", "remote.git", "peer"]);
                Fixture::git(
                    &fixture.root.join("peer"),
                    &[
                        "-c",
                        "user.name=Peer",
                        "-c",
                        "user.email=peer@example.invalid",
                        "-c",
                        "commit.gpgSign=false",
                        "commit",
                        "--allow-empty",
                        "-m",
                        "Peer change",
                    ],
                );
                Fixture::git(&fixture.root.join("peer"), &["push", "origin", "main"]);
                Fixture::git(&fixture.checkout, &["commit", "--allow-empty", "-m", "Local change"]);
            } else {
                fs::write(&executable, b"#!/bin/sh\ntouch \"$0.invoked\"\nexit 1\n").unwrap();
                fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
                if check == "hook" {
                    fs::copy(&executable, fixture.checkout.join(".git/hooks/pre-commit")).unwrap();
                } else {
                    Fixture::git(&fixture.checkout, &["config", "gpg.program", executable.to_str().unwrap()]);
                    Fixture::git(&fixture.checkout, &["config", "user.signingkey", "example"]);
                    Fixture::git(&fixture.checkout, &["config", "commit.gpgSign", "true"]);
                }
            }
            fs::write(fixture.checkout.join("staged.txt"), b"staged unrelated").unwrap();
            Fixture::git(&fixture.checkout, &["add", "staged.txt"]);
            fs::write(fixture.checkout.join("staged.txt"), b"working unrelated").unwrap();
            let before = Fixture::git(&fixture.checkout, &["rev-parse", "HEAD"]);
            let remote_before = Fixture::git(&fixture.root.join("remote.git"), &["rev-parse", "main"]);
            let outbox = super::super::LocalOutbox::new(fixture.root.join("recovery")).unwrap();
            let save = fixture.stage();
            outbox.stage(&save).unwrap();
            let repository = GitRepository::open(fixture.connection(), "").unwrap();
            assert!(repository.publish(&save.record, outbox.recover(&save.record).unwrap()).is_err());
            assert_eq!(Fixture::git(&fixture.checkout, &["rev-parse", "HEAD"]), before);
            assert_eq!(Fixture::git(&fixture.root.join("remote.git"), &["rev-parse", "main"]), remote_before);
            assert_eq!(Fixture::git(&fixture.checkout, &["show", ":staged.txt"]), b"staged unrelated");
            assert_eq!(fs::read(fixture.checkout.join("staged.txt")).unwrap(), b"working unrelated");
            assert_eq!(outbox.pending().unwrap().len(), 1);
            if check == "divergence" {
                assert!(!fixture.checkout.join("Robot project").exists());
            } else {
                let invoked =
                    if check == "hook" { fixture.checkout.join(".git/hooks/pre-commit.invoked") } else { executable.with_extension("invoked") };
                assert!(invoked.exists(), "the configured {check} must actually run");
                if check == "hook" {
                    fs::remove_file(fixture.checkout.join(".git/hooks/pre-commit")).unwrap();
                } else {
                    Fixture::git(&fixture.checkout, &["config", "commit.gpgSign", "false"]);
                }
                repository.publish(&save.record, outbox.recover(&save.record).unwrap()).unwrap();
                assert_eq!(Fixture::git(&fixture.checkout, &["diff", "--cached", "--name-only"]), b"staged.txt\n");
            }
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
    fn linked_worktrees_publish_their_branch_and_preserve_their_separate_index() {
        let fixture = Fixture::new();
        let path = fixture.root.join("linked checkout");
        Fixture::git(&fixture.checkout, &["worktree", "add", "-b", "designs/work", path.to_str().unwrap()]);
        fs::write(path.join("staged.txt"), b"linked work").unwrap();
        Fixture::git(&path, &["add", "staged.txt"]);
        let original = Fixture::git(&fixture.checkout, &["rev-parse", "HEAD"]);
        let connection = Connection { checkout: path.to_string_lossy().into_owned(), branch: "designs/work".into(), ..fixture.connection() };
        let repository = GitRepository::open(connection.clone(), "").unwrap();
        let save = PendingSave::stage(
            &connection,
            "Robot project",
            "parts/bracket",
            None,
            super::super::prepare_document(&solidcraft_doc::Document::new("Linked bracket")).unwrap(),
        )
        .unwrap();
        let published = repository.publish(&save.record, save.record.recover(|path| Ok(save.objects[path].clone())).unwrap()).unwrap();
        assert_eq!(repository.open_design(&published.destination).unwrap().0.name, "Linked bracket");
        assert_eq!(Fixture::git(&path, &["diff", "--cached", "--name-only"]), b"staged.txt\n");
        assert_eq!(Fixture::git(&fixture.checkout, &["rev-parse", "HEAD"]), original);
        assert_eq!(Fixture::git(&fixture.root.join("remote.git"), &["rev-parse", "designs/work"]), format!("{}\n", published.commit).as_bytes());
        assert_eq!(Fixture::git(&fixture.root.join("remote.git"), &["rev-parse", "main"]), original);
    }

    #[test]
    fn externally_changed_design_paths_are_preserved_before_any_save_writes() {
        for state in ["untracked", "staged", "unstaged"] {
            let fixture = Fixture::empty();
            let repository = GitRepository::open(fixture.connection(), "").unwrap();
            let first = fixture.stage();
            let parent = if state == "untracked" {
                None
            } else {
                Some(repository.publish(&first.record, first.record.recover(|path| Ok(first.objects[path].clone())).unwrap()).unwrap().revision)
            };
            let save = PendingSave::stage(
                &fixture.connection(),
                "Robot project",
                "parts/bracket",
                parent,
                super::super::prepare_document(&solidcraft_doc::Document::new("Next captured edit")).unwrap(),
            )
            .unwrap();
            let relative = "Robot project/parts/bracket/document.json";
            let path = fixture.checkout.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let external = super::super::prepare_document(&solidcraft_doc::Document::new("External edit")).unwrap().document;
            fs::write(&path, &external).unwrap();
            if state == "staged" {
                Fixture::git(&fixture.checkout, &["add", "--", relative]);
            }
            let before = Fixture::git(&fixture.checkout, &["status", "--porcelain=v1", "-z"]);
            assert!(repository.publish(&save.record, save.record.recover(|path| Ok(save.objects[path].clone())).unwrap()).is_err());
            assert_eq!(fs::read(path).unwrap(), external);
            assert_eq!(Fixture::git(&fixture.checkout, &["status", "--porcelain=v1", "-z"]), before);
        }
    }

    #[test]
    fn cloud_projects_keep_exact_bytes_across_line_endings_and_repository_ignore_rules() {
        for ignored in [false, true] {
            let fixture = Fixture::empty();
            if ignored {
                fs::write(fixture.checkout.join(".gitignore"), b"Robot project/\n").unwrap();
            }
            let repository = GitRepository::open(fixture.connection(), "").unwrap();
            let save = fixture.stage();
            let snapshot = save.record.recover(|path| Ok(save.objects[path].clone())).unwrap();
            let document = snapshot.document.clone();
            let published = repository.publish(&save.record, snapshot).unwrap();
            Fixture::git(&fixture.root, &["clone", "-c", "core.autocrlf=true", "remote.git", "CRLF checkout"]);
            let peer = GitRepository::open(
                Connection { checkout: fixture.root.join("CRLF checkout").to_string_lossy().into_owned(), ..fixture.connection() },
                "",
            )
            .unwrap();
            assert_eq!(peer.open_design(&published.destination).unwrap().0.name, "Bracket");
            assert_eq!(peer.snapshot(&published.destination).unwrap().document, document);
            assert_eq!(fs::read(fixture.checkout.join(".gitignore")).ok(), ignored.then(|| b"Robot project/\n".to_vec()));
            let attributes = fixture.checkout.join("Robot project/.gitattributes");
            fs::write(&attributes, b"custom project rules\n").unwrap();
            let next = PendingSave::stage(
                &fixture.connection(),
                "Robot project",
                "parts/bracket",
                Some(published.revision),
                super::super::prepare_document(&solidcraft_doc::Document::new("Next edit")).unwrap(),
            )
            .unwrap();
            assert!(repository.publish(&next.record, next.record.recover(|path| Ok(next.objects[path].clone())).unwrap()).is_err());
            assert_eq!(fs::read(attributes).unwrap(), b"custom project rules\n");
        }
    }

    #[test]
    fn higher_priority_git_filters_cannot_publish_changed_capture_bytes_even_on_retry() {
        let fixture = Fixture::empty();
        fs::write(fixture.checkout.join(".git/info/attributes"), b"**/document.json filter=convert\n").unwrap();
        Fixture::git(&fixture.checkout, &["config", "filter.convert.clean", "cat; printf '\\n'"]);
        Fixture::git(&fixture.checkout, &["config", "filter.convert.smudge", "cat"]);
        let outbox = super::super::LocalOutbox::new(fixture.root.join("recovery")).unwrap();
        let save = fixture.stage();
        outbox.stage(&save).unwrap();
        let repository = GitRepository::open(fixture.connection(), "").unwrap();
        for _ in 0..2 {
            let error = repository.publish(&save.record, outbox.recover(&save.record).unwrap()).unwrap_err();
            assert!(error.to_string().contains("Git changed captured document bytes"), "{error}");
            assert_eq!(outbox.pending().unwrap().len(), 1);
            assert_eq!(Fixture::git(&fixture.checkout, &["rev-list", "--count", "HEAD"]), b"1\n");
            assert!(Fixture::git(&fixture.root.join("remote.git"), &["for-each-ref", "refs/heads/"]).is_empty());
        }
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
    fn remembered_ssh_helper_is_process_and_identity_scoped() {
        let fixture = Fixture::new();
        let key = fixture.root.join("key with 'quotes'");
        fs::write(&key, b"example").unwrap();
        let path = key.to_str().unwrap();
        let connection = Connection { ssh_identity: SshIdentity::File(path.into()), ..fixture.connection() };
        let repository = GitRepository::open(connection, "").unwrap().with_ssh_askpass(Some("/app path/solidcraft".into()));
        let command = repository.command().unwrap();
        let env: std::collections::BTreeMap<_, _> =
            command.get_envs().map(|(k, v)| (k.to_string_lossy().into_owned(), v.map(|v| v.to_string_lossy().into_owned()))).collect();
        assert_eq!(env["SSH_ASKPASS"].as_deref(), Some("/app path/solidcraft"));
        assert_eq!(env["SSH_ASKPASS_REQUIRE"].as_deref(), Some("force"));
        assert_eq!(env["SOLIDCRAFT_CLOUD_SSH_ASKPASS"].as_deref(), Some(solidcraft_cloud::jobs::ssh_credential(path).as_str()));
        #[cfg(not(windows))]
        assert_eq!(env["SOLIDCRAFT_CLOUD_SSH_KEY"].as_deref(), Some(path));
        let other = GitRepository::open(fixture.connection(), "").unwrap().command().unwrap();
        assert!(other.get_envs().all(|(k, _)| k != "SSH_ASKPASS" && k != "SOLIDCRAFT_CLOUD_SSH_ASKPASS"));
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
