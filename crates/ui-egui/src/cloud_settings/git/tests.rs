use super::*;
use crate::{Services, documents, scenario::Harness};
use solidcraft_cloud::jobs::{Host, Job, Operation};
use std::sync::Mutex;

#[derive(Default)]
struct Worker(Mutex<Vec<(Job, Operation, String)>>);
impl Host for Worker {
    fn submit(&self, operation: Operation, executable: String) -> solidcraft_cloud::Result<Job> {
        let job = Job::new("git".into())?;
        match &operation {
            Operation::LoadSettings => job.update(|s| {
                s.phase = Phase::Ready;
                s.result = Some(json!(Settings { retry_pending: false, ..Default::default() }));
            })?,
            Operation::List(_) => job.update(|s| {
                s.phase = Phase::Ready;
                s.result = Some(json!([]));
            })?,
            _ => self.0.lock().unwrap().push((job.clone(), operation, executable)),
        }
        Ok(job)
    }
}

fn setup(dark: bool) -> (Harness, Arc<Worker>) {
    let worker = Arc::new(Worker::default());
    let mut h = Harness::new();
    h.app = SolveApp::new(Default::default(), Services { cloud_host: Some(worker.clone()), ..Default::default() });
    h.app.preferences.theme = if dark { "dark" } else { "light" }.into();
    h.frames(3);
    h.app.start("cloud.configure");
    h.frames(3);
    (h, worker)
}

fn wizard(h: &mut Harness) -> &mut Wizard {
    h.app.cloud.editor.as_mut().unwrap().git.as_mut().unwrap()
}
fn fill(h: &mut Harness) {
    let w = wizard(h);
    w.connection.name = "Workshop".into();
    w.connection.checkout = "/example/check out".into();
    w.connection.remote = "../storage.git".into();
    w.executable = "/example/Git path/git".into();
    h.frame();
}
fn test_job(worker: &Worker) -> (Job, Connection, String) {
    let jobs = worker.0.lock().unwrap();
    let (job, Operation::Test(c), executable) = jobs.iter().rev().find(|(_, op, _)| matches!(op, Operation::Test(_))).unwrap() else {
        panic!("test")
    };
    (job.clone(), c.clone(), executable.clone())
}
fn verified(worker: &Worker) -> Connection {
    let (job, mut connection, _) = test_job(worker);
    connection.id = if connection.id == "git" { "git-verified".into() } else { connection.id };
    connection.git_remote = Some(solidcraft_cloud::GitRemote { fetch: "../storage.git".into(), push: vec!["../storage.git".into()] });
    job.update(|s| {
        s.phase = Phase::Ready;
        s.result = Some(json!(connection));
    })
    .unwrap();
    connection
}
fn click(h: &mut Harness, handle: &str) {
    h.step(&json!({"click":{"handle":handle}})).unwrap();
}

#[test]
fn verified_checkout_waits_for_use_and_durable_apply_in_both_themes() {
    for dark in [false, true] {
        let (mut h, worker) = setup(dark);
        fill(&mut h);
        click(&mut h, "dialog:ok");
        let (_, candidate, executable) = test_job(&worker);
        assert_eq!(candidate.checkout, "/example/check out");
        assert_eq!(executable, "/example/Git path/git");
        assert!(candidate.credential.is_empty());
        assert!(candidate.repository.is_empty(), "a display name must not become repository identity");
        assert!(h.app.session.cloud_settings().connections.is_empty());
        assert!(!apply(&mut h.app, false));
        documents::open_tab(&mut h.app, Default::default()).unwrap();
        let connection = verified(&worker);
        h.frames(3);
        assert!(wizard(&mut h).verified());
        assert!(h.app.session.cloud_settings().connections.is_empty());
        click(&mut h, "dialog:ok");
        assert!(h.app.cloud.editor.as_ref().unwrap().git.is_none());
        assert!(h.app.session.cloud_settings().connections.is_empty());
        click(&mut h, "prefs:apply");
        let (job, settings) = {
            let jobs = worker.0.lock().unwrap();
            let (job, Operation::Configure(s), _) = jobs.last().unwrap() else { panic!("configure") };
            (job.clone(), s.clone())
        };
        assert_eq!(settings.connections, [connection]);
        assert_eq!(settings.git_executable, "/example/Git path/git");
        job.update(|s| {
            s.phase = Phase::Synced;
            s.result = Some(json!(settings));
        })
        .unwrap();
        h.frames(3);
        assert_eq!(h.app.session.cloud_settings(), &settings);
    }
}

