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
