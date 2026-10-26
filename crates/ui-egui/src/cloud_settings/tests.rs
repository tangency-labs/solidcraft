use super::*;
use crate::{Services, documents, scenario::Harness};
use solidcraft_cloud::jobs::{Host, Job, Operation};
use std::sync::{Arc, Mutex};

struct Worker {
    settings: Settings,
    jobs: Mutex<Vec<(Job, Operation)>>,
}

impl Host for Worker {
    fn submit(&self, operation: Operation, _: String) -> solidcraft_cloud::Result<Job> {
        let job = Job::new(match &operation {
            Operation::Test(c) | Operation::Setup(c) | Operation::Synchronize(c) => c.id.clone(),
            _ => String::new(),
        })?;
        match &operation {
            Operation::LoadSettings => job.update(|s| {
                s.phase = Phase::Ready;
                s.result = Some(json!(self.settings));
            })?,
            Operation::List(_) => job.update(|s| {
                s.phase = Phase::Ready;
                s.result = Some(json!([]));
            })?,
            _ => self.jobs.lock().unwrap().push((job.clone(), operation)),
        }
        Ok(job)
    }
}

fn setup(dark: bool) -> (Harness, Arc<Worker>) {
    let first = solidcraft_cloud::setup::github_connection("example/personal").unwrap();
    let second = solidcraft_cloud::setup::github_connection("example/work").unwrap();
    let worker = Arc::new(Worker {
        settings: Settings {
            default_connection: Some(first.id.clone()),
            connections: vec![first, second],
            retry_pending: false,
            sync_after_save: false,
            git_executable: "/example/git".into(),
        },
        jobs: Default::default(),
    });
    let mut h = Harness::new();
    h.app = SolveApp::new(Default::default(), Services { cloud_host: Some(worker.clone()), ..Default::default() });
    h.app.preferences.theme = if dark { "dark" } else { "light" }.into();
    h.frame();
    h.app.start("cloud.settings");
    h.frames(3);
    (h, worker)
}

fn complete_configuration(worker: &Worker, fail: bool) {
    let jobs = worker.jobs.lock().unwrap();
    let (job, Operation::Configure(settings)) = jobs.iter().rev().find(|(_, op)| matches!(op, Operation::Configure(_))).unwrap() else {
        panic!("configuration")
    };
    if fail {
        job.fail(solidcraft_cloud::Error::Storage("disk full".into())).unwrap();
    } else {
        job.update(|s| {
            s.phase = Phase::Synced;
            s.result = Some(json!(settings));
        })
        .unwrap();
    }
}

#[test]
fn add_repository_menu_separates_provider_setup_from_git_settings() {
    for dark in [false, true] {
        let (mut h, _) = setup(dark);
        assert!(crate::scenario::rect_of("cloud_settings:advanced").is_none());
        h.step(&json!({"click":{"handle":"cloud_settings:connect"}})).unwrap();
        h.frames(2);
        h.step(&json!({"click":{"handle":"dialog:list:cloud_provider_github"}})).unwrap();
        h.frames(2);
        assert!(h.app.cloud.setup.is_some());
        assert!(h.app.cloud.editor.as_ref().unwrap().git.is_none());
        h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
        h.frames(2);
        h.step(&json!({"click":{"handle":"cloud_settings:connect"}})).unwrap();
        h.frames(2);
        h.step(&json!({"click":{"handle":"dialog:list:cloud_provider_git"}})).unwrap();
        h.frames(2);
        assert!(h.app.cloud.setup.is_none());
        assert!(h.app.cloud.editor.as_ref().unwrap().git.is_some());
    }
}

