use super::*;
use serde_json::json;
use solidcraft_cloud::{Connection, Snapshot, jobs::Operation};
use std::sync::Mutex;

#[derive(Default)]
struct MockHost(Mutex<Vec<(Job, Operation)>>);

impl Host for MockHost {
    fn submit(&self, operation: Operation, _: String) -> solidcraft_cloud::Result<Job> {
        let connection = match &operation {
            Operation::Configure(settings) => {
                let job = Job::new(String::new())?;
                job.update(|status| {
                    status.result = Some(json!(settings));
                    status.phase = Phase::Synced;
                })?;
                return Ok(job);
            }
            Operation::GenerateSshKey(_) => String::new(),
            Operation::LoadSettings => return Err(solidcraft_cloud::Error::Storage("no stored settings".into())),
            Operation::Setup(connection)
            | Operation::Test(connection)
            | Operation::List(connection)
            | Operation::Synchronize(connection)
            | Operation::Open { connection, .. }
            | Operation::Save { connection, .. } => connection.id.clone(),
        };
        let job = Job::new(connection)?;
        self.0.lock().unwrap().push((job.clone(), operation));
        Ok(job)
    }
}

impl MockHost {
    fn job(&self, index: usize) -> Job {
        self.0.lock().unwrap()[index].0.clone()
    }

    fn saved(&self, index: usize) {
        let records = self.0.lock().unwrap();
        let (job, Operation::Save { save, .. }) = &records[index] else { panic!("expected captured save") };
        save.validate().unwrap();
        job.update(|status| {
            status.saved_locally = true;
            status.phase = Phase::SavedLocally;
            status.destination = Some(save.record.destination.clone());
            status.revision = Some(save.record.id.clone());
        })
        .unwrap();
    }
}

fn configured() -> (Session, Arc<MockHost>, Destination) {
    let mut session = Session::default();
    let host = Arc::new(MockHost::default());
    session.set_cloud_host(Some(host.clone()));
    session
        .execute(
            "cloud.configure",
            &json!({"settings":{
                "connections":[Connection::default()],"default_connection":"github","sync_after_save":false
            }}),
        )
        .unwrap();
    let destination = Destination { connection: "github".into(), project: "Robot".into(), file: "parts/bracket".into() };
    (session, host, destination)
}

#[test]
fn a_proposed_connection_can_be_tested_without_changing_saved_preferences() {
    let (mut session, host, _) = configured();
    let before = session.cloud_settings().clone();
    let proposed = Connection {
        id: "git".into(),
        provider: String::new(),
        checkout: "/example/checkout".into(),
        credential: String::new(),
        ..Default::default()
    };
    session.execute("cloud.test", &json!({"proposed":proposed, "git_executable":"/example/git"})).unwrap();
    assert_eq!(session.cloud_settings(), &before);
    let jobs = host.0.lock().unwrap();
    let (_, Operation::Test(actual)) = jobs.last().unwrap() else { panic!("connection test") };
    assert_eq!(actual, &proposed);
    drop(jobs);
    assert!(session.execute("cloud.test", &json!({"connection":"github","proposed":proposed})).is_err());
    assert!(session.execute("cloud.test", &json!({"proposed":proposed,"git_executable":"git\nother"})).is_err());
    assert_eq!(session.cloud_settings(), &before);
}

