use std::sync::{Arc, Mutex};

use serde_json::json;
use solidcraft_cloud::{
    Connection, Destination, Published, Settings,
    jobs::{Host, Job, Operation, Phase},
};

use crate::{Services, SolveApp, documents, scenario::Harness};

struct Worker {
    settings: Settings,
    jobs: Mutex<Vec<(Job, Operation)>>,
}

impl Host for Worker {
    fn submit(&self, operation: Operation, _: String) -> solidcraft_cloud::Result<Job> {
        let connection = match &operation {
            Operation::LoadSettings | Operation::Configure(_) => String::new(),
            Operation::Setup(c)
            | Operation::Test(c)
            | Operation::List(c)
            | Operation::Synchronize(c)
            | Operation::Open { connection: c, .. }
            | Operation::Save { connection: c, .. } => c.id.clone(),
        };
        let job = Job::new(connection)?;
        match &operation {
            Operation::LoadSettings => job.update(|s| {
                s.result = Some(json!(self.settings));
                s.phase = Phase::Ready;
            })?,
            Operation::Configure(settings) => job.update(|s| {
                s.result = Some(json!(settings));
                s.phase = Phase::Synced;
            })?,
            _ => self.jobs.lock().unwrap().push((job.clone(), operation)),
        }
        Ok(job)
    }
}

impl Worker {
    fn saved(&self, index: usize) {
        let jobs = self.jobs.lock().unwrap();
        let (job, Operation::Save { save, .. }) = &jobs[index] else { panic!("save required") };
        job.update(|status| {
            status.saved_locally = true;
            status.phase = Phase::SavedLocally;
            status.destination = Some(save.record.destination.clone());
            status.revision = Some(save.record.id.clone());
        })
        .unwrap();
    }
}

fn setup(retry: bool) -> (Harness, Arc<Worker>, Destination) {
    let worker = Arc::new(Worker {
        settings: Settings {
            connections: vec![Connection::default()],
            default_connection: Some("github".into()),
            sync_after_save: false,
            retry_pending: retry,
            ..Default::default()
        },
        jobs: Default::default(),
    });
    let mut h = Harness::new();
    h.app = SolveApp::new(Default::default(), Services { cloud_host: Some(worker.clone()), ..Default::default() });
    h.frame();
    let destination = Destination { connection: "github".into(), project: "Robot".into(), file: "parts/bracket".into() };
    (h, worker, destination)
}

#[test]
fn normal_cloud_save_and_close_wait_for_durability_and_keep_newer_edits_dirty() {
    let (mut h, worker, destination) = setup(false);
    h.app.session.doc_mut().name = "First capture".into();
    h.app.run("cloud.save", json!({"destination":destination})).unwrap();
    assert!(h.app.session.is_dirty());
    worker.saved(0);
    h.frame();
    assert!(!h.app.session.is_dirty());
    assert!(h.app.session.path.is_none());
    h.app.session.doc_mut().name = "Second capture".into();
    crate::toolbar::save(&mut h.app);
    assert!(!h.app.file_picker_busy(), "the cloud worker uses the live session, without Save As or a scratch-session writer");
    assert!(h.app.session.is_dirty());
    h.app.session.doc_mut().name = "Edited during local persistence".into();
    worker.saved(1);
    h.frame();
    assert!(h.app.session.is_dirty(), "only the captured revision became saved");
    documents::request_quit(&mut h.app);
    documents::answer_prompt(&mut h.app, documents::Answer::Save);
    assert!(!h.app.quit_requested && h.app.session.cloud_save_pending());
    h.frame();
    assert!(!h.app.quit_requested, "an operation id is not a disk acknowledgement");
    worker.saved(2);
    h.frame();
    assert!(h.app.quit_requested);
}

#[test]
fn tabs_share_connection_updates_and_confirmed_conflicts_without_redirecting_other_designs() {
    let (mut h, worker, destination) = setup(false);
    h.app.session.doc_mut().name = "Saved in the first tab".into();
    h.app.run("cloud.save", json!({"destination":destination})).unwrap();
    worker.saved(0);
    h.frame();
    let revision = h.app.session.cloud_binding().unwrap().revision.clone().unwrap();
    documents::open_tab(&mut h.app, Default::default()).unwrap();
    assert_eq!(h.app.session.cloud_settings(), &worker.settings);
    assert!(h.app.session.cloud_binding().is_none());
    h.app.run("cloud.sync", json!({})).unwrap();
    let conflict = Destination { file: "conflicts/bracket".into(), ..destination };
    worker.jobs.lock().unwrap()[1]
        .0
        .update(|status| {
            status.publications.push(Published { destination: conflict.clone(), revision, commit: "a".repeat(40), conflict: true });
            status.phase = Phase::Synced;
        })
        .unwrap();
    h.frame();
    assert!(h.app.session.cloud_binding().is_none());
    documents::switch(&mut h.app, 0);
    assert_eq!(h.app.session.cloud_binding().unwrap().destination, conflict);
    assert_eq!(h.app.session.cloud_operations().unwrap().iter().find(|s| s.saved_locally).unwrap().phase, Phase::Synced);
    let mut changed = worker.settings.clone();
    changed.connections.push(Connection { id: "local".into(), credential: "".into(), provider: "".into(), ..Default::default() });
    changed.default_connection = Some("local".into());
    h.app.run("cloud.configure", json!({"settings":changed})).unwrap();
    documents::switch(&mut h.app, 1);
    h.frame();
    assert_eq!(h.app.session.cloud_settings(), &changed);
    documents::switch(&mut h.app, 0);
    assert_eq!(h.app.session.cloud_settings(), &changed);
    assert_eq!(h.app.session.cloud_binding().unwrap().destination, conflict, "changing the default never retargets an open design");
}