#[test]
fn apply_waits_for_durability_across_tabs_and_cancel_keeps_the_last_apply() {
    for dark in [false, true] {
        let (mut h, worker) = setup(dark);
        assert!(crate::scenario::rect_of("dialog:table:cloud_repositories:viewport").is_some());
        let mut changed = worker.settings.clone();
        changed.default_connection = Some(changed.connections[1].id.clone());
        changed.retry_pending = true;
        h.app.cloud.editor.as_mut().unwrap().draft = changed.clone();
        h.step(&json!({"click":{"handle":"prefs:apply"}})).unwrap();
        assert_eq!(h.app.session.cloud_settings(), &worker.settings);
        assert!(busy(&h.app));
        let grid_at_apply = h.app.ui.show_grid;
        h.app.ui.show_grid = !grid_at_apply;
        h.step(&json!({"click":{"handle":"prefs:cancel"}})).unwrap();
        assert!(h.app.prefs_window.open, "an accepted Apply must finish its durability boundary");
        documents::open_tab(&mut h.app, Default::default()).unwrap();
        complete_configuration(&worker, false);
        h.frames(3);
        assert_eq!(h.app.session.cloud_settings(), &changed);
        assert!(!busy(&h.app));
        h.app.cloud.editor.as_mut().unwrap().draft.connections.clear();
        h.step(&json!({"click":{"handle":"prefs:cancel"}})).unwrap();
        assert!(!h.app.prefs_window.open);
        assert_eq!(h.app.session.cloud_settings(), &changed);
        assert!(h.app.cloud.editor.is_none());
        assert_eq!(h.app.ui.show_grid, grid_at_apply, "Cancel restores the settings captured when Apply was requested");
        documents::switch(&mut h.app, 0);
        assert_eq!(h.app.session.cloud_settings(), &changed);
    }
}

#[test]
fn failed_ok_retains_edits_and_settings_until_retry_succeeds() {
    let (mut h, worker) = setup(true);
    h.app.cloud.editor.as_mut().unwrap().draft.sync_after_save = true;
    h.step(&json!({"click":{"handle":"prefs:ok"}})).unwrap();
    assert!(h.app.prefs_window.open);
    complete_configuration(&worker, true);
    h.frames(3);
    assert!(h.app.prefs_window.open);
    assert!(!h.app.session.cloud_settings().sync_after_save);
    assert!(h.app.cloud.editor.as_ref().unwrap().draft.sync_after_save);
    assert!(h.app.cloud.editor.as_ref().unwrap().error.as_deref().unwrap().contains("disk full"));
    h.step(&json!({"click":{"handle":"prefs:ok"}})).unwrap();
    complete_configuration(&worker, false);
    h.frames(3);
    assert!(!h.app.prefs_window.open);
    assert!(h.app.session.cloud_settings().sync_after_save);
}

#[test]
fn defaults_preserve_connections_and_disconnect_applies_only_after_confirmation() {
    let (mut h, worker) = setup(false);
    defaults(&mut h.app);
    h.frame();
    let editor = h.app.cloud.editor.as_ref().unwrap();
    assert_eq!(editor.draft.connections, worker.settings.connections);
    assert_eq!(editor.draft.default_connection, worker.settings.default_connection);
    assert!(editor.draft.retry_pending && editor.draft.sync_after_save);
    assert!(editor.draft.git_executable.is_empty());
    h.step(&json!({"click":{"handle":"cloud_settings:disconnect"}})).unwrap();
    assert_eq!(h.app.session.cloud_settings().connections.len(), 2);
    assert_eq!(h.app.cloud.editor.as_ref().unwrap().draft.connections.len(), 1);
    h.step(&json!({"click":{"handle":"prefs:cancel"}})).unwrap();
    assert_eq!(h.app.session.cloud_settings(), &worker.settings);
    h.app.start("cloud.settings");
    h.frames(2);
    h.step(&json!({"click":{"handle":"cloud_settings:disconnect"}})).unwrap();
    h.step(&json!({"click":{"handle":"prefs:ok"}})).unwrap();
    complete_configuration(&worker, false);
    h.frames(3);
    assert_eq!(h.app.session.cloud_settings().connections, worker.settings.connections[1..]);
    assert!(h.app.session.cloud_settings().default_connection.is_none());
}

#[test]
fn verified_setup_stays_pending_until_preferences_apply_and_cancel_discards_it() {
    let (mut h, worker) = setup(false);
    h.step(&json!({"click":{"handle":"cloud_settings:reconnect"}})).unwrap();
    h.frames(2);
    assert!(crate::scenario::rect_of("cloud:repository").is_some());
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    let jobs = worker.jobs.lock().unwrap();
    let (job, Operation::Setup(c)) = jobs.iter().rev().find(|(_, op)| matches!(op, Operation::Setup(_))).unwrap() else { panic!("setup") };
    let mut replacement = c.clone();
    replacement.credential = "replacement-credential".into();
    job.update(|s| {
        s.phase = Phase::Ready;
        s.result = Some(json!(replacement));
    })
    .unwrap();
    drop(jobs);
    h.frames(3);
    assert!(h.app.cloud.setup.is_none());
    assert_eq!(h.app.session.cloud_settings(), &worker.settings);
    assert_eq!(h.app.cloud.editor.as_ref().unwrap().draft.connections[0].credential, "replacement-credential");
    h.step(&json!({"click":{"handle":"prefs:cancel"}})).unwrap();
    assert_eq!(h.app.session.cloud_settings(), &worker.settings);
    assert!(!worker.jobs.lock().unwrap().iter().any(|(_, op)| matches!(op, Operation::Configure(_))));
}

