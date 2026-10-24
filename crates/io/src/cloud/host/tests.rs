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
    let opening = restarted.submit(Operation::Open { connection: connection.clone(), destination }, "unavailable-git".into()).unwrap();
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
        .submit(Operation::Open { connection: connection.clone(), destination: second_record.destination.clone() }, "unavailable-git".into())
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
        let opening = restarted.submit(Operation::Open { connection: connection.clone(), destination }, "unavailable-git".into()).unwrap();
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
        let opening = host.submit(Operation::Open { connection: connection.clone(), destination: destination.clone() }, String::new()).unwrap();
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
    let job = host.submit(Operation::Save { connection: busy.clone(), save: Box::new(local), synchronize: true }, "unavailable-git".into()).unwrap();
    let status = published(&job);
    assert_eq!(status.phase, Phase::SavedLocally);
    assert!(status.saved_locally);
    assert_eq!(outbox.pending().unwrap().len(), 1);
    drop(held);
    fs::write(outbox.root().join("pending/future.json"), br#"{"format":"solidcraft-outbox/999"}"#).unwrap();
    let capture = save(&healthy, "parts/working", "Independent connection", None);
    let job = host.submit(Operation::Save { connection: healthy.clone(), save: Box::new(capture), synchronize: true }, String::new()).unwrap();
    assert_eq!(published(&job).phase, Phase::Synced);
    assert!(fixture.outbox(&healthy).pending().unwrap().is_empty());
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
    let job = fixture.host().submit(Operation::Open { connection, destination }, String::new()).unwrap();
    let status = complete(&job);
    assert_eq!(status.phase, Phase::Failed);
    assert!(status.message.unwrap().contains("changed elsewhere"));
    assert!(job.take_opened().is_err());
    assert_eq!(outbox.pending().unwrap().len(), 2);
    let _: Snapshot = outbox.recover(&first.record).unwrap();
    outbox.recover(&second.record).unwrap();
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
    let result = complete(&host.submit(Operation::Setup(connection), String::new()).unwrap());
    assert_eq!(result.phase, Phase::Ready, "{:?}", result.message);
    let prepared: Connection = serde_json::from_value(result.result.unwrap()).unwrap();
    let repeated = complete(&host.submit(Operation::Setup(prepared.clone()), String::new()).unwrap());
    assert_eq!(repeated.phase, Phase::Ready, "{:?}", repeated.message);
    assert_eq!(prepared, serde_json::from_value::<Connection>(repeated.result.unwrap()).unwrap());
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