#[test]
fn configuration_is_adopted_after_storage_and_failure_keeps_previous_settings() {
    struct SettingsHost(Mutex<Vec<Job>>);
    impl Host for SettingsHost {
        fn submit(&self, _: Operation, _: String) -> solidcraft_cloud::Result<Job> {
            let job = Job::new(String::new())?;
            self.0.lock().unwrap().push(job.clone());
            Ok(job)
        }
    }
    let host = Arc::new(SettingsHost(Mutex::new(Vec::new())));
    let mut session = Session::default();
    session.set_cloud_host(Some(host.clone()));
    let requested = Settings { connections: vec![Connection::default()], ..Default::default() };
    let job = session.execute("cloud.configure", &json!({"settings":requested})).unwrap();
    assert!(session.cloud_configuring());
    assert!(session.cloud_settings().connections.is_empty());
    assert!(session.execute("cloud.configure", &json!({"settings":requested})).is_err());
    assert!(session.execute("cloud.list", &json!({"connection":"github"})).is_err());
    host.0.lock().unwrap()[0]
        .update(|s| {
            s.result = Some(json!(requested));
            s.phase = Phase::Synced;
        })
        .unwrap();
    session.execute("cloud.status", &json!({"operation":job["operation"]})).unwrap();
    assert!(!session.cloud_configuring());
    assert_eq!(session.take_cloud_settings_change(), Some(requested.clone()));
    session.execute("cloud.configure", &json!({"settings":Settings::default()})).unwrap();
    host.0.lock().unwrap()[1].fail(solidcraft_cloud::Error::Storage("disk full".into())).unwrap();
    session.poll_cloud().unwrap();
    assert_eq!(session.cloud_settings(), &requested);
    assert_eq!(session.take_cloud_settings_change(), None);
    session.execute("cloud.settings", &json!({"reload":true})).unwrap();
    host.0.lock().unwrap()[2]
        .update(|s| {
            s.result = Some(json!(Settings::default()));
            s.phase = Phase::Ready;
        })
        .unwrap();
    session.poll_cloud().unwrap();
    assert!(session.cloud_settings().connections.is_empty());
}

#[test]
fn cloud_capture_becomes_the_saved_baseline_without_losing_later_edits() {
    let (mut session, host, destination) = configured();
    session.doc_mut().name = "Captured revision".into();
    let submitted = session.execute("cloud.save", &json!({"destination":destination})).unwrap();
    assert!(submitted["operation"].is_string());
    assert!(session.is_dirty(), "an operation id does not acknowledge durability");
    host.saved(0);
    host.job(0).update(|status| status.phase = Phase::Syncing).unwrap();
    session.doc_mut().name = "Edited during upload".into();
    let status = session.execute("cloud.status", &json!({"operation":submitted["operation"]})).unwrap();
    assert_eq!(status["phase"], "syncing");
    assert!(session.is_dirty());
    assert_eq!(session.doc.name, "Edited during upload");
    let records = host.0.lock().unwrap();
    let Operation::Save { save, synchronize, .. } = &records[0].1 else { panic!("expected save") };
    assert!(!synchronize);
    let snapshot = save.record.recover(|path| Ok(save.objects[path].clone())).unwrap();
    let captured = solidcraft_io::cloud::open_document(&snapshot.manifest, &snapshot.document, |path| Ok(snapshot.objects[path].clone())).unwrap();
    assert_eq!(captured.name, "Captured revision");
    drop(records);
    session.doc_mut().name = "Captured revision".into();
    assert!(!session.is_dirty(), "the acknowledged baseline is the captured document");
}

#[test]
fn unchanged_cloud_save_keeps_the_revision_and_submits_no_work() {
    for phase in [Phase::SavedLocally, Phase::Syncing, Phase::Synced] {
        let (mut session, host, destination) = configured();
        session.execute("cloud.save", &json!({"destination":destination})).unwrap();
        host.saved(0);
        host.job(0).update(|status| status.phase = phase).unwrap();
        session.poll_cloud().unwrap();
        assert!(!session.is_dirty());
        let revision = session.cloud_binding().unwrap().revision.clone();
        for (command, params) in [("file.save", json!({})), ("cloud.save", json!({})), ("cloud.save", json!({"destination":destination}))] {
            let result = session.execute(command, &params).unwrap();
            assert_eq!(result["unchanged"], true);
            assert!(result.get("operation").is_none());
            assert_eq!(host.0.lock().unwrap().len(), 1, "{command} must not submit another save for an unchanged {phase:?} document");
            assert_eq!(session.cloud_binding().unwrap().revision, revision);
            assert!(!session.cloud_save_pending());
            assert!(!session.is_dirty());
        }
    }
}