#[test]
fn external_configuration_is_retained_and_requires_reload_before_applying_local_edits() {
    let (mut h, worker) = setup(true);
    h.app.cloud.editor.as_mut().unwrap().draft.connections[0].name = "Local edit".into();
    let mut external = worker.settings.clone();
    external.connections[1].name = "Changed elsewhere".into();
    h.app.run("cloud.configure", json!({"settings":external})).unwrap();
    complete_configuration(&worker, false);
    h.frames(2);
    h.step(&json!({"click":{"handle":"prefs:apply"}})).unwrap();
    assert!(!busy(&h.app));
    assert!(h.app.cloud.editor.as_ref().unwrap().error.is_some());
    assert_eq!(h.app.session.cloud_settings(), &external);
    h.step(&json!({"click":{"handle":"cloud_settings:reload"}})).unwrap();
    assert_eq!(h.app.cloud.editor.as_ref().unwrap().draft, external);
}

#[test]
fn test_sync_and_cancel_use_the_selected_applied_connection() {
    let (mut h, worker) = setup(true);
    h.step(&json!({"click":{"handle":"cloud_settings:test"}})).unwrap();
    let (test, id) = {
        let jobs = worker.jobs.lock().unwrap();
        let (job, Operation::Test(c)) = jobs.last().unwrap() else { panic!("connection test") };
        assert_eq!(c.id, worker.settings.connections[0].id);
        (job.clone(), job.status().unwrap().operation)
    };
    test.update(|s| s.storage_persistent = Some(false)).unwrap();
    h.frames(2);
    h.step(&json!({"click":{"handle":format!("cloud_settings:cancel:{id}")}})).unwrap();
    assert_eq!(test.status().unwrap().phase, Phase::Cancelled);
    h.step(&json!({"click":{"handle":"cloud_settings:sync"}})).unwrap();
    let jobs = worker.jobs.lock().unwrap();
    let (_, Operation::Synchronize(c)) = jobs.last().unwrap() else { panic!("synchronization") };
    assert_eq!(c.id, worker.settings.connections[0].id);
}

#[test]
fn repository_rows_select_remove_and_keep_test_progress_in_place() {
    for dark in [false, true] {
        let (mut h, worker) = setup(dark);
        assert!(crate::scenario::rect_of("cloud_settings:name").is_none());
        let second = worker.settings.connections[1].id.clone();
        h.step(&json!({"click":{"handle":format!("cloud_settings:row:{second}")}})).unwrap();
        assert_eq!(h.app.cloud.editor.as_ref().unwrap().selected.as_deref(), Some(second.as_str()));
        let controls =
            ["cloud_settings:test", "cloud_settings:sync", "cloud_settings:reconnect", "cloud_settings:status", "cloud_settings:sync_after_save"];
        let before: Vec<_> = controls.iter().map(|id| crate::scenario::rect_of(id).unwrap()).collect();
        h.step(&json!({"click":{"handle":"cloud_settings:test"}})).unwrap();
        let jobs = worker.jobs.lock().unwrap();
        let (job, Operation::Test(connection)) = jobs.last().unwrap() else { panic!("test") };
        assert_eq!(connection.id, second);
        let job = job.clone();
        drop(jobs);
        h.frames(3);
        for (id, rect) in controls.iter().zip(&before) {
            assert_eq!(crate::scenario::rect_of(id).unwrap(), *rect, "{id} moved while testing");
        }
        job.update(|s| s.phase = Phase::Ready).unwrap();
        h.frames(3);
        for (id, rect) in controls.iter().zip(&before) {
            assert_eq!(crate::scenario::rect_of(id).unwrap(), *rect, "{id} moved after testing");
        }
        h.step(&json!({"click":{"handle":format!("cloud_settings:remove:{second}")}})).unwrap();
        assert_eq!(h.app.cloud.editor.as_ref().unwrap().draft.connections.len(), 1);
        assert_eq!(h.app.session.cloud_settings().connections.len(), 2, "removal waits for Apply");
        assert_eq!(h.app.cloud.editor.as_ref().unwrap().draft.default_connection, worker.settings.default_connection);
    }
}
