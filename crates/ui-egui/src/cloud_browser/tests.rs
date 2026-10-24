use super::*;
use crate::{Services, documents, scenario::Harness};
use solidcraft_cloud::{
    Settings,
    jobs::{Host, Job, Operation},
};
use std::sync::Mutex;

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
                s.phase = Phase::Ready;
                s.result = Some(json!(self.settings));
            })?,
            Operation::Configure(settings) => job.update(|s| {
                s.phase = Phase::Synced;
                s.result = Some(json!(settings));
            })?,
            _ => self.jobs.lock().unwrap().push((job.clone(), operation)),
        }
        Ok(job)
    }
}

fn setup(dark: bool) -> (Harness, Arc<Worker>, Destination) {
    let worker = Arc::new(Worker {
        settings: Settings {
            connections: vec![
                Connection { name: "Personal designs".into(), ..Default::default() },
                Connection { id: "work".into(), name: "Work designs".into(), credential: "cloud:work".into(), ..Default::default() },
            ],
            default_connection: Some("github".into()),
            retry_pending: false,
            sync_after_save: false,
            ..Default::default()
        },
        jobs: Default::default(),
    });
    let mut h = Harness::new();
    h.app = SolveApp::new(Default::default(), Services { cloud_host: Some(worker.clone()), ..Default::default() });
    h.app.ui.dark = dark;
    h.frame();
    let destination = Destination { connection: "github".into(), project: "Robot".into(), file: "parts/bracket".into() };
    (h, worker, destination)
}

fn latest(worker: &Worker, kind: &str) -> Job {
    worker
        .jobs
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|(_, op)| matches!((kind, op), ("list", Operation::List(_)) | ("open", Operation::Open { .. }) | ("save", Operation::Save { .. })))
        .unwrap()
        .0
        .clone()
}

fn show_listing(h: &mut Harness, worker: &Worker, destination: &Destination) {
    h.app.start("cloud.list");
    h.frames(2);
    latest(worker, "list")
        .update(|s| {
            s.phase = Phase::Ready;
            s.result = Some(json!([destination]));
        })
        .unwrap();
    h.frames(2);
}

#[test]
fn home_browses_independent_repositories_and_keeps_entries_when_refresh_fails() {
    for dark in [false, true] {
        let (mut h, worker, destination) = setup(dark);
        show_listing(&mut h, &worker, &destination);
        assert!(crate::scenario::rect_of("cloud:design:0").is_some());
        assert!(crate::scenario::rect_of("cloud:repositories").is_some());
        h.step(&json!({"click":{"handle":"cloud:refresh"}})).unwrap();
        latest(&worker, "list").fail(solidcraft_cloud::Error::Storage("offline".into())).unwrap();
        h.frames(2);
        assert_eq!(*h.app.cloud.browser.catalogs["github"].entries, vec![destination]);
        assert_eq!(h.app.cloud.browser.catalogs["github"].message.as_deref(), Some("Cloud storage: offline"));
        h.app.cloud.browser.selected = Some("work".into());
        h.frames(2);
        latest(&worker, "list")
            .update(|s| {
                s.phase = Phase::Ready;
                s.result = Some(json!([]));
            })
            .unwrap();
        h.frames(2);
        assert!(h.app.cloud.browser.catalogs["work"].entries.is_empty());
        assert_eq!(h.app.cloud.browser.catalogs["github"].entries.len(), 1);
        assert_eq!(h.app.session.cloud_settings().connections.len(), 2);
    }
}

#[test]
fn opening_a_design_keeps_edits_made_during_download_and_after_switching_tabs() {
    let (mut h, worker, destination) = setup(true);
    h.app.session.doc_mut().name = "Unsaved first tab".into();
    show_listing(&mut h, &worker, &destination);
    h.step(&json!({"click":{"handle":"cloud:design:0"}})).unwrap();
    let job = latest(&worker, "open");
    h.app.session.doc_mut().name = "Edited during download".into();
    documents::open_tab(&mut h.app, Session::default()).unwrap();
    h.app.session.doc_mut().name = "Second tab also unsaved".into();
    let mut remote = Session::default();
    remote.doc_mut().name = "Cloud bracket".into();
    let snapshot = solidcraft_engine::io::cloud::prepare_document(&remote.design()).unwrap();
    job.update(|s| {
        s.destination = Some(destination.clone());
        s.revision = Some("a".repeat(64));
    })
    .unwrap();
    job.opened(snapshot).unwrap();
    h.frames(3);
    assert_eq!(h.app.session.doc.name, "Cloud bracket");
    assert_eq!(h.app.session.cloud_binding().unwrap().destination, destination);
    assert!(!h.app.session.is_dirty());
    assert_eq!(h.app.docs.count(), 3);
    documents::switch(&mut h.app, 0);
    assert_eq!(h.app.session.doc.name, "Edited during download");
    assert!(h.app.session.is_dirty());
    documents::switch(&mut h.app, 1);
    assert_eq!(h.app.session.doc.name, "Second tab also unsaved");
    assert!(h.app.session.is_dirty());
}

