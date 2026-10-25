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
            Operation::LoadSettings | Operation::Configure(_) | Operation::GenerateSshKey(_) => String::new(),
            Operation::Setup(c)
            | Operation::Test(c)
            | Operation::List(c)
            | Operation::Synchronize(c)
            | Operation::Thumbnail { connection: c, .. }
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
fn home_controls_have_consistent_heights_and_left_alignment() {
    for dark in [false, true] {
        let (mut h, worker, destination) = setup(dark);
        show_listing(&mut h, &worker, &destination);
        let connect = crate::scenario::rect_of("cloud:connect").unwrap();
        let repository = crate::scenario::rect_of("cloud:repositories").unwrap();
        assert!((connect.left() - repository.left()).abs() < 1.0, "repository selector aligns with actions");
        for id in ["cloud:save", "cloud:settings", "cloud:repositories", "cloud:refresh", "cloud:sync"] {
            let rect = crate::scenario::rect_of(id).unwrap();
            assert!((rect.height() - connect.height()).abs() < 1.0, "{id}: {rect:?} versus {connect:?}");
        }
        for width in [740.0, 480.0] {
            h.call("ui.resize", json!({"width":width,"height":1100}));
            h.frames(3);
            let page = crate::scenario::rect_of("home:page").unwrap();
            let controls: Vec<_> = ["cloud:connect", "cloud:save", "cloud:settings", "cloud:repositories", "cloud:refresh", "cloud:sync"]
                .into_iter()
                .map(|id| crate::scenario::rect_of(id).unwrap())
                .collect();
            let sync = crate::scenario::rect_of("cloud:sync").unwrap();
            let t = crate::theme::Tokens::get();
            let label_width = h
                .ctx
                .fonts_mut(|fonts| fonts.layout_no_wrap(tr("cmd.cloud.sync").into(), egui::FontId::proportional(t.font_label), t.form_ink).size().x);
            assert!(sync.width() >= label_width + t.space_8 * 2.0, "synchronization label must remain readable at {width}");
            for (index, rect) in controls.iter().enumerate() {
                assert!(rect.left() >= page.left() && rect.right() <= page.right(), "{width}: {rect:?} outside {page:?}");
                assert!(controls.iter().skip(index + 1).all(|other| !rect.intersects(*other)), "controls overlap at {width}");
            }
        }
    }
}