#[test]
fn startup_resumes_connections_once_without_duplicate_active_synchronizations() {
    let (mut h, worker, _) = setup(true);
    assert_eq!(worker.jobs.lock().unwrap().len(), 1);
    assert!(matches!(worker.jobs.lock().unwrap()[0].1, Operation::Synchronize(_)));
    h.frames(10);
    assert_eq!(worker.jobs.lock().unwrap().len(), 1);
    worker.jobs.lock().unwrap()[0].0.update(|status| status.phase = Phase::Synced).unwrap();
    let mut output = h.ctx.run_ui(egui::RawInput { time: Some(62.0), ..Default::default() }, |ui| h.app.logic(ui.ctx()));
    // This logic-only frame has no renderer to consume font texture updates.
    output.textures_delta.clear();
    assert_eq!(worker.jobs.lock().unwrap().len(), 2, "offline queues get another attempt later");
}

#[test]
fn an_initial_capture_can_finish_before_quit_without_opening_native_save_as() {
    let (mut h, worker, destination) = setup(false);
    assert!(!h.app.session.is_dirty());
    h.app.run("cloud.save", json!({"destination":destination})).unwrap();
    assert!(!documents::active_is_blank(&h.app));
    documents::request_quit(&mut h.app);
    documents::answer_prompt(&mut h.app, documents::Answer::Save);
    assert!(!h.app.file_picker_busy() && !h.app.quit_requested);
    assert_eq!(worker.jobs.lock().unwrap().len(), 1, "wait for the existing capture");
    worker.saved(0);
    h.frame();
    assert!(h.app.quit_requested);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn actual_native_storage_recovers_an_offline_design_through_application_commands() {
    use solidcraft_cloud::jobs::JobStatus;
    use std::time::{Duration, Instant};
    fn wait(h: &mut Harness, id: &str) -> JobStatus {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            h.frame();
            let status = h.app.session.cloud_operations().unwrap().into_iter().find(|s| s.operation == id).unwrap();
            if !matches!(status.phase, Phase::Working | Phase::Syncing) {
                // The worker can finish between this frame's poll and our observation.
                // Let the next real frame take up that observed acknowledgement.
                h.frame();
                return status;
            }
            assert!(Instant::now() < deadline, "timed out: {status:?}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn launch(h: &mut Harness, root: &std::path::Path) {
        let host = solidcraft_engine::io::cloud::NativeHost::new(root.into()).unwrap();
        h.app = SolveApp::new(Default::default(), Services { cloud_host: Some(Arc::new(host)), ..Default::default() });
        let id = h.app.session.cloud_operations().unwrap()[0].operation.clone();
        assert_eq!(wait(h, &id).phase, Phase::Ready);
    }
    let mut h = Harness::new();
    let root = h.dir.join("cloud");
    launch(&mut h, &root);
    let connection = Connection {
        id: "offline".into(),
        provider: "".into(),
        credential: "".into(),
        checkout: h.dir.join("missing checkout").to_string_lossy().into_owned(),
        ..Default::default()
    };
    let settings = Settings {
        connections: vec![connection],
        default_connection: Some("offline".into()),
        sync_after_save: false,
        retry_pending: false,
        ..Default::default()
    };
    let configured = h.app.run("cloud.configure", json!({"settings":settings})).unwrap();
    assert_eq!(wait(&mut h, configured["operation"].as_str().unwrap()).phase, Phase::Synced);
    h.app.session.doc_mut().name = "Recovered offline".into();
    let destination = Destination { connection: "offline".into(), project: "Robot".into(), file: "parts/bracket".into() };
    let saved = h.app.run("cloud.save", json!({"destination":destination})).unwrap();
    assert_eq!(wait(&mut h, saved["operation"].as_str().unwrap()).phase, Phase::SavedLocally);
    assert!(!h.app.session.is_dirty());
    launch(&mut h, &root);
    assert_eq!(h.app.session.cloud_settings(), &settings);
    let listed = h.app.run("cloud.list", json!({})).unwrap();
    assert_eq!(wait(&mut h, listed["operation"].as_str().unwrap()).result.unwrap(), json!([destination]));
    let opened = h.app.run("cloud.open", json!({"destination":destination})).unwrap();
    assert_eq!(wait(&mut h, opened["operation"].as_str().unwrap()).phase, Phase::Ready);
    h.app.run("cloud.open.finish", json!({"operation":opened["operation"]})).unwrap();
    assert_eq!(h.app.session.doc.name, "Recovered offline");
    assert!(!h.app.session.is_dirty());
    assert_eq!(h.app.session.cloud_binding().unwrap().destination, destination);
}
