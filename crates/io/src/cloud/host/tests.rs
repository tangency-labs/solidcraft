use super::*;
use solidcraft_cloud::{Snapshot, StagedSave};
use std::{
    fs,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "solidcraft cloud worker {} {}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn host(&self) -> NativeHost {
        NativeHost::new(self.0.join("recovery")).unwrap()
    }

    fn connection(&self, id: &str) -> Connection {
        Connection {
            id: id.into(),
            checkout: self.0.join(id).to_string_lossy().into_owned(),
            credential: format!("cloud:{id}"),
            remote: String::new(),
            ..Default::default()
        }
    }

    fn git(path: &Path, args: &[&str]) -> Vec<u8> {
        let result = Command::new("git").current_dir(path).args(args).output().unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
        result.stdout
    }

    fn checkout(&self, id: &str) -> Connection {
        let connection = self.connection(id);
        let path = Path::new(&connection.checkout);
        fs::create_dir_all(path).unwrap();
        Self::git(path, &["init", "--initial-branch=main"]);
        Self::git(path, &["config", "user.name", "Cloud tests"]);
        Self::git(path, &["config", "user.email", "cloud@example.invalid"]);
        Self::git(path, &["config", "commit.gpgSign", "false"]);
        Self::git(path, &["commit", "--allow-empty", "-m", "Initial repository"]);
        connection
    }

    fn outbox(&self, connection: &Connection) -> LocalOutbox {
        LocalOutbox::new(self.0.join("recovery").join(solidcraft_cloud::digest(connection.id.as_bytes()))).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn save(connection: &Connection, file: &str, name: &str, parent: Option<String>) -> StagedSave {
    PendingSave::stage(connection, "Robot", file, parent, super::super::prepare_document(&solidcraft_doc::Document::new(name)).unwrap()).unwrap()
}

fn complete(job: &Job) -> solidcraft_cloud::jobs::JobStatus {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let status = job.status().unwrap();
        if !matches!(status.phase, Phase::Working | Phase::Syncing) {
            // SavedLocally is the durable boundary; a worker can still start its upload.
            // Tests waiting on a synchronous save use published() below instead.
            return status;
        }
        assert!(Instant::now() < deadline, "operation timed out: {status:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn published(job: &Job) -> solidcraft_cloud::jobs::JobStatus {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let status = job.status().unwrap();
        if matches!(status.phase, Phase::Synced | Phase::Failed) || status.message.is_some() {
            return status;
        }
        assert!(Instant::now() < deadline, "publication timed out: {status:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn provider_api_uses_native_recovery_without_git_and_manages_revisions_across_restart() {
    use solidcraft_cloud::{
        Destination, Manifest, Publication, Published,
        management::{Change, History, Revision},
    };
    use std::sync::{Arc, Mutex};
    #[derive(Default)]
    struct Remote {
        online: bool,
        files: BTreeMap<String, Vec<u8>>,
        commits: Vec<BTreeMap<String, Vec<u8>>>,
    }
    struct Access(Arc<Mutex<Remote>>);
    struct Api(Arc<Mutex<Remote>>, Connection);
    fn offline() -> Error {
        Error::Storage("test offline".into())
    }
    fn snapshot(files: &BTreeMap<String, Vec<u8>>, destination: &Destination) -> Result<Snapshot> {
        let path = destination.design_path()?;
        let manifest = Manifest::read(files.get(&format!("{path}/manifest.json")).ok_or_else(offline)?)?;
        let document = files.get(&format!("{path}/document.json")).ok_or_else(offline)?.clone();
        let objects = files
            .iter()
            .filter_map(|(p, b)| {
                p.strip_prefix(&format!("{}/", destination.project)).filter(|p| p.starts_with("objects/")).map(|p| (p.into(), b.clone()))
            })
            .collect();
        Ok(Snapshot { manifest, document, objects })
    }
    impl super::super::NativeAccess for Access {
        fn credential(&self, _: &str) -> Result<Option<String>> {
            Err(offline())
        }
        fn store_credential(&self, _: &str, _: &str) -> Result<()> {
            Err(offline())
        }
        fn prepare(&self, connection: Connection, _: &Job) -> Result<Connection> {
            Ok(connection)
        }
        fn uses_provider_api(&self, _: &Connection) -> bool {
            true
        }
        fn provider_repository(&self, connection: &Connection, _: &Job) -> Result<Box<dyn super::super::ProviderRepository>> {
            Ok(Box::new(Api(self.0.clone(), connection.clone())))
        }
    }
    impl super::super::ProviderRepository for Api {
        fn test(&mut self) -> Result<()> {
            if self.0.lock().unwrap().online { Ok(()) } else { Err(offline()) }
        }
        fn list(&mut self, stored: &mut [solidcraft_cloud::StoredDesign]) -> Result<Vec<Destination>> {
            self.test()?;
            let state = self.0.lock().unwrap();
            for entry in stored {
                let matches = match snapshot(&state.files, &entry.destination) {
                    Ok(s) => entry.matches_manifest(&s.manifest)?,
                    Err(_) => false,
                };
                if !matches {
                    entry.outdated = true;
                    entry.phase = Phase::Ready;
                }
            }
            state
                .files
                .keys()
                .filter_map(|p| p.strip_prefix("Robot/").and_then(|p| p.strip_suffix("/manifest.json")))
                .filter_map(|file| {
                    let destination = Destination { connection: self.1.id.clone(), project: "Robot".into(), file: file.into() };
                    match snapshot(&state.files, &destination) {
                        Ok(s) if !s.manifest.deleted() => Some(Ok(destination)),
                        Ok(_) => None,
                        Err(e) => Some(Err(e)),
                    }
                })
                .collect()
        }
        fn trash(&mut self) -> Result<Vec<Destination>> {
            self.test()?;
            let state = self.0.lock().unwrap();
            Ok(state
                .files
                .keys()
                .filter_map(|p| p.strip_prefix("Robot/").and_then(|p| p.strip_suffix("/manifest.json")))
                .map(|file| Destination { connection: self.1.id.clone(), project: "Robot".into(), file: file.into() })
                .filter(|d| snapshot(&state.files, d).is_ok_and(|s| s.manifest.deleted() && !s.manifest.moved()))
                .collect())
        }
        fn open(&mut self, destination: &Destination) -> Result<Snapshot> {
            self.test()?;
            snapshot(&self.0.lock().unwrap().files, destination)
        }
        fn thumbnail(&mut self, _: &Destination) -> Result<Option<Vec<u8>>> {
            self.test()?;
            Ok(None)
        }
        fn history(&mut self, d: &Destination) -> Result<History> {
            let s = self.open(d)?;
            Ok(History {
                revision: solidcraft_cloud::revision(&s.manifest).map(str::to_owned),
                revisions: vec![Revision {
                    commit: format!("{:040x}", self.0.lock().unwrap().commits.len()),
                    file: d.file.clone(),
                    date: String::new(),
                    timestamp: 0,
                    message: "test".into(),
                }],
                queued: false,
                deleted: s.manifest.deleted(),
            })
        }
        fn open_at(&mut self, d: &Destination, commit: &str) -> Result<Snapshot> {
            self.test()?;
            let n = usize::from_str_radix(commit, 16).map_err(|_| offline())?;
            snapshot(self.0.lock().unwrap().commits.get(n.saturating_sub(1)).ok_or_else(offline)?, d)
        }
        fn publish(&mut self, save: &PendingSave, s: Snapshot, parent: Option<&Published>) -> Result<Published> {
            self.test()?;
            let mut state = self.0.lock().unwrap();
            let target = Published::target(save, parent)?;
            let existing = state.files.get(&format!("{}/manifest.json", target.design_path()?)).cloned();
            let plan = Publication::prepare_with_parent(save, s, None, existing.as_deref(), parent)?;
            if !plan.already_published {
                state.files.extend(plan.files);
                let files = state.files.clone();
                state.commits.push(files);
            }
            Ok(Published {
                destination: plan.destination,
                revision: save.id.clone(),
                commit: format!("{:040x}", state.commits.len()),
                conflict: plan.conflict,
            })
        }
    }
    let fixture = Fixture::new();
    let connection = Connection { id: "provider".into(), repository: "example/designs".into(), checkout: String::new(), ..Default::default() };
    let remote = Arc::new(Mutex::new(Remote::default()));
    let host = fixture.host().with_access(Arc::new(Access(remote.clone())));
    let executable = "/missing/git-must-never-run".to_owned();
    assert_eq!(complete(&host.submit(Operation::Setup(connection.clone()), executable.clone()).unwrap()).phase, Phase::Ready);
    let first = save(&connection, "parts/bracket", "First", None);
    let revision = first.record.id.clone();
    let captured = published(
        &host.submit(Operation::Save { connection: connection.clone(), save: Box::new(first), synchronize: true }, executable.clone()).unwrap(),
    );
    assert_eq!(captured.phase, Phase::SavedLocally);
    assert!(captured.saved_locally);
    remote.lock().unwrap().online = true;
    let host = fixture.host().with_access(Arc::new(Access(remote.clone())));
    for _ in 0..2 {
        assert_eq!(published(&host.submit(Operation::Synchronize(connection.clone()), executable.clone()).unwrap()).phase, Phase::Synced);
    }
    assert_eq!(remote.lock().unwrap().commits.len(), 1);
    let destination = Destination { connection: connection.id.clone(), project: "Robot".into(), file: "parts/bracket".into() };
    let renamed = Destination { file: "parts/renamed".into(), ..destination.clone() };
    let rename = published(
        &host
            .submit(
                Operation::Manage {
                    connection: connection.clone(),
                    destination: destination.clone(),
                    expected_revision: Some(revision),
                    change: Change::Rename { file: renamed.file.clone() },
                },
                executable.clone(),
            )
            .unwrap(),
    );
    assert_eq!(rename.phase, Phase::Synced, "{rename:?}");
    assert_eq!(rename.managed.len(), 1);
    remote.lock().unwrap().online = false;
    let deleted = published(
        &host
            .submit(
                Operation::Manage {
                    connection: connection.clone(),
                    destination: renamed.clone(),
                    expected_revision: rename.revision,
                    change: Change::Delete,
                },
                executable.clone(),
            )
            .unwrap(),
    );
    assert_eq!(deleted.phase, Phase::SavedLocally);
    remote.lock().unwrap().online = true;
    let host = fixture.host().with_access(Arc::new(Access(remote.clone())));
    assert_eq!(published(&host.submit(Operation::Synchronize(connection.clone()), executable.clone()).unwrap()).phase, Phase::Synced);
    let trash = complete(&host.submit(Operation::Trash(connection.clone()), executable.clone()).unwrap());
    assert_eq!(trash.result, Some(serde_json::json!([renamed.clone()])));
    let restored = published(
        &host
            .submit(
                Operation::Manage {
                    connection: connection.clone(),
                    destination: renamed.clone(),
                    expected_revision: deleted.revision,
                    change: Change::Restore { commit: format!("{:040x}", 1), file: destination.file },
                },
                executable.clone(),
            )
            .unwrap(),
    );
    assert_eq!(restored.phase, Phase::Synced, "{restored:?}");
    let listed = complete(&host.submit(Operation::List(connection.clone()), executable).unwrap());
    assert_eq!(listed.result, Some(serde_json::json!([renamed])));
    assert!(fixture.outbox(&connection).pending().unwrap().is_empty());
    assert!(connection.checkout.is_empty());
}

#[test]
fn management_renames_trashes_restores_and_resumes_without_touching_other_work() {
    use solidcraft_cloud::{Destination, management::Change};
    let fixture = Fixture::new();
    let connection = fixture.checkout("managed");
    let host = fixture.host();
    let destination = Destination { connection: connection.id.clone(), project: "Robot".into(), file: "parts/bracket".into() };
    let initial = published(
        &host
            .submit(
                Operation::Save {
                    connection: connection.clone(),
                    save: Box::new(save(&connection, &destination.file, "Bracket", None)),
                    synchronize: true,
                },
                String::new(),
            )
            .unwrap(),
    );
    assert_eq!(initial.phase, Phase::Synced);
    let checkout = Path::new(&connection.checkout);
    let first_commit = String::from_utf8(Fixture::git(checkout, &["rev-parse", "HEAD"])).unwrap().trim().to_owned();
    fs::write(checkout.join("staged.txt"), "unrelated staged bytes").unwrap();
    Fixture::git(checkout, &["add", "staged.txt"]);
    fs::write(checkout.join("untracked.txt"), "unrelated working bytes").unwrap();
    let stage = Fixture::git(checkout, &["show", ":staged.txt"]);
    let mut edited = super::super::prepare_document(&solidcraft_doc::Document::new("Edited bracket")).unwrap();
    let png = include_bytes!("../../../../../assets/samples/exports/0.png").to_vec();
    solidcraft_cloud::attach_thumbnail(&mut edited, png.clone()).unwrap();
    let edited = PendingSave::stage(&connection, "Robot", &destination.file, initial.revision.clone(), edited).unwrap();
    let second = published(
        &host.submit(Operation::Save { connection: connection.clone(), save: Box::new(edited), synchronize: true }, String::new()).unwrap(),
    );
    assert_eq!(second.phase, Phase::Synced);
    let renamed = Destination { file: "parts/Renamed bracket".into(), ..destination.clone() };
    let action = Operation::Manage {
        connection: connection.clone(),
        destination: destination.clone(),
        expected_revision: second.revision.clone(),
        change: Change::Rename { file: renamed.file.clone() },
    };
    let rename = published(&host.submit(action, String::new()).unwrap());
    assert_eq!(rename.phase, Phase::Synced, "{rename:?}");
    assert_eq!(rename.managed.len(), 1);
    let repository = GitRepository::open(connection.clone(), "").unwrap();
    assert_eq!(repository.list().unwrap(), vec![renamed.clone()]);
    assert!(repository.list_deleted().unwrap().is_empty(), "a rename is not a deleted design in Trash");
    assert_eq!(repository.open_design(&renamed).unwrap().0.name, "Renamed bracket");
    assert_eq!(repository.thumbnail(&renamed).unwrap(), Some(png), "renaming preserves the preview against the new document hash");
    assert!(fixture.outbox(&connection).cached(&connection, &destination).unwrap().is_none());
    assert_eq!(Fixture::git(checkout, &["show", ":staged.txt"]), stage);
    assert_eq!(fs::read_to_string(checkout.join("untracked.txt")).unwrap(), "unrelated working bytes");
    let stale = complete(
        &host
            .submit(
                Operation::Manage {
                    connection: connection.clone(),
                    destination: renamed.clone(),
                    expected_revision: second.revision,
                    change: Change::Delete,
                },
                String::new(),
            )
            .unwrap(),
    );
    assert_eq!(stale.phase, Phase::Failed);
    assert!(fixture.outbox(&connection).pending().unwrap().is_empty());
    let deleted = published(
        &host
            .submit(
                Operation::Manage {
                    connection: connection.clone(),
                    destination: renamed.clone(),
                    expected_revision: rename.revision,
                    change: Change::Delete,
                },
                String::new(),
            )
            .unwrap(),
    );
    assert_eq!(deleted.phase, Phase::Synced, "{deleted:?}");
    assert!(repository.list().unwrap().is_empty());
    assert_eq!(repository.list_deleted().unwrap(), vec![renamed.clone()]);
    assert!(repository.open_design(&renamed).is_err());
    drop(host);
    let reopened = fixture.host();
    let trash = complete(&reopened.submit(Operation::Trash(connection.clone()), String::new()).unwrap());
    assert_eq!(trash.result, Some(serde_json::json!([renamed])));
    let history =
        complete(&reopened.submit(Operation::History { connection: connection.clone(), destination: renamed.clone() }, String::new()).unwrap());
    assert_eq!(history.phase, Phase::Ready);
    assert!(
        history.result.as_ref().unwrap()["revisions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["commit"] == first_commit && r["file"] == destination.file)
    );
    let restored = published(
        &reopened
            .submit(
                Operation::Manage {
                    connection: connection.clone(),
                    destination: renamed.clone(),
                    expected_revision: deleted.revision,
                    change: Change::Restore { commit: first_commit, file: destination.file },
                },
                String::new(),
            )
            .unwrap(),
    );
    assert_eq!(restored.phase, Phase::Synced, "{restored:?}");
    assert_eq!(repository.open_design(&renamed).unwrap().0.name, "Bracket");
    assert!(repository.list_deleted().unwrap().is_empty());
    assert_eq!(repository.list().unwrap(), vec![renamed.clone()]);
    let offline = fixture.0.join("unplugged checkout");
    fs::rename(checkout, &offline).unwrap();
    let queued = published(
        &reopened
            .submit(
                Operation::Manage {
                    connection: connection.clone(),
                    destination: renamed.clone(),
                    expected_revision: restored.revision,
                    change: Change::Delete,
                },
                String::new(),
            )
            .unwrap(),
    );
    assert!(queued.saved_locally);
    assert_eq!(queued.phase, Phase::SavedLocally);
    assert_eq!(fixture.outbox(&connection).pending().unwrap().len(), 1);
    let local_list = complete(&reopened.submit(Operation::List(connection.clone()), String::new()).unwrap());
    assert_eq!(local_list.result, Some(serde_json::json!([])));
    drop(reopened);
    fs::rename(offline, checkout).unwrap();
    let resumed = published(&fixture.host().submit(Operation::Synchronize(connection.clone()), String::new()).unwrap());
    assert_eq!(resumed.phase, Phase::Synced, "{resumed:?}");
    assert!(fixture.outbox(&connection).pending().unwrap().is_empty());
    assert_eq!(repository.list_deleted().unwrap(), vec![renamed]);
    assert_eq!(Fixture::git(checkout, &["show", ":staged.txt"]), stage);
    assert_eq!(fs::read_to_string(checkout.join("untracked.txt")).unwrap(), "unrelated working bytes");
}

#[test]
fn queued_management_can_be_discarded_without_losing_the_saved_design() {
    use solidcraft_cloud::management::Change;
    let fixture = Fixture::new();
    let connection = fixture.checkout("discard");
    let host = fixture.host();
    let capture = save(&connection, "parts/bracket", "Keep this design", None);
    let destination = capture.record.destination.clone();
    let saved = published(
        &host.submit(Operation::Save { connection: connection.clone(), save: Box::new(capture), synchronize: true }, String::new()).unwrap(),
    );
    assert_eq!(saved.phase, Phase::Synced);
    let checkout = Path::new(&connection.checkout);
    let occupied = checkout.join("Robot/parts/occupied/document.json");
    fs::create_dir_all(occupied.parent().unwrap()).unwrap();
    fs::write(&occupied, super::super::prepare_document(&solidcraft_doc::Document::new("Other document")).unwrap().document).unwrap();
    Fixture::git(checkout, &["add", "Robot/parts/occupied/document.json"]);
    Fixture::git(checkout, &["commit", "-m", "Keep an existing document at the target"]);
    let original = fs::read(&occupied).unwrap();
    let rename = published(
        &host
            .submit(
                Operation::Manage {
                    connection: connection.clone(),
                    destination: destination.clone(),
                    expected_revision: saved.revision,
                    change: Change::Rename { file: "parts/occupied".into() },
                },
                String::new(),
            )
            .unwrap(),
    );
    assert_eq!(rename.phase, Phase::SavedLocally, "{rename:?}");
    assert!(rename.saved_locally && rename.message.is_some());
    assert_eq!(fs::read(&occupied).unwrap(), original);
    let revision = rename.revision.unwrap();
    let discarded = published(
        &fixture
            .host()
            .submit(Operation::Discard { connection: connection.clone(), destination: destination.clone(), revision }, String::new())
            .unwrap(),
    );
    assert_eq!(discarded.phase, Phase::Synced, "{discarded:?}");
    assert!(fixture.outbox(&connection).pending().unwrap().is_empty());
    let repo = GitRepository::open(connection.clone(), "").unwrap();
    assert_eq!(repo.open_design(&destination).unwrap().0.name, "Keep this design");
    assert_eq!(fs::read(&occupied).unwrap(), original);

    let snapshot = repo.snapshot(&destination).unwrap();
    let parent = solidcraft_cloud::revision(&snapshot.manifest).map(str::to_owned);
    let snapshot = super::super::prepare_change(snapshot, &destination, &Change::Delete).unwrap();
    let staged = PendingSave::stage(&connection, "Robot", &destination.file, parent, snapshot).unwrap();
    let revision = staged.record.id.clone();
    fixture.outbox(&connection).stage(&staged).unwrap();
    repo.publish(&staged.record, fixture.outbox(&connection).recover(&staged.record).unwrap()).unwrap();
    let rejected = published(&host.submit(Operation::Discard { connection: connection.clone(), destination, revision }, String::new()).unwrap());
    assert_eq!(rejected.phase, Phase::Failed, "an already published action must be acknowledged, not discarded");
    assert_eq!(fixture.outbox(&connection).pending().unwrap().len(), 1);
    assert_eq!(published(&host.submit(Operation::Synchronize(connection.clone()), String::new()).unwrap()).phase, Phase::Synced);
    assert!(fixture.outbox(&connection).pending().unwrap().is_empty());
}

#[test]
fn saving_during_another_synchronization_waits_and_publishes_the_durable_capture() {
    let fixture = Fixture::new();
    let connection = fixture.checkout("overlapping-save");
    let host = fixture.host();
    let outbox = fixture.outbox(&connection);
    let lock = outbox.synchronization_lock().unwrap();
    let captured = save(&connection, "parts/bracket", "Saved during synchronization", None);
    let record = captured.record.clone();
    let job = host.submit(Operation::Save { connection: connection.clone(), save: Box::new(captured), synchronize: true }, String::new()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let status = job.status().unwrap();
        assert_ne!(status.phase, Phase::Failed, "an overlapping synchronization must not fail Save: {status:?}");
        if status.saved_locally {
            break;
        }
        assert!(Instant::now() < deadline, "the local capture must not wait for network synchronization");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(outbox.pending().unwrap().len(), 1);
    let recovered = outbox.recover_document(&record).unwrap();
    assert_eq!(
        super::super::open_document(&recovered.manifest, &recovered.document, |_| panic!("no assets")).unwrap().name,
        "Saved during synchronization"
    );
    std::thread::sleep(Duration::from_millis(50));
    let waiting = job.status().unwrap();
    assert_eq!(waiting.phase, Phase::Syncing, "a durable save must wait for the active publisher: {waiting:?}");
    assert!(waiting.message.is_none());
    let other = fixture.checkout("independent-publication");
    let independent = host
        .submit(
            Operation::Save { connection: other.clone(), save: Box::new(save(&other, "parts/other", "Independent design", None)), synchronize: true },
            String::new(),
        )
        .unwrap();
    assert_eq!(published(&independent).phase, Phase::Synced, "another repository must remain usable while this connection waits");
    drop(lock);
    let result = published(&job);
    assert_eq!(result.phase, Phase::Synced, "{result:?}");
    assert_eq!(result.revision.as_deref(), Some(record.id.as_str()));
    assert!(outbox.pending().unwrap().is_empty());
    assert_eq!(GitRepository::open(connection.clone(), "").unwrap().open_design(&record.destination).unwrap().0.name, "Saved during synchronization");
    assert_eq!(Fixture::git(Path::new(&connection.checkout), &["rev-list", "--count", "HEAD"]), b"2\n");
}

#[test]
fn cancelling_a_save_waiting_for_synchronization_retains_its_capture_for_retry() {
    let fixture = Fixture::new();
    let connection = fixture.checkout("cancel-overlap");
    let host = fixture.host();
    let outbox = fixture.outbox(&connection);
    let lock = outbox.synchronization_lock().unwrap();
    let captured = save(&connection, "parts/bracket", "Retained while waiting", None);
    let record = captured.record.clone();
    let job = host.submit(Operation::Save { connection: connection.clone(), save: Box::new(captured), synchronize: true }, String::new()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !job.status().unwrap().saved_locally {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    job.cancel().unwrap();
    drop(lock);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(job.status().unwrap().phase, Phase::SavedLocally);
    assert!(job.status().unwrap().message.is_none());
    assert_eq!(outbox.pending().unwrap()[0].id, record.id);
    assert_eq!(Fixture::git(Path::new(&connection.checkout), &["rev-list", "--count", "HEAD"]), b"1\n");
    let retry = host.submit(Operation::Synchronize(connection.clone()), String::new()).unwrap();
    assert_eq!(published(&retry).phase, Phase::Synced);
    assert!(outbox.pending().unwrap().is_empty());
    assert_eq!(GitRepository::open(connection, "").unwrap().open_design(&record.destination).unwrap().0.name, "Retained while waiting");
}

#[test]
fn completed_sync_releases_its_connection_before_the_next_operation() {
    let fixture = Fixture::new();
    let connection = fixture.connection("handoff");
    let host = fixture.host();
    let outbox = fixture.outbox(&connection);
    for _ in 0..4096 {
        let job = host.submit(Operation::Synchronize(connection.clone()), String::new()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let status = job.status().unwrap();
            if status.phase == Phase::Synced {
                let _lock = outbox.synchronization_lock().expect("a completed worker must release the connection before acknowledging completion");
                break;
            }
            assert!(matches!(status.phase, Phase::Working | Phase::Syncing), "{status:?}");
            assert!(Instant::now() < deadline, "{status:?}");
            std::thread::yield_now();
        }
    }
}

#[test]
fn connection_settings_survive_restart_and_future_settings_are_never_overwritten() {
    let fixture = Fixture::new();
    let host = fixture.host();
    let empty = complete(&host.submit(Operation::LoadSettings, String::new()).unwrap());
    assert_eq!(empty.phase, Phase::Ready);
    assert_eq!(empty.result.unwrap(), serde_json::json!(solidcraft_cloud::Settings::default()));
    let settings = solidcraft_cloud::Settings {
        connections: vec![fixture.connection("one"), fixture.connection("two")],
        default_connection: Some("two".into()),
        sync_after_save: false,
        ..Default::default()
    };
    assert_eq!(complete(&host.submit(Operation::Configure(settings.clone()), String::new()).unwrap()).phase, Phase::Synced);
    let reloaded = complete(&fixture.host().submit(Operation::LoadSettings, String::new()).unwrap());
    assert_eq!(reloaded.result.unwrap(), serde_json::json!(settings));
    let file = fixture.0.join("recovery/connections.json");
    let future = br#"{"format":"solidcraft-connections/999","settings":{}}"#;
    fs::write(&file, future).unwrap();
    let failed = complete(&fixture.host().submit(Operation::Configure(Default::default()), String::new()).unwrap());
    assert_eq!(failed.phase, Phase::Failed);
    assert!(failed.message.unwrap().contains("Upgrade SolidCraft"));
    assert_eq!(fs::read(file).unwrap(), future);
}

#[test]
fn restart_lists_and_opens_latest_offline_capture_without_a_checkout_or_git() {
    let fixture = Fixture::new();
    let connection = fixture.connection("offline");
    let host = fixture.host();
    let first = save(&connection, "parts/bracket", "First offline edit", None);
    let second = save(&connection, "parts/bracket", "Second offline edit", Some(first.record.id.clone()));
    let destination = second.record.destination.clone();
    let revision = second.record.id.clone();
    for capture in [first, second] {
        let job = host
            .submit(Operation::Save { connection: connection.clone(), save: Box::new(capture), synchronize: false }, "unavailable-git".into())
            .unwrap();
        let status = complete(&job);
        assert!(status.saved_locally);
        assert_eq!(status.phase, Phase::SavedLocally);
    }
    drop(host);
    let restarted = fixture.host();
    let listing = complete(&restarted.submit(Operation::List(connection.clone()), "unavailable-git".into()).unwrap());
    assert_eq!(listing.phase, Phase::Ready);
    assert_eq!(listing.result.unwrap(), serde_json::json!([destination]));
    let opening =
        restarted.submit(Operation::Open { connection: connection.clone(), destination, local_revision: None }, "unavailable-git".into()).unwrap();
    assert_eq!(complete(&opening).revision, Some(revision));
    let snapshot = opening.take_opened().unwrap();
    let document = super::super::open_document(&snapshot.manifest, &snapshot.document, |_| panic!("no assets")).unwrap();
    assert_eq!(document.name, "Second offline edit");
    let sync = restarted.submit(Operation::Synchronize(connection.clone()), "unavailable-git".into()).unwrap();
    assert_eq!(published(&sync).phase, Phase::Failed);
    sync.cancel().unwrap();
    assert_eq!(fixture.outbox(&connection).pending().unwrap().len(), 2);
}

#[test]
fn save_jobs_report_their_own_revision_and_restart_sync_is_idempotent() {
    let fixture = Fixture::new();
    let connection = fixture.checkout("work");
    let host = fixture.host();
    let older = save(&connection, "parts/other", "Other design", None);
    let current = save(&connection, "parts/current", "Requested design", None);
    let expected = current.record.clone();
    fixture.outbox(&connection).stage(&older).unwrap();
    let job = host.submit(Operation::Save { connection: connection.clone(), save: Box::new(current), synchronize: true }, String::new()).unwrap();
    let status = published(&job);
    assert_eq!(status.phase, Phase::Synced, "{status:?}");
    assert_eq!(status.revision.as_deref(), Some(expected.id.as_str()));
    assert_eq!(status.destination.as_ref(), Some(&expected.destination));
    assert_eq!(status.result.unwrap()["destination"], serde_json::json!(expected.destination));
    let sync = host.submit(Operation::Synchronize(connection.clone()), String::new()).unwrap();
    assert_eq!(published(&sync).phase, Phase::Synced);
    assert!(fixture.outbox(&connection).pending().unwrap().is_empty());
    let before = Fixture::git(Path::new(&connection.checkout), &["rev-parse", "HEAD"]);
    // A crash after remote confirmation but before queue removal retries the captured bytes.
    let current = save(&connection, "parts/current", "Requested design", None);
    fixture.outbox(&connection).stage(&current).unwrap();
    let sync = fixture.host().submit(Operation::Synchronize(connection.clone()), String::new()).unwrap();
    assert_eq!(published(&sync).phase, Phase::Synced);
    assert_eq!(before, Fixture::git(Path::new(&connection.checkout), &["rev-parse", "HEAD"]));
}

#[test]
fn confirmed_catalog_survives_lost_checkout_and_old_receipts_cannot_replace_newer_heads() {
    let fixture = Fixture::new();
    let connection = fixture.checkout("confirmed");
    let host = fixture.host();
    let first = save(&connection, "parts/bracket", "First publication", None);
    let first_record = first.record.clone();
    let second = save(&connection, "parts/bracket", "Latest publication", Some(first.record.id.clone()));
    let second_record = second.record.clone();
    for capture in [first, second] {
        let job = host.submit(Operation::Save { connection: connection.clone(), save: Box::new(capture), synchronize: true }, String::new()).unwrap();
        assert_eq!(published(&job).phase, Phase::Synced);
    }
    assert!(fixture.outbox(&connection).pending().unwrap().is_empty());
    fs::remove_dir_all(&connection.checkout).unwrap();
    drop(host);
    let restarted = fixture.host();
    let listing = complete(&restarted.submit(Operation::List(connection.clone()), "unavailable-git".into()).unwrap());
    assert_eq!(listing.phase, Phase::Ready, "{listing:?}");
    assert_eq!(listing.result.unwrap(), serde_json::json!([second_record.destination]));
    // Simulate restart after a receipt reached disk but queue removal was interrupted.
    fixture.outbox(&connection).stage(&save(&connection, "parts/bracket", "First publication", None)).unwrap();
    let sync = restarted.submit(Operation::Synchronize(connection.clone()), "unavailable-git".into()).unwrap();
    let status = published(&sync);
    assert_eq!(status.phase, Phase::Synced, "{status:?}");
    assert_eq!(status.publications.len(), 1);
    assert_eq!(status.publications[0].revision, first_record.id);
    assert!(fixture.outbox(&connection).pending().unwrap().is_empty());
    let opening = restarted
        .submit(
            Operation::Open { connection: connection.clone(), destination: second_record.destination.clone(), local_revision: None },
            "unavailable-git".into(),
        )
        .unwrap();
    let status = complete(&opening);
    assert_eq!(status.phase, Phase::Ready, "{status:?}");
    assert_eq!(status.revision.as_deref(), Some(second_record.id.as_str()));
    let snapshot = opening.take_opened().unwrap();
    assert_eq!(super::super::open_document(&snapshot.manifest, &snapshot.document, |_| panic!("no assets")).unwrap().name, "Latest publication");
    // Reusing the connection identifier for another repository cannot expose this catalog.
    let mut redirected = connection;
    redirected.repository = "different/repository".into();
    let listing = complete(&restarted.submit(Operation::List(redirected), "unavailable-git".into()).unwrap());
    assert_eq!(listing.phase, Phase::Failed);
    assert!(listing.message.unwrap().contains("original repository"));
}

#[test]
fn confirmed_conflict_copies_reopen_at_their_actual_destinations_after_restart() {
    let fixture = Fixture::new();
    let connection = fixture.checkout("conflicts");
    let host = fixture.host();
    let original = save(&connection, "parts/bracket", "Original design", None);
    let destination = original.record.destination.clone();
    let job = host.submit(Operation::Save { connection: connection.clone(), save: Box::new(original), synchronize: true }, String::new()).unwrap();
    assert_eq!(published(&job).phase, Phase::Synced);
    let competing = save(&connection, "parts/bracket", "Competing design", None);
    let competing_record = competing.record.clone();
    let job = host.submit(Operation::Save { connection: connection.clone(), save: Box::new(competing), synchronize: true }, String::new()).unwrap();
    let status = published(&job);
    assert_eq!(status.phase, Phase::Synced, "{status:?}");
    let publication = status.publications.first().unwrap().clone();
    assert!(publication.conflict);
    assert_ne!(publication.destination, destination);
    // Simulate interruption just before the last queue removal. Open/list must use the
    // receipt's conflict destination, not mistake this capture for the original local head.
    fixture.outbox(&connection).stage(&save(&connection, "parts/bracket", "Competing design", None)).unwrap();
    fs::remove_dir_all(&connection.checkout).unwrap();
    let restarted = fixture.host();
    let listing = complete(&restarted.submit(Operation::List(connection.clone()), "unavailable-git".into()).unwrap());
    assert_eq!(listing.phase, Phase::Ready);
    let entries: Vec<solidcraft_cloud::Destination> = serde_json::from_value(listing.result.unwrap()).unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries.contains(&destination));
    assert!(entries.contains(&publication.destination));
    for (destination, expected) in [(destination, "Original design"), (publication.destination.clone(), "Competing design")] {
        let opening = restarted
            .submit(Operation::Open { connection: connection.clone(), destination, local_revision: None }, "unavailable-git".into())
            .unwrap();
        assert_eq!(complete(&opening).phase, Phase::Ready);
        let snapshot = opening.take_opened().unwrap();
        assert_eq!(super::super::open_document(&snapshot.manifest, &snapshot.document, |_| panic!("no assets")).unwrap().name, expected);
    }
    let sync = restarted.submit(Operation::Synchronize(connection.clone()), "unavailable-git".into()).unwrap();
    let status = published(&sync);
    assert_eq!(status.phase, Phase::Synced);
    assert_eq!(status.publications[0].destination, publication.destination);
    assert_eq!(status.publications[0].revision, competing_record.id);
    assert!(fixture.outbox(&connection).pending().unwrap().is_empty());
}

#[test]
fn synchronization_pulls_peer_updates_with_an_empty_queue_and_preserves_unrelated_work() {
    let fixture = Fixture::new();
    let mut connection = fixture.checkout("pull");
    let saved_checkout = PathBuf::from(&connection.checkout);
    let checkout = saved_checkout.as_path();
    fs::write(checkout.join("notes.txt"), "Original notes\n").unwrap();
    fs::write(checkout.join("draft.txt"), "Original draft\n").unwrap();
    Fixture::git(checkout, &["add", "notes.txt", "draft.txt"]);
    Fixture::git(checkout, &["commit", "-m", "Unrelated documents"]);
    let bare = fixture.0.join("storage.git");
    Fixture::git(&fixture.0, &["clone", "--bare", &connection.checkout, bare.to_str().unwrap()]);
    Fixture::git(checkout, &["remote", "add", "origin", bare.to_str().unwrap()]);
    connection.remote = "origin".into();
    connection = GitRepository::open(connection, "").unwrap().verified_connection().unwrap();
    let first = save(&connection, "parts/bracket", "Original bracket", None);
    let destination = first.record.destination.clone();
    let first_id = first.record.id.clone();
    assert_eq!(
        published(
            &fixture
                .host()
                .submit(Operation::Save { connection: connection.clone(), save: Box::new(first), synchronize: true }, String::new())
                .unwrap()
        )
        .phase,
        Phase::Synced
    );
    let peer = fixture.0.join("peer");
    Fixture::git(&fixture.0, &["clone", bare.to_str().unwrap(), peer.to_str().unwrap()]);
    Fixture::git(&peer, &["config", "user.name", "Peer"]);
    Fixture::git(&peer, &["config", "user.email", "peer@example.invalid"]);
    Fixture::git(&peer, &["config", "commit.gpgSign", "false"]);
    let peer_connection = Connection { checkout: peer.to_string_lossy().into_owned(), ..connection.clone() };
    let edit = save(&peer_connection, "parts/bracket", "Peer bracket", Some(first_id));
    let revision = edit.record.id.clone();
    GitRepository::open(peer_connection, "").unwrap().publish(&edit.record, edit.record.recover(|p| Ok(edit.objects[p].clone())).unwrap()).unwrap();
    fs::write(checkout.join("notes.txt"), "Staged notes\n").unwrap();
    Fixture::git(checkout, &["add", "notes.txt"]);
    fs::write(checkout.join("draft.txt"), "Unstaged draft\n").unwrap();
    let before = Fixture::git(checkout, &["status", "--porcelain=v1", "-z"]);
    assert!(fixture.outbox(&connection).pending().unwrap().is_empty());
    let result = published(&fixture.host().submit(Operation::Synchronize(connection.clone()), String::new()).unwrap());
    assert_eq!(result.phase, Phase::Synced, "{result:?}");
    assert!(result.publications.is_empty(), "a download cannot invent a publication");
    assert_eq!(Fixture::git(checkout, &["status", "--porcelain=v1", "-z"]), before);
    assert_eq!(fs::read_to_string(checkout.join("notes.txt")).unwrap(), "Staged notes\n");
    assert_eq!(fs::read_to_string(checkout.join("draft.txt")).unwrap(), "Unstaged draft\n");
    let opening =
        fixture.host().submit(Operation::Open { connection: connection.clone(), destination, local_revision: None }, String::new()).unwrap();
    let status = complete(&opening);
    assert_eq!(status.phase, Phase::Ready);
    assert_eq!(status.revision, Some(revision));
    let snapshot = opening.take_opened().unwrap();
    assert_eq!(super::super::open_document(&snapshot.manifest, &snapshot.document, |_| panic!("no assets")).unwrap().name, "Peer bracket");
    let merge_head = checkout.join(".git/MERGE_HEAD");
    fs::write(&merge_head, Fixture::git(checkout, &["rev-parse", "HEAD"])).unwrap();
    assert_eq!(published(&fixture.host().submit(Operation::Synchronize(connection), String::new()).unwrap()).phase, Phase::Failed);
    assert_eq!(Fixture::git(checkout, &["status", "--porcelain=v1", "-z"]), before);
}

#[test]
fn offline_children_follow_a_confirmed_conflict_copy_across_restart_without_duplicate_revisions() {
    let fixture = Fixture::new();
    let connection = fixture.checkout("lineage");
    let host = fixture.host();
    let original = save(&connection, "parts/bracket", "External design", None);
    let original_destination = original.record.destination.clone();
    let original_job =
        host.submit(Operation::Save { connection: connection.clone(), save: Box::new(original), synchronize: true }, String::new()).unwrap();
    assert_eq!(published(&original_job).phase, Phase::Synced);
    let first = save(&connection, "parts/bracket", "Offline first", None);
    let first_id = first.record.id.clone();
    let child = save(&connection, "parts/bracket", "Offline child", Some(first_id.clone()));
    let child_record = child.record.clone();
    let grandchild = save(&connection, "parts/bracket", "Offline latest", Some(child_record.id.clone()));
    let latest_id = grandchild.record.id.clone();
    for capture in [first, child, grandchild] {
        let job =
            host.submit(Operation::Save { connection: connection.clone(), save: Box::new(capture), synchronize: false }, String::new()).unwrap();
        assert_eq!(complete(&job).phase, Phase::SavedLocally);
    }
    let status = published(&fixture.host().submit(Operation::Synchronize(connection.clone()), String::new()).unwrap());
    assert_eq!(status.phase, Phase::Synced, "{status:?}");
    assert_eq!(status.publications.len(), 3);
    let destination = status.publications[0].destination.clone();
    assert_ne!(destination, original_destination);
    assert!(status.publications.iter().all(|p| p.destination == destination && p.conflict));
    assert_eq!(status.publications[2].revision, latest_id);
    let repo = GitRepository::open(connection.clone(), "").unwrap();
    assert_eq!(repo.open_design(&original_destination).unwrap().0.name, "External design");
    assert_eq!(repo.open_design(&destination).unwrap().0.name, "Offline latest");
    let outbox = fixture.outbox(&connection);
    assert!(outbox.pending().unwrap().is_empty());
    let receipt = outbox.read(&format!("receipts/{}.json", child_record.id)).unwrap();
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&receipt).unwrap()["format"], "solidcraft-native-publication/2");
    // Queue removal was interrupted after the child receipt. Replay without a checkout;
    // retain the newer cache instead of rolling it back to that child's document.
    let replay = save(&connection, "parts/bracket", "Offline child", Some(first_id));
    outbox.stage(&replay).unwrap();
    let commits = Fixture::git(Path::new(&connection.checkout), &["rev-list", "--count", "HEAD"]);
    fs::rename(&connection.checkout, fixture.0.join("disconnected")).unwrap();
    let status = published(&fixture.host().submit(Operation::Synchronize(connection.clone()), "unavailable-git".into()).unwrap());
    assert_eq!(status.phase, Phase::Synced, "{status:?}");
    assert_eq!(status.publications[0].destination, destination);
    assert_eq!(outbox.cached(&connection, &destination).unwrap().unwrap().revision, Some(latest_id));
    assert_eq!(Fixture::git(&fixture.0.join("disconnected"), &["rev-list", "--count", "HEAD"]), commits);
}

#[test]
fn newer_or_corrupt_checkout_content_never_silently_opens_a_stale_cached_design() {
    let fixture = Fixture::new();
    let connection = fixture.checkout("schema");
    let host = fixture.host();
    let capture = save(&connection, "parts/bracket", "Cached design", None);
    let destination = capture.record.destination.clone();
    let job = host.submit(Operation::Save { connection: connection.clone(), save: Box::new(capture), synchronize: true }, String::new()).unwrap();
    assert_eq!(published(&job).phase, Phase::Synced);
    let path = Path::new(&connection.checkout).join("Robot/parts/bracket/document.json");
    for bytes in [br#"{"format":"solidcraft/4294967295"}"#.as_slice(), b"corrupt document"] {
        fs::write(&path, bytes).unwrap();
        let opening = host
            .submit(Operation::Open { connection: connection.clone(), destination: destination.clone(), local_revision: None }, String::new())
            .unwrap();
        let status = complete(&opening);
        assert_eq!(status.phase, Phase::Failed, "{status:?}");
        if bytes.starts_with(b"{") {
            assert!(status.message.unwrap().contains("Upgrade SolidCraft"));
        }
        assert!(opening.take_opened().is_err());
    }
    let marker = Path::new(&connection.checkout).join("Robot/solidcraft-project.json");
    fs::write(marker, br#"{"format":"solidcraft-cloud/999"}"#).unwrap();
    let listing = complete(&host.submit(Operation::List(connection.clone()), String::new()).unwrap());
    assert_eq!(listing.phase, Phase::Failed);
    assert!(listing.message.unwrap().contains("Upgrade SolidCraft"));
}

#[test]
fn a_busy_or_damaged_connection_cannot_block_other_queues_or_local_saves() {
    let fixture = Fixture::new();
    let busy = fixture.connection("busy");
    let healthy = fixture.checkout("healthy");
    let host = fixture.host();
    let outbox = fixture.outbox(&busy);
    let held = outbox.synchronization_lock().unwrap();
    let local = save(&busy, "parts/offline", "Retained while syncing", None);
    let waiting =
        host.submit(Operation::Save { connection: busy.clone(), save: Box::new(local), synchronize: true }, "unavailable-git".into()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !waiting.status().unwrap().saved_locally {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(waiting.status().unwrap().phase, Phase::Syncing);
    assert_eq!(outbox.pending().unwrap().len(), 1);
    let capture = save(&healthy, "parts/working", "Independent connection", None);
    let job = host.submit(Operation::Save { connection: healthy.clone(), save: Box::new(capture), synchronize: true }, String::new()).unwrap();
    assert_eq!(published(&job).phase, Phase::Synced);
    assert!(fixture.outbox(&healthy).pending().unwrap().is_empty());
    // Cancelling the busy publisher retains its capture while the healthy one completes.
    // The damaged queue remains independently detectable on an explicit retry.
    waiting.cancel().unwrap();
    drop(held);
    fs::write(outbox.root().join("pending/future.json"), br#"{"format":"solidcraft-outbox/999"}"#).unwrap();
    let sync = host.submit(Operation::Synchronize(busy), String::new()).unwrap();
    assert!(published(&sync).message.unwrap().contains("Upgrade SolidCraft"));
}

#[test]
fn competing_offline_captures_remain_available_instead_of_opening_an_arbitrary_tip() {
    let fixture = Fixture::new();
    let connection = fixture.connection("offline");
    let first = save(&connection, "parts/bracket", "Device one", None);
    let second = save(&connection, "parts/bracket", "Device two", None);
    let destination = first.record.destination.clone();
    let outbox = fixture.outbox(&connection);
    outbox.stage(&first).unwrap();
    outbox.stage(&second).unwrap();
    let job = fixture
        .host()
        .submit(Operation::Open { connection: connection.clone(), destination: destination.clone(), local_revision: None }, String::new())
        .unwrap();
    let status = complete(&job);
    assert_eq!(status.phase, Phase::Failed);
    assert!(status.message.unwrap().contains("changed elsewhere"));
    assert!(job.take_opened().is_err());
    assert_eq!(outbox.pending().unwrap().len(), 2);
    let _: Snapshot = outbox.recover(&first.record).unwrap();
    outbox.recover(&second.record).unwrap();
    for (save, name) in [(&first, "Device one"), (&second, "Device two")] {
        let job = fixture
            .host()
            .submit(
                Operation::Open { connection: connection.clone(), destination: destination.clone(), local_revision: Some(save.record.id.clone()) },
                "unavailable-git".into(),
            )
            .unwrap();
        let status = complete(&job);
        assert_eq!(status.phase, Phase::Ready, "{status:?}");
        assert_eq!(status.revision, Some(save.record.id.clone()));
        let snapshot = job.take_opened().unwrap();
        assert_eq!(super::super::open_document(&snapshot.manifest, &snapshot.document, |_| panic!("no assets")).unwrap().name, name);
    }
    let job =
        fixture.host().submit(Operation::Open { connection, destination, local_revision: Some("f".repeat(64)) }, "unavailable-git".into()).unwrap();
    assert_eq!(complete(&job).phase, Phase::Failed);
    assert!(job.take_opened().is_err());
    assert_eq!(outbox.pending().unwrap().len(), 2, "inspection never discards captures");
}

#[test]
fn asynchronous_git_operations_retain_the_callers_allowed_folders() {
    let fixture = Fixture::new();
    let connection = fixture.checkout("outside");
    let host = fixture.host();
    let scope = crate::vfs::PathScope::new([fixture.0.join("recovery")]);
    let job = crate::vfs::scoped(Some(scope), || host.submit(Operation::Test(connection), String::new())).unwrap();
    let status = complete(&job);
    assert_eq!(status.phase, Phase::Failed);
    assert!(status.message.unwrap().contains("outside"));
}

#[test]
fn guided_checkout_setup_retries_and_pushes_without_git_identity_configuration() {
    struct Access {
        connection: Connection,
        secrets: std::sync::Mutex<BTreeMap<String, String>>,
    }
    impl super::super::NativeAccess for Access {
        fn credential(&self, _: &str) -> Result<Option<String>> {
            Ok(None)
        }
        fn store_credential(&self, id: &str, token: &str) -> Result<()> {
            self.secrets.lock().unwrap().insert(id.into(), token.into());
            Ok(())
        }
        fn prepare(&self, mut connection: Connection, _: &Job) -> Result<Connection> {
            connection.id = self.connection.id.clone();
            connection.remote = self.connection.remote.clone();
            connection.checkout.clear();
            Ok(connection)
        }
    }
    let fixture = Fixture::new();
    let source = fixture.checkout("source");
    let bare = fixture.0.join("remote with spaces.git");
    Fixture::git(&fixture.0, &["clone", "--bare", &source.checkout, bare.to_str().unwrap()]);
    let connection =
        Connection { id: "guided".into(), remote: bare.to_string_lossy().into_owned(), repository: "owner/designs".into(), ..Default::default() };
    let access = std::sync::Arc::new(Access { connection: connection.clone(), secrets: Default::default() });
    let host = fixture.host().with_access(access.clone());
    assert_eq!(complete(&host.store_credential("test-id".into(), "test-token".into()).unwrap()).phase, Phase::Ready);
    assert_eq!(access.secrets.lock().unwrap().get("test-id").unwrap(), "test-token");
    let requested = Connection { id: "github-automatic".into(), ..connection.clone() };
    let result = complete(&host.submit(Operation::Setup(requested.clone()), String::new()).unwrap());
    assert_eq!(result.phase, Phase::Ready, "{:?}", result.message);
    let prepared: Connection = serde_json::from_value(result.result.unwrap()).unwrap();
    let repeated = complete(&host.submit(Operation::Setup(prepared.clone()), String::new()).unwrap());
    assert_eq!(repeated.phase, Phase::Ready, "{:?}", repeated.message);
    assert_eq!(prepared, serde_json::from_value::<Connection>(repeated.result.unwrap()).unwrap());
    let other = Connection { id: "another-account".into(), ..connection };
    let other_host = fixture.host().with_access(std::sync::Arc::new(Access { connection: other, secrets: Default::default() }));
    let other = complete(&other_host.submit(Operation::Setup(requested), String::new()).unwrap());
    assert_eq!(other.phase, Phase::Ready, "{other:?}");
    let other: Connection = serde_json::from_value(other.result.unwrap()).unwrap();
    assert_ne!(prepared.checkout, other.checkout, "automatic account discovery must keep checkouts isolated");
    // Empty local identity overrides any machine-wide identity without changing it.
    Fixture::git(Path::new(&prepared.checkout), &["config", "user.name", ""]);
    Fixture::git(Path::new(&prepared.checkout), &["config", "user.email", ""]);
    Fixture::git(Path::new(&prepared.checkout), &["config", "commit.gpgSign", "false"]);
    let capture = save(&prepared, "parts/first", "First cloud save", None);
    let status =
        complete(&host.submit(Operation::Save { connection: prepared.clone(), save: Box::new(capture), synchronize: true }, String::new()).unwrap());
    assert_eq!(status.phase, Phase::Synced, "{:?}", status.message);
    let reopened = fixture.0.join("ordinary checkout");
    Fixture::git(&fixture.0, &["clone", bare.to_str().unwrap(), reopened.to_str().unwrap()]);
    let destination = status.destination.unwrap();
    let repo = GitRepository::open(Connection { checkout: reopened.to_string_lossy().into_owned(), ..prepared.clone() }, "").unwrap();
    assert_eq!(repo.open_design(&destination).unwrap().0.name, "First cloud save");
    assert_eq!(Fixture::git(Path::new(&prepared.checkout), &["config", "--get", "user.name"]), b"\n");
}

#[test]
fn externally_published_checkouts_are_cached_without_receipts_and_survive_lost_checkout() {
    let fixture = Fixture::new();
    let connection = fixture.checkout("external");
    let capture = save(&connection, "parts/bracket", "External bracket", None);
    let destination = capture.record.destination.clone();
    let repo = GitRepository::open(connection.clone(), "").unwrap();
    let publication = repo.publish(&capture.record, capture.record.recover(|p| Ok(capture.objects[p].clone())).unwrap()).unwrap();
    let before = Fixture::git(Path::new(&connection.checkout), &["rev-parse", "HEAD"]);
    let manifest_path = Path::new(&connection.checkout).join("Robot/parts/bracket/manifest.json");
    let manifest = fs::read(&manifest_path).unwrap();
    let opening = fixture
        .host()
        .submit(Operation::Open { connection: connection.clone(), destination: destination.clone(), local_revision: None }, String::new())
        .unwrap();
    assert_eq!(complete(&opening).phase, Phase::Ready);
    assert_eq!(complete(&opening).revision.as_deref(), Some(publication.revision.as_str()));
    let snapshot = opening.take_opened().unwrap();
    assert_eq!(super::super::open_document(&snapshot.manifest, &snapshot.document, |_| Err(storage("no assets"))).unwrap().name, "External bracket");
    assert_eq!(fs::read(&manifest_path).unwrap(), manifest);
    assert_eq!(Fixture::git(Path::new(&connection.checkout), &["rev-parse", "HEAD"]), before);
    assert!(fixture.outbox(&connection).pending().unwrap().is_empty());
    assert!(fixture.outbox(&connection).receipt(&capture.record).unwrap().is_none());
    fs::remove_dir_all(&connection.checkout).unwrap();
    let host = fixture.host();
    let list = complete(&host.submit(Operation::List(connection.clone()), "missing-git-executable".into()).unwrap());
    assert_eq!(list.phase, Phase::Ready);
    assert_eq!(list.catalog.len(), 1);
    assert_eq!(list.catalog[0].phase, Phase::Ready);
    let opening =
        host.submit(Operation::Open { connection: connection.clone(), destination, local_revision: None }, "missing-git-executable".into()).unwrap();
    assert_eq!(complete(&opening).phase, Phase::Ready);
    let recovered = opening.take_opened().unwrap();
    assert_eq!(recovered.document, snapshot.document);
}

#[test]
fn synchronization_pulls_outdated_caches_and_preserves_unrelated_git_work() {
    let fixture = Fixture::new();
    let connection = fixture.checkout("pull-cached");
    let host = fixture.host();
    let first = save(&connection, "parts/bracket", "First version", None);
    let record = first.record.clone();
    assert_eq!(
        published(&host.submit(Operation::Save { connection: connection.clone(), save: Box::new(first), synchronize: true }, String::new()).unwrap())
            .phase,
        Phase::Synced
    );
    let changed = save(&connection, "parts/bracket", "New repository version", Some(record.id));
    GitRepository::open(connection.clone(), "")
        .unwrap()
        .publish(&changed.record, changed.record.recover(|p| Ok(changed.objects[p].clone())).unwrap())
        .unwrap();
    let listing = complete(&host.submit(Operation::List(connection.clone()), String::new()).unwrap());
    assert!(listing.catalog[0].outdated);
    let offline = complete(&fixture.host().submit(Operation::List(connection.clone()), "missing-git".into()).unwrap());
    assert!(offline.catalog[0].outdated, "observed staleness survives restart and offline listing");
    let checkout = Path::new(&connection.checkout);
    fs::write(checkout.join("notes.txt"), b"unrelated staged").unwrap();
    Fixture::git(checkout, &["add", "notes.txt"]);
    fs::write(checkout.join("scratch.txt"), b"unrelated unstaged").unwrap();
    let sync = host.submit(Operation::Synchronize(connection.clone()), String::new()).unwrap();
    assert_eq!(published(&sync).phase, Phase::Synced);
    let listing = complete(&host.submit(Operation::List(connection.clone()), String::new()).unwrap());
    assert!(!listing.catalog[0].outdated);
    assert_eq!(listing.catalog[0].phase, Phase::Synced);
    assert_eq!(listing.catalog[0].revision.as_deref(), Some(changed.record.id.as_str()));
    assert_eq!(Fixture::git(checkout, &["diff", "--cached", "--name-only"]), b"notes.txt\n");
    assert_eq!(fs::read(checkout.join("scratch.txt")).unwrap(), b"unrelated unstaged");
}

#[test]
fn changed_checkout_is_not_marked_synced_and_cache_failures_never_open_an_older_head() {
    let fixture = Fixture::new();
    let connection = fixture.checkout("observed");
    let capture = save(&connection, "parts/bracket", "Published head", None);
    let record = capture.record.clone();
    let destination = record.destination.clone();
    let host = fixture.host();
    assert_eq!(
        published(
            &host.submit(Operation::Save { connection: connection.clone(), save: Box::new(capture), synchronize: true }, String::new()).unwrap()
        )
        .phase,
        Phase::Synced
    );
    let list = complete(&host.submit(Operation::List(connection.clone()), String::new()).unwrap());
    assert_eq!(list.catalog[0].phase, Phase::Synced);
    let changed = save(&connection, "parts/bracket", "Changed externally", Some(record.id));
    GitRepository::open(connection.clone(), "")
        .unwrap()
        .publish(&changed.record, changed.record.recover(|p| Ok(changed.objects[p].clone())).unwrap())
        .unwrap();
    let list = complete(&host.submit(Operation::List(connection.clone()), String::new()).unwrap());
    assert_eq!(list.catalog[0].phase, Phase::Ready);
    let outbox = fixture.outbox(&connection);
    let held = fs::OpenOptions::new().write(true).create(true).truncate(false).open(outbox.path("catalog.lock").unwrap()).unwrap();
    held.try_lock().unwrap();
    let blocked = host
        .submit(Operation::Open { connection: connection.clone(), destination: destination.clone(), local_revision: None }, String::new())
        .unwrap();
    assert_eq!(complete(&blocked).phase, Phase::Failed);
    assert!(complete(&blocked).message.unwrap().contains("catalog"));
    assert!(blocked.take_opened().is_err());
    drop(held);
    let opening = host
        .submit(Operation::Open { connection: connection.clone(), destination: destination.clone(), local_revision: None }, String::new())
        .unwrap();
    assert_eq!(complete(&opening).phase, Phase::Ready);
    let latest = opening.take_opened().unwrap();
    assert_eq!(super::super::open_document(&latest.manifest, &latest.document, |_| Err(storage("no assets"))).unwrap().name, "Changed externally");
    let cached = outbox.cached(&connection, &destination).unwrap().unwrap();
    assert!(!cached.confirmed);
    let cache_path = outbox.path(&format!("cached/{}.json", solidcraft_cloud::digest(&serde_json::to_vec(&destination).unwrap()))).unwrap();
    let mut future: serde_json::Value = serde_json::from_slice(&fs::read(&cache_path).unwrap()).unwrap();
    future["format"] = "solidcraft-cache/999".into();
    let future = serde_json::to_vec(&future).unwrap();
    fs::write(&cache_path, &future).unwrap();
    let opening = host.submit(Operation::Open { connection, destination, local_revision: None }, String::new()).unwrap();
    assert_eq!(complete(&opening).phase, Phase::Failed);
    assert!(complete(&opening).message.unwrap().contains("Upgrade SolidCraft"));
    assert_eq!(fs::read(cache_path).unwrap(), future);
}

#[test]
fn previews_do_not_read_imports_and_a_missing_preview_does_not_block_opening() {
    let fixture = Fixture::new();
    let connection = fixture.connection("preview");
    let mut doc = solidcraft_doc::Document::new("Preview import");
    doc.add_feature(
        solidcraft_doc::FeatureKind::Import {
            file: "source.step".into(),
            step: "ISO-10303-21;\nEND-ISO-10303-21;".into(),
            components: Vec::new(),
            skip: Vec::new(),
        },
        None,
    )
    .unwrap();
    let mut snapshot = super::super::prepare_document(&doc).unwrap();
    let png = include_bytes!("../../../../../assets/samples/exports/0.png").to_vec();
    solidcraft_cloud::attach_thumbnail(&mut snapshot, png.clone()).unwrap();
    let capture = PendingSave::stage(&connection, "Robot", "parts/bracket", None, snapshot).unwrap();
    let source = capture.record.manifest.assets.values().next().unwrap().chunks.first().unwrap().path().unwrap();
    let source_bytes = capture.objects[&source].clone();
    let picture = solidcraft_cloud::Thumbnail::read(&capture.record.manifest).unwrap().unwrap().paths().unwrap().pop().unwrap();
    let destination = capture.record.destination.clone();
    let host = fixture.host();
    assert!(
        complete(
            &host.submit(Operation::Save { connection: connection.clone(), save: Box::new(capture), synchronize: false }, String::new()).unwrap()
        )
        .saved_locally
    );
    let outbox = fixture.outbox(&connection);
    fs::remove_file(outbox.path(&source).unwrap()).unwrap();
    let preview =
        complete(&host.submit(Operation::Thumbnail { connection: connection.clone(), destination: destination.clone() }, String::new()).unwrap());
    assert_eq!(preview.phase, Phase::Ready);
    assert_eq!(solidcraft_cloud::Thumbnail::decode_response(&preview.result.unwrap()).unwrap(), Some(png));
    fs::write(outbox.path(&source).unwrap(), source_bytes).unwrap();
    fs::remove_file(outbox.path(&picture).unwrap()).unwrap();
    let opening = host.submit(Operation::Open { connection, destination, local_revision: None }, String::new()).unwrap();
    assert_eq!(complete(&opening).phase, Phase::Ready);
    assert_eq!(opening.take_opened().unwrap().manifest.assets.len(), 1);
}