#[test]
fn file_actions_copy_a_local_design_to_cloud_without_changing_its_source() {
    for navigation in [false, true] {
        let (mut h, worker, _) = setup(true);
        h.app.run("solid.box", json!({"length":8,"width":6,"height":4})).unwrap();
        let path = h.dir.join("Local bracket.solidcraft");
        h.app.run("file.save_as", json!({"path":path})).unwrap();
        h.settle();
        let original = std::fs::read(&path).unwrap();
        if navigation {
            h.app.home.show(0);
            h.frames(2);
            h.step(&json!({"click":{"handle":"home:save_cloud"}})).unwrap();
        } else {
            h.app.home.open = false;
            h.frames(2);
            h.step(&json!({"click":{"handle":"appbar:folder"}})).unwrap();
            h.step(&json!({"widget":"Save to Cloud Project"})).unwrap();
        }
        h.frames(2);
        assert!(h.app.cloud.browser.picker.is_some());
        assert_eq!(h.app.session.path.as_deref(), path.to_str());
        let picker = h.app.cloud.browser.picker.as_mut().unwrap();
        picker.project = "Robot".into();
        picker.name = "local-bracket".into();
        h.frame();
        h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
        let (job, save) = {
            let jobs = worker.jobs.lock().unwrap();
            let (job, Operation::Save { save, .. }) = jobs.last().unwrap() else { panic!("save required") };
            (job.clone(), save.record.clone())
        };
        assert_eq!(save.destination.file, "parts/local-bracket");
        job.update(|s| {
            s.saved_locally = true;
            s.phase = Phase::SavedLocally;
            s.destination = Some(save.destination.clone());
            s.revision = Some(save.id.clone());
        })
        .unwrap();
        h.frames(2);
        assert_eq!(h.app.session.cloud_binding().unwrap().destination, save.destination);
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
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
fn completed_synchronization_refreshes_home_even_without_publications() {
    let (mut h, worker, destination) = setup(true);
    show_listing(&mut h, &worker, &destination);
    let before = latest(&worker, "list").status().unwrap().operation;
    h.app.run("cloud.sync", json!({"connection":"github"})).unwrap();
    let sync = worker.jobs.lock().unwrap().iter().rev().find(|(_, op)| matches!(op, Operation::Synchronize(_))).unwrap().0.clone();
    sync.update(|status| {
        status.phase = Phase::Synced;
        status.result = Some(json!([]));
    })
    .unwrap();
    h.frames(2);
    let refresh = latest(&worker, "list").status().unwrap().operation;
    assert_ne!(refresh, before);
    h.frames(3);
    assert_eq!(latest(&worker, "list").status().unwrap().operation, refresh, "one refresh per completion");
}

#[test]
fn competing_local_versions_require_a_choice_and_open_only_the_selected_capture() {
    for dark in [false, true] {
        let (mut h, worker, destination) = setup(dark);
        h.app.start("cloud.list");
        h.frames(2);
        let captures: Vec<_> = ["First offline edit", "Second offline edit"]
            .into_iter()
            .map(|name| {
                solidcraft_cloud::PendingSave::stage(
                    &Connection::default(),
                    &destination.project,
                    &destination.file,
                    None,
                    solidcraft_engine::io::cloud::prepare_document(&Document::new(name)).unwrap(),
                )
                .unwrap()
            })
            .collect();
        latest(&worker, "list")
            .update(|s| {
                s.phase = Phase::Ready;
                s.result = Some(json!([destination]));
                s.catalog = solidcraft_cloud::stored_designs(vec![], captures.iter().map(|s| s.record.clone()).collect()).unwrap();
            })
            .unwrap();
        h.frames(3);
        h.step(&json!({"click":{"handle":"cloud:design:0"}})).unwrap();
        h.frames(2);
        assert!(h.app.cloud.browser.versions.is_some());
        assert!(!worker.jobs.lock().unwrap().iter().any(|(_, op)| matches!(op, Operation::Open { .. })));
        h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
        assert!(h.app.cloud.browser.versions.is_none());
        h.step(&json!({"click":{"handle":"cloud:design:0"}})).unwrap();
        h.frames(2);
        h.step(&json!({"click":{"handle":"cloud:version:1"}})).unwrap();
        let selected = h.app.cloud.browser.catalogs["github"].stored[0].local_revisions[1].revision.clone();
        h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
        let job = latest(&worker, "open");
        {
            let jobs = worker.jobs.lock().unwrap();
            let (_, Operation::Open { destination: actual, local_revision, .. }) =
                jobs.iter().find(|(j, _)| j.status().unwrap().operation == job.status().unwrap().operation).unwrap()
            else {
                panic!("opening")
            };
            assert_eq!(actual, &destination);
            assert_eq!(local_revision.as_ref(), Some(&selected));
        }
        let capture = captures.iter().find(|s| s.record.id == selected).unwrap();
        job.update(|s| {
            s.destination = Some(destination.clone());
            s.revision = Some(selected.clone());
        })
        .unwrap();
        job.opened(capture.record.recover(|p| Ok(capture.objects[p].clone())).unwrap()).unwrap();
        h.frames(3);
        assert_eq!(h.app.session.cloud_binding().unwrap().revision.as_ref(), Some(&selected));
        assert!(!h.app.session.is_dirty());
    }
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

#[test]
fn home_restores_durable_statuses_and_refreshes_confirmed_publications_in_both_themes() {
    for dark in [false, true] {
        let (mut h, worker, destination) = setup(dark);
        h.app.start("cloud.list");
        h.frames(2);
        let staged = solidcraft_cloud::PendingSave::stage(
            &Connection::default(),
            &destination.project,
            &destination.file,
            None,
            solidcraft_engine::io::cloud::prepare_document(&Document::new("Offline bracket")).unwrap(),
        )
        .unwrap();
        let stored = solidcraft_cloud::stored_designs(vec![], vec![staged.record.clone()]).unwrap();
        latest(&worker, "list")
            .update(|s| {
                s.phase = Phase::Ready;
                s.result = Some(json!([destination]));
                s.catalog = stored;
            })
            .unwrap();
        h.frames(3);
        assert!(h.app.cloud.browser.statuses.is_empty(), "restart has no Save jobs");
        assert_eq!(phase(&h.app.cloud.browser.catalogs["github"].stored[0]), tr("cloud.saved_locally"));
        h.step(&json!({"click":{"handle":"cloud:refresh"}})).unwrap();
        latest(&worker, "list").fail(solidcraft_cloud::Error::Storage("offline".into())).unwrap();
        h.frames(3);
        assert_eq!(h.app.cloud.browser.catalogs["github"].stored[0].phase, Phase::SavedLocally);
        let sync = h.app.run("cloud.sync", json!({})).unwrap();
        let job = worker.jobs.lock().unwrap().iter().find(|(j, _)| j.status().unwrap().operation == sync["operation"]).unwrap().0.clone();
        job.update(|s| {
            s.phase = Phase::Synced;
            s.publications.push(solidcraft_cloud::Published {
                destination: destination.clone(),
                revision: staged.record.id.clone(),
                commit: "a".repeat(40),
                conflict: false,
            });
        })
        .unwrap();
        let before = latest(&worker, "list").status().unwrap().operation;
        h.frames(3);
        assert_ne!(latest(&worker, "list").status().unwrap().operation, before, "publication refreshes durable metadata");
        let entry = solidcraft_cloud::CachedEntry::new(staged.record.clone(), destination.clone(), Some(staged.record.id), true).unwrap();
        let stored = solidcraft_cloud::stored_designs(vec![entry], vec![]).unwrap();
        latest(&worker, "list")
            .update(|s| {
                s.phase = Phase::Ready;
                s.result = Some(json!([destination]));
                s.catalog = stored;
            })
            .unwrap();
        h.frames(3);
        assert_eq!(phase(&h.app.cloud.browser.catalogs["github"].stored[0]), tr("cloud.synced"));
    }
}

#[test]
fn preview_result_crosses_tabs_and_is_consumed_while_a_file_picker_is_pending() {
    let (mut h, worker, destination) = setup(true);
    show_listing(&mut h, &worker, &destination);
    let job = worker.jobs.lock().unwrap().iter().rev().find(|(_, op)| matches!(op, Operation::Thumbnail { .. })).unwrap().0.clone();
    documents::open_tab(&mut h.app, Session::default()).unwrap();
    h.app.home.show(0);
    h.app.services.pick_folder = Some(Box::new(|| Box::pin(std::future::pending())));
    h.app.pick_folder(|_, _| {});
    assert!(h.app.file_picker_busy());
    let png = include_bytes!("../../../../assets/samples/exports/0.png").to_vec();
    job.update(|s| {
        s.phase = Phase::Ready;
        s.result = Some(solidcraft_cloud::Thumbnail::response(Some(png)));
    })
    .unwrap();
    h.frames(3);
    assert!(job.status().unwrap().result.is_none(), "image bytes leave operation history");
    assert!(previews::image(&h.app, &destination).is_some());
}

#[test]
fn preview_requests_are_bounded_and_evicted_operations_are_cancelled() {
    let (mut h, worker, _) = setup(false);
    for n in 0..70 {
        previews::touch(&mut h.app, Destination { connection: "github".into(), project: "Robot".into(), file: format!("parts/preview-{n}") });
    }
    let jobs = worker.jobs.lock().unwrap();
    let previews: Vec<_> = jobs.iter().filter(|(_, op)| matches!(op, Operation::Thumbnail { .. })).collect();
    assert_eq!(previews.len(), 70);
    assert_eq!(previews.iter().filter(|(j, _)| j.status().unwrap().phase == Phase::Working).count(), 64);
    assert_eq!(previews.iter().filter(|(j, _)| j.status().unwrap().phase == Phase::Cancelled).count(), 6);
}