#[test]
fn ordinary_save_uses_the_bound_destination_and_previous_local_revision() {
    let (mut session, host, destination) = configured();
    session.execute("cloud.save", &json!({"destination":destination})).unwrap();
    assert!(session.execute("cloud.save", &json!({"destination":destination})).is_err());
    assert_eq!(host.0.lock().unwrap().len(), 1);
    host.saved(0);
    session.poll_cloud().unwrap();
    let parent = session.cloud_binding().unwrap().revision.clone();
    session.doc_mut().name = "Next local edit".into();
    session.execute("file.save", &json!({})).unwrap();
    let records = host.0.lock().unwrap();
    let Operation::Save { save, .. } = &records[1].1 else { panic!("expected save") };
    assert_eq!(save.record.destination, destination);
    assert_eq!(save.record.expected_revision, parent);
}

#[test]
fn confirmed_conflict_copy_redirects_only_its_matching_binding() {
    let (mut session, host, destination) = configured();
    session.execute("cloud.save", &json!({"destination":destination})).unwrap();
    host.saved(0);
    session.poll_cloud().unwrap();
    let conflict = Destination { file: "conflicts/parts/bracket-copy".into(), ..destination.clone() };
    host.job(0)
        .update(|status| {
            status.destination = Some(conflict.clone());
            status.phase = Phase::Synced;
        })
        .unwrap();
    session.poll_cloud().unwrap();
    assert_eq!(session.cloud_binding().unwrap().destination, conflict);
    assert!(!session.is_dirty());
    session.doc_mut().name = "Continue conflict copy".into();
    session.execute("file.save", &json!({})).unwrap();
    let records = host.0.lock().unwrap();
    let Operation::Save { save, .. } = &records[1].1 else { panic!("expected save") };
    assert_eq!(save.record.destination, conflict);
    drop(records);
    host.saved(1);
    session.poll_cloud().unwrap();
    assert_eq!(session.cloud_binding().unwrap().destination, conflict);

    // A different local revision must not follow a delayed result from an earlier save.
    let (mut session, host, destination) = configured();
    session.execute("cloud.save", &json!({"destination":destination})).unwrap();
    host.saved(0);
    session.poll_cloud().unwrap();
    session.doc_mut().name = "Newer queued edit".into();
    session.execute("file.save", &json!({})).unwrap();
    host.saved(1);
    session.poll_cloud().unwrap();
    host.job(0)
        .update(|status| {
            status.destination = Some(conflict);
            status.phase = Phase::Synced;
        })
        .unwrap();
    session.poll_cloud().unwrap();
    assert_eq!(session.cloud_binding().unwrap().destination, destination);
}

#[test]
fn partial_synchronization_confirms_an_older_offline_save_and_its_conflict_destination() {
    let (mut session, host, destination) = configured();
    session.execute("cloud.save", &json!({"destination":destination})).unwrap();
    host.saved(0);
    session.poll_cloud().unwrap();
    let revision = session.cloud_binding().unwrap().revision.clone().unwrap();
    let conflict = Destination { file: "conflicts/parts/bracket-copy".into(), ..destination };
    session.execute("cloud.sync", &json!({})).unwrap();
    host.job(1)
        .update(|status| {
            status.publications.push(solidcraft_cloud::Published { destination: conflict.clone(), revision, commit: "a".repeat(40), conflict: true });
            status.phase = Phase::Failed;
            status.message = Some("Another queued file could not be published".into());
        })
        .unwrap();
    session.poll_cloud().unwrap();
    assert_eq!(host.job(0).status().unwrap().phase, Phase::Synced);
    assert_eq!(session.cloud_binding().unwrap().destination, conflict);
    assert!(!session.is_dirty());
    assert_eq!(host.job(1).status().unwrap().phase, Phase::Failed);
}

#[test]
fn offline_operation_history_does_not_limit_the_number_of_durable_saves() {
    let (mut session, host, destination) = configured();
    for index in 0..140 {
        session.doc_mut().name = format!("Offline revision {index}");
        session.execute("cloud.save", &json!({"destination":destination})).unwrap();
        host.saved(index);
        session.poll_cloud().unwrap();
    }
    assert_eq!(host.0.lock().unwrap().len(), 140, "durable records belong to the host, independently of status history");
    assert_eq!(session.cloud_operations().unwrap().len(), 128);
}