#[test]
fn initial_save_uses_chosen_destination_and_waits_for_local_durability() {
    for dark in [false, true] {
        let (mut h, worker, _) = setup(dark);
        h.app.cloud.browser.selected = Some("work".into());
        crate::toolbar::save(&mut h.app);
        h.frames(2);
        assert!(!h.app.file_picker_busy());
        assert!(crate::scenario::rect_of("cloud:name").is_some());
        let picker = h.app.cloud.browser.picker.as_mut().unwrap();
        assert_eq!(picker.connection, "work", "the explicitly browsed repository is proposed without changing the default");
        picker.connection = "work".into();
        picker.project = "Robot".into();
        picker.name = "bracket".into();
        h.frame();
        h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
        assert!(h.app.cloud.browser.picker.is_none());
        assert!(h.app.session.cloud_save_pending());
        assert_eq!(h.app.session.doc.name, "bracket", "an unnamed design takes the chosen name");
        let jobs = worker.jobs.lock().unwrap();
        let (job, Operation::Save { save, .. }) = jobs.last().unwrap() else { panic!("save required") };
        assert_eq!(save.record.destination, Destination { connection: "work".into(), project: "Robot".into(), file: "parts/bracket".into() });
        h.app.session.doc_mut().name = "Later edits".into();
        job.update(|s| {
            s.saved_locally = true;
            s.phase = Phase::SavedLocally;
            s.destination = Some(save.record.destination.clone());
            s.revision = Some(save.record.id.clone());
        })
        .unwrap();
        drop(jobs);
        h.frames(2);
        assert!(h.app.session.is_dirty());
        assert_eq!(h.app.session.cloud_binding().unwrap().destination.connection, "work");
        assert_eq!(h.app.session.cloud_settings().default_connection.as_deref(), Some("github"));
    }
}

#[test]
fn picker_cancels_on_tab_change_and_refuses_known_existing_names_and_hostile_paths() {
    let (mut h, worker, existing) = setup(false);
    show_listing(&mut h, &worker, &existing);
    h.app.start("cloud.save");
    let picker = h.app.cloud.browser.picker.as_mut().unwrap();
    picker.project = "Robot".into();
    picker.name = "bracket".into();
    let (_, exists, valid) = destination(&h.app, h.app.cloud.browser.picker.as_ref().unwrap());
    assert!(exists && !valid);
    h.app.cloud.browser.picker.as_mut().unwrap().project = "../outside".into();
    assert!(!destination(&h.app, h.app.cloud.browser.picker.as_ref().unwrap()).2);
    h.app.cloud.browser.picker.as_mut().unwrap().project = "Robot".into();
    h.frames(2);
    h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
    assert!(h.app.cloud.browser.picker.is_none());
    h.app.start("cloud.save");
    documents::open_tab(&mut h.app, Session::default()).unwrap();
    h.frames(2);
    assert!(h.app.cloud.browser.picker.is_none());
    assert!(!worker.jobs.lock().unwrap().iter().any(|(_, op)| matches!(op, Operation::Save { .. })));
}

#[test]
fn quit_waits_for_destination_choice_and_cancellation_keeps_the_design() {
    let (mut h, _, _) = setup(false);
    h.app.home.open = false;
    h.app.session.doc_mut().name = "Unsaved".into();
    documents::request_quit(&mut h.app);
    documents::answer_prompt(&mut h.app, documents::Answer::Save);
    h.frames(3);
    assert!(h.app.docs.closing.is_some());
    assert!(!h.app.quit_requested);
    h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
    h.frames(2);
    assert!(!h.app.quit_requested);
    assert!(h.app.session.is_dirty());
    assert!(h.app.docs.quitting.is_none());
}

#[test]
fn listing_cannot_cross_connections_or_accept_invalid_destinations() {
    let job = Job::new("github".into()).unwrap();
    job.update(|s| {
        s.result = Some(json!([{"connection":"work","project":"Robot","file":"parts/a"}]));
    })
    .unwrap();
    assert!(entries(&job.status().unwrap(), "github").is_err());
    job.update(|s| {
        s.result = Some(json!([{"connection":"github","project":"../outside","file":"parts/a"}]));
    })
    .unwrap();
    assert!(entries(&job.status().unwrap(), "github").is_err());
}