#[test]
fn failed_test_retries_and_edits_invalidate_verified_results() {
    let (mut h, worker) = setup(true);
    fill(&mut h);
    click(&mut h, "dialog:ok");
    test_job(&worker).0.fail(solidcraft_cloud::Error::Storage("offline".into())).unwrap();
    h.frames(3);
    assert!(wizard(&mut h).error.as_ref().unwrap().contains("offline"));
    click(&mut h, "dialog:ok");
    verified(&worker);
    h.frames(3);
    wizard(&mut h).connection.remote = "../other.git".into();
    h.frames(3);
    assert!(!wizard(&mut h).verified());
    click(&mut h, "dialog:ok");
    assert!(wizard(&mut h).pending.is_some(), "{:?}", wizard(&mut h).error);
    let (_, candidate, _) = test_job(&worker);
    assert_eq!(candidate.remote, "../other.git");
    assert_eq!(candidate.id, "git");
    assert!(candidate.git_remote.is_none());
    assert!(h.app.cloud.editor.as_ref().unwrap().draft.connections.is_empty());
    assert!(h.app.session.cloud_settings().connections.is_empty());
}

#[test]
fn ssh_choices_are_connection_scoped_and_cancel_keeps_applied_settings() {
    let (mut h, worker) = setup(false);
    fill(&mut h);
    click(&mut h, "cloud_git:cloud.git_ssh");
    click(&mut h, "cloud_git:ssh_key");
    wizard(&mut h).connection.ssh_identity = SshIdentity::File("/keys/key with 'quotes'".into());
    h.frame();
    click(&mut h, "dialog:ok");
    assert_eq!(test_job(&worker).1.ssh_identity, SshIdentity::File("/keys/key with 'quotes'".into()));
    verified(&worker);
    h.frames(3);
    click(&mut h, "cloud_git:ssh_system");
    click(&mut h, "dialog:ok");
    assert_eq!(test_job(&worker).1.ssh_identity, SshIdentity::System);
    verified(&worker);
    h.frames(3);
    click(&mut h, "dialog:ok");
    click(&mut h, "prefs:cancel");
    assert!(h.app.session.cloud_settings().connections.is_empty());
    assert!(h.app.session.cloud_settings().git_executable.is_empty());
}

#[test]
fn cancelling_a_test_across_tabs_prevents_late_staging() {
    let (mut h, worker) = setup(true);
    fill(&mut h);
    click(&mut h, "dialog:ok");
    let job = test_job(&worker).0;
    documents::open_tab(&mut h.app, Default::default()).unwrap();
    click(&mut h, "dialog:cancel");
    assert_eq!(job.status().unwrap().phase, Phase::Cancelled);
    verified(&worker);
    h.frames(3);
    assert!(h.app.cloud.editor.as_ref().unwrap().git.is_none());
    assert!(h.app.cloud.editor.as_ref().unwrap().draft.connections.is_empty());
}

#[test]
fn delayed_folder_picker_cannot_modify_a_reopened_editor() {
    use std::{cell::RefCell, future::poll_fn, rc::Rc, task::Poll};
    let (mut h, _) = setup(true);
    let answer = Rc::new(RefCell::new(None));
    let captured = answer.clone();
    h.app.services.pick_folder = Some(Box::new(move || {
        let captured = captured.clone();
        Box::pin(poll_fn(move |_| captured.borrow_mut().take().map_or(Poll::Pending, Poll::Ready)))
    }));
    click(&mut h, "cloud_git:browse:cloud.git_checkout");
    assert!(h.app.file_picker_busy());
    cancel(&mut h.app);
    open(&mut h.app);
    *answer.borrow_mut() = Some(Some("/late/answer".into()));
    h.frames(3);
    assert!(wizard(&mut h).connection.checkout.is_empty());
    assert!(!h.app.file_picker_busy());
}

#[test]
fn editing_a_connection_preserves_its_credentials_and_other_destinations() {
    let (mut h, worker) = setup(true);
    let first = solidcraft_cloud::setup::github_connection("example/personal").unwrap();
    let second = solidcraft_cloud::setup::github_connection("example/work").unwrap();
    let original = Settings {
        connections: vec![first.clone(), second.clone()],
        default_connection: Some(second.id.clone()),
        retry_pending: false,
        ..Default::default()
    };
    cancel(&mut h.app);
    h.app.cloud.editor.as_mut().unwrap().draft = original.clone();
    h.frame();
    h.app.cloud.editor.as_mut().unwrap().selected = Some(first.id.clone());
    h.frames(3);
    click(&mut h, "cloud_settings:advanced");
    assert!(wizard(&mut h).existing);
    assert_eq!(wizard(&mut h).connection, first);
    wizard(&mut h).connection.checkout = "/example/checkout".into();
    h.frames(3);
    click(&mut h, "dialog:ok");
    let (_, candidate, _) = test_job(&worker);
    assert_eq!(candidate.id, first.id);
    assert_eq!(candidate.credential, first.credential);
    verified(&worker);
    h.frames(3);
    click(&mut h, "dialog:ok");
    let edited = &h.app.cloud.editor.as_ref().unwrap().draft;
    assert_eq!(edited.connections[1], second);
    assert_eq!(edited.default_connection, original.default_connection);
    assert_eq!(edited.connections[0].credential, first.credential);
    assert!(edited.connections[0].git_remote.is_some());
}