#[test]
fn replacing_a_design_keeps_connections_and_operations_but_cannot_adopt_an_old_capture() {
    let (mut session, host, destination) = configured();
    let before = session.cloud_operations().unwrap().len();
    session.execute("cloud.save", &json!({"destination":destination})).unwrap();
    session.execute("file.new", &json!({"name":"Another design"})).unwrap();
    session.doc_mut().name = "An unsaved edit to the new design".into();
    host.saved(0);
    session.poll_cloud().unwrap();
    assert!(session.is_dirty());
    assert!(session.cloud_binding().is_none());
    assert_eq!(session.cloud_settings().connections.len(), 1);
    assert_eq!(session.cloud_operations().unwrap().len(), before + 1);
    session.execute("cloud.save", &json!({"destination":destination})).unwrap();
    assert_eq!(host.0.lock().unwrap().len(), 2);
}

#[test]
fn cancelled_open_and_future_formats_never_replace_the_current_design() {
    let (mut session, host, destination) = configured();
    session.doc_mut().name = "Keep this edit".into();
    let opening = session.execute("cloud.open", &json!({"destination":destination})).unwrap();
    session.execute("cloud.cancel", &json!({"operation":opening["operation"]})).unwrap();
    host.job(0).opened(solidcraft_io::cloud::prepare_document(&Document::new("Do not adopt")).unwrap()).unwrap();
    host.job(0).fail(solidcraft_cloud::Error::Storage("late network failure".into())).unwrap();
    assert_eq!(host.job(0).status().unwrap().phase, Phase::Cancelled);
    assert!(session.execute("cloud.open.finish", &json!({"operation":opening["operation"]})).is_err());
    let opening = session.execute("cloud.open", &json!({"destination":destination})).unwrap();
    let mut future = Snapshot::prepare(b"{}".to_vec(), Default::default()).unwrap();
    future.manifest.format = "solidcraft-cloud/2".into();
    host.job(1).update(|status| status.destination = Some(destination)).unwrap();
    host.job(1).opened(future).unwrap();
    let error = session.execute("cloud.open.finish", &json!({"operation":opening["operation"]})).unwrap_err();
    assert!(error.to_string().contains("Upgrade SolidCraft to open or save this design"));
    assert_eq!(session.doc.name, "Keep this edit");
    assert!(session.is_dirty());
}

#[test]
fn completed_open_adopts_a_verified_design_and_preserves_the_host() {
    let (mut session, host, destination) = configured();
    let opening = session.execute("cloud.open", &json!({"destination":destination})).unwrap();
    let mut snapshot = solidcraft_io::cloud::prepare_document(&Document::new("Opened design")).unwrap();
    snapshot.manifest.extra.insert("revision".into(), json!("published"));
    host.job(0)
        .update(|status| {
            status.destination = Some(destination.clone());
            status.revision = Some("published".into());
        })
        .unwrap();
    host.job(0).opened(snapshot).unwrap();
    session.execute("cloud.open.finish", &json!({"operation":opening["operation"]})).unwrap();
    assert_eq!(session.doc.name, "Opened design");
    assert!(!session.is_dirty());
    assert_eq!(session.cloud_binding().unwrap().destination, destination);
    let result = session.execute("file.save", &json!({})).unwrap();
    assert_eq!(result["unchanged"], true);
    assert_eq!(host.0.lock().unwrap().len(), 1, "opening and saving without edits must not create a revision");
    session.execute("cloud.sync", &json!({})).unwrap();
    assert!(matches!(host.0.lock().unwrap()[1].1, Operation::Synchronize(_)));
}

#[test]
fn a_clean_cloud_design_can_still_be_copied_to_another_destination() {
    let (mut session, host, destination) = configured();
    session.execute("cloud.save", &json!({"destination":destination})).unwrap();
    host.saved(0);
    session.poll_cloud().unwrap();
    assert!(!session.is_dirty());
    let copy = Destination { file: "parts/copy".into(), ..destination };
    session.execute("cloud.save", &json!({"destination":copy})).unwrap();
    let records = host.0.lock().unwrap();
    let (_, Operation::Save { save, .. }) = &records[1] else { panic!("expected cloud copy") };
    assert_eq!(save.record.destination, copy);
    assert!(save.record.expected_revision.is_none());
}

#[test]
fn cancelling_an_upload_keeps_its_capture_and_cannot_undo_confirmed_publication() {
    let (mut session, host, destination) = configured();
    let submitted = session.execute("cloud.save", &json!({"destination":destination})).unwrap();
    host.saved(0);
    host.job(0).update(|status| status.phase = Phase::Syncing).unwrap();
    session.execute("cloud.cancel", &json!({"operation":submitted["operation"]})).unwrap();
    assert_eq!(host.job(0).status().unwrap().phase, Phase::SavedLocally);
    session.poll_cloud().unwrap();
    assert!(session.cloud_binding().is_some());
    assert_eq!(host.0.lock().unwrap().len(), 1);
    host.job(0).update(|status| status.phase = Phase::Synced).unwrap();
    session.execute("cloud.cancel", &json!({"operation":submitted["operation"]})).unwrap();
    assert_eq!(host.job(0).status().unwrap().phase, Phase::Synced);
    host.job(0).fail(solidcraft_cloud::Error::Storage("late worker failure".into())).unwrap();
    assert_eq!(host.job(0).status().unwrap().phase, Phase::Synced);
}

#[test]
fn cloud_configuration_rejects_invalid_destinations_without_changing_the_editor() {
    let (mut session, _host, destination) = configured();
    let before = session.cloud_operations().unwrap().len();
    assert!(session.execute("cloud.configure", &json!({"settings":{"default_connection":"missing"}})).is_err());
    assert_eq!(session.cloud_settings().connections.len(), 1);
    let invalid = Destination { project: "../outside".into(), ..destination };
    assert!(session.execute("cloud.save", &json!({"destination":invalid})).is_err());
    assert_eq!(session.cloud_operations().unwrap().len(), before);
    session.set_cloud_host(None);
    assert!(session.execute("cloud.test", &json!({})).unwrap_err().to_string().contains("unavailable"));
}

#[test]
fn native_save_as_detaches_the_cloud_binding_and_ignores_its_late_capture() {
    let (mut session, host, destination) = configured();
    session.execute("cloud.save", &json!({"destination":destination})).unwrap();
    host.saved(0);
    session.poll_cloud().unwrap();
    session.doc_mut().name = "Native copy".into();
    session.execute("cloud.save", &json!({})).unwrap();
    let path = std::env::temp_dir().join(format!("solidcraft-cloud-native-copy-{}.solidcraft", std::process::id()));
    let mut done = session.scratch();
    done.execute("file.save_as", &json!({"path":path})).unwrap();
    session.adopt_save(&done);
    assert!(session.cloud_binding().is_none());
    host.saved(1);
    session.poll_cloud().unwrap();
    assert!(session.cloud_binding().is_none());
    assert!(!session.is_dirty());
    assert_eq!(solidcraft_io::read_design(&std::fs::read(&path).unwrap()).unwrap().name, "Native copy");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn key_generation_consumes_secret_input_without_serializing_it() {
    let (mut session, host, _) = configured();
    session.set_cloud_ssh_key_input(solidcraft_cloud::jobs::SshKeyInput { passphrase: "a local secret".to_owned().into(), remember: true }).unwrap();
    let result = session.execute("cloud.ssh.generate", &json!({})).unwrap();
    assert!(!result.to_string().contains("a local secret"));
    let jobs = host.0.lock().unwrap();
    let Operation::GenerateSshKey(input) = &jobs[0].1 else { panic!("key generation") };
    assert_eq!(&**input.passphrase, "a local secret");
    assert!(input.remember);
    assert!(session.cloud.ssh_key_input.is_none());
    drop(jobs);
    session.execute("cloud.ssh.generate", &json!({})).unwrap();
    let jobs = host.0.lock().unwrap();
    let Operation::GenerateSshKey(input) = &jobs[1].1 else { panic!("key generation") };
    assert!(input.passphrase.is_empty());
    drop(jobs);
    assert!(session.execute("cloud.ssh.generate", &json!({"passphrase":"not accepted"})).is_err());
    session.set_cloud_host(None);
    session.set_cloud_ssh_key_input(solidcraft_cloud::jobs::SshKeyInput { passphrase: "one shot".to_owned().into(), remember: false }).unwrap();
    assert!(session.execute("cloud.ssh.generate", &json!({})).is_err());
    assert!(session.cloud.ssh_key_input.is_none());
}
