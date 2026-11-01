use super::*;
use crate::{Services, documents, scenario::Harness};
use solidcraft_cloud::{
    Settings,
    jobs::{Host, Job, Operation},
};
use solidcraft_engine::Session;
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
            | Operation::Trash(c)
            | Operation::History { connection: c, .. }
            | Operation::Manage { connection: c, .. }
            | Operation::Discard { connection: c, .. }
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
        .find(|(_, op)| {
            matches!(
                (kind, op),
                ("list", Operation::List(_))
                    | ("open", Operation::Open { .. })
                    | ("save", Operation::Save { .. })
                    | ("history", Operation::History { .. })
                    | ("manage", Operation::Manage { .. })
                    | ("discard", Operation::Discard { .. })
                    | ("trash", Operation::Trash(_))
            )
        })
        .unwrap()
        .0
        .clone()
}

#[test]
fn management_waits_for_history_preserves_observed_revision_and_recovers_a_queued_action() {
    for dark in [false, true] {
        let (mut h, worker, destination) = setup(dark);
        show_listing(&mut h, &worker, &destination);
        h.app.start("cloud.rename");
        h.frames(2);
        h.step(&json!({"click":{"handle":"cloud:manage:delete"}})).unwrap();
        assert!(!worker.jobs.lock().unwrap().iter().any(|(_, op)| matches!(op, Operation::Manage { .. })));
        let history = json!({"revision":"a".repeat(64),"revisions":[{"commit":"b".repeat(40),"file":destination.file,"date":"2026-10-26T00:00:00Z","message":"Save bracket"}]});
        latest(&worker, "history")
            .update(|s| {
                s.phase = Phase::Ready;
                s.result = Some(history.clone());
            })
            .unwrap();
        h.frames(2);
        h.step(&json!({"click":{"handle":"cloud:manage:name"}})).unwrap();
        h.call("ui.key", json!({"key":"A","ctrl":true}));
        h.call("ui.text", json!({"text":"Renamed bracket"}));
        h.frames(2);
        h.step(&json!({"click":{"handle":"cloud:manage:rename"}})).unwrap();
        {
            let jobs = worker.jobs.lock().unwrap();
            let (_, Operation::Manage { destination: actual, expected_revision, change, .. }) =
                jobs.iter().rev().find(|(_, op)| matches!(op, Operation::Manage { .. })).unwrap()
            else {
                panic!("management")
            };
            assert_eq!(actual, &destination);
            assert_eq!(expected_revision.as_deref(), Some("a".repeat(64).as_str()));
            assert_eq!(change, &solidcraft_cloud::management::Change::Rename { file: "parts/Renamed bracket".into() });
        }
        let job = latest(&worker, "manage");
        let count = worker.jobs.lock().unwrap().len();
        h.step(&json!({"click":{"handle":"cloud:manage:rename"}})).unwrap();
        assert_eq!(worker.jobs.lock().unwrap().len(), count, "busy actions do not submit twice");
        job.update(|s| {
            s.phase = Phase::SavedLocally;
            s.saved_locally = true;
            s.revision = Some("c".repeat(64));
            s.message = Some("offline".into());
        })
        .unwrap();
        h.frames(2);
        h.step(&json!({"click":{"handle":"cloud:manage:discard"}})).unwrap();
        let discard = latest(&worker, "discard");
        discard.fail(solidcraft_cloud::Error::Storage("repository unavailable".into())).unwrap();
        h.frames(2);
        h.step(&json!({"click":{"handle":"cloud:manage:discard"}})).unwrap();
        latest(&worker, "discard").update(|s| s.phase = Phase::Synced).unwrap();
        h.frames(2);
        latest(&worker, "history")
            .update(|s| {
                s.phase = Phase::Ready;
                s.result = Some(history);
            })
            .unwrap();
        h.frames(2);
        h.step(&json!({"click":{"handle":"cloud:manage:delete"}})).unwrap();
        h.step(&json!({"click":{"handle":"cloud:manage:delete:confirm"}})).unwrap();
        latest(&worker, "manage").update(|s| s.phase = Phase::Synced).unwrap();
        h.frames(2);
        latest(&worker, "trash")
            .update(|s| {
                s.phase = Phase::Ready;
                s.result = Some(json!([destination]));
            })
            .unwrap();
        h.frames(2);
        h.step(&json!({"click":{"handle":"cloud:trash:0"}})).unwrap();
        latest(&worker,"history").update(|s|{s.phase=Phase::Ready;s.result=Some(json!({"revision":"d".repeat(64),"deleted":true,"revisions":[{"commit":"b".repeat(40),"file":destination.file,"date":"2026-10-26T00:00:00Z","message":"Saved before deletion"}]}));}).unwrap();
        h.frames(2);
        h.step(&json!({"click":{"handle":"cloud:manage:revision:0"}})).unwrap();
        h.step(&json!({"click":{"handle":"cloud:manage:restore"}})).unwrap();
        let jobs = worker.jobs.lock().unwrap();
        let (_, Operation::Manage { change, .. }) = jobs.iter().rev().find(|(_, op)| matches!(op, Operation::Manage { .. })).unwrap() else {
            panic!("restore")
        };
        assert!(matches!(change,solidcraft_cloud::management::Change::Restore {commit,file} if commit==&"b".repeat(40) && file==&destination.file));
        drop(jobs);
        h.app.session.doc_mut().name = "Keep my open design".into();
        latest(&worker, "manage").update(|s| s.phase = Phase::Synced).unwrap();
        h.frames(2);
        assert!(h.app.file_load.busy(), "restoration starts an Open after this frame's status poll");
        assert_eq!(h.app.session.doc.name, "Keep my open design");
        let opening = latest(&worker, "open");
        opening
            .update(|s| {
                s.destination = Some(destination.clone());
                s.revision = Some("e".repeat(64));
            })
            .unwrap();
        opening.opened(solidcraft_engine::io::cloud::prepare_document(&Document::new("Restored bracket")).unwrap()).unwrap();
        h.settle();
        assert_eq!(h.app.session.doc.name, "Restored bracket");
        assert_eq!(h.app.docs.count(), 2, "restoring keeps the existing edited design in its tab");
        assert_eq!(h.app.session.cloud_binding().unwrap().destination, destination);
    }
}

// Fake cloud jobs are completed explicitly by each test. Do not let Harness::settle
// wait for a network job before the test has had a chance to provide its response.
fn click_cloud(h: &mut Harness, handle: &str) {
    let at = crate::scenario::handle_at(handle).unwrap();
    let (request, _) = crate::control::ControlRequest::new("ui.click", json!({"x":at.x,"y":at.y}));
    crate::control::handle(&mut h.app, &h.ctx, &request);
    h.frames(8);
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
fn startup_shows_local_projects_while_synchronization_and_remote_listing_are_pending() {
    for dark in [false, true] {
        let (mut h, worker, destination) = setup(dark);
        let mut settings = h.app.session.cloud_settings().clone();
        settings.retry_pending = true;
        h.app.run("cloud.configure", json!({"settings":settings})).unwrap();
        h.app.start("cloud.list");
        h.frames(2);
        let listing = latest(&worker, "list");
        listing.update(|s| s.result = Some(json!([destination]))).unwrap();
        h.frames(3);
        assert_eq!(listing.status().unwrap().phase, Phase::Working);
        assert!(crate::scenario::rect_of("cloud:refresh").is_none(), "Synchronize is the single repository action");
        assert_eq!(crate::scenario::count_of("cloud:sync:enabled"), Some(0.0));
        assert!(
            worker
                .jobs
                .lock()
                .unwrap()
                .iter()
                .any(|(job, op)| matches!(op, Operation::Synchronize(_)) && job.status().unwrap().phase == Phase::Working)
        );
        assert_eq!(h.app.cloud.browser.catalogs["github"].entries.as_ref(), std::slice::from_ref(&destination));
        assert!(crate::scenario::rect_of("cloud:design:0").is_some(), "cached projects must be visible before either network operation finishes");
        listing.fail(solidcraft_cloud::Error::Storage("cellular connection lost".into())).unwrap();
        h.frames(3);
        assert_eq!(
            h.app.cloud.browser.catalogs["github"].entries.as_ref(),
            std::slice::from_ref(&destination),
            "a failed refresh retains local projects"
        );
    }
}

#[test]
fn cloud_errors_stay_in_the_header_tooltip_and_clear_after_recovery() {
    for dark in [false, true] {
        let (mut h, worker, destination) = setup(dark);
        show_listing(&mut h, &worker, &destination);
        h.app.status = None;
        let submitted = h.app.run("cloud.sync", json!({"connection":"github"})).unwrap();
        let job = worker.jobs.lock().unwrap().iter().find(|(job, _)| job.status().unwrap().operation == submitted["operation"]).unwrap().0.clone();
        job.fail(solidcraft_cloud::Error::Storage("offline example; local saves retained".into())).unwrap();
        h.frames(3);
        assert!(!h.app.status.as_ref().is_some_and(|s| s.0.contains("offline example")), "cloud errors must not become page banners");
        assert!(crate::scenario::rect_of("dialog:section_status").is_some());
        assert!(status::notice(&h.app, selected(&h.app).as_ref()).unwrap().contains("offline example"));
        assert_eq!(h.app.cloud.browser.catalogs["github"].entries.as_ref(), std::slice::from_ref(&destination));
        let preview = h.app.run("cloud.thumbnail", json!({"destination":destination})).unwrap();
        worker
            .jobs
            .lock()
            .unwrap()
            .iter()
            .find(|(job, _)| job.status().unwrap().operation == preview["operation"])
            .unwrap()
            .0
            .update(|s| {
                s.phase = Phase::Ready;
                s.result = Some(solidcraft_cloud::Thumbnail::response(None));
            })
            .unwrap();
        h.frames(3);
        assert!(status::notice(&h.app, selected(&h.app).as_ref()).unwrap().contains("offline example"), "a preview cannot clear a failed push");
        let retried = h.app.run("cloud.sync", json!({"connection":"github"})).unwrap();
        worker
            .jobs
            .lock()
            .unwrap()
            .iter()
            .find(|(job, _)| job.status().unwrap().operation == retried["operation"])
            .unwrap()
            .0
            .update(|s| s.phase = Phase::Synced)
            .unwrap();
        h.frames(3);
        latest(&worker, "list")
            .update(|s| {
                s.phase = Phase::Ready;
                s.result = Some(json!([destination]));
            })
            .unwrap();
        h.frames(3);
        assert!(status::notice(&h.app, selected(&h.app).as_ref()).is_none());
        assert!(crate::scenario::rect_of("dialog:section_status").is_none());
    }
}

#[test]
fn home_repository_menu_opens_only_the_selected_setup() {
    for dark in [false, true] {
        let (mut h, worker, destination) = setup(dark);
        show_listing(&mut h, &worker, &destination);
        h.step(&json!({"click":{"handle":"cloud:add_storage"}})).unwrap();
        h.frames(2);
        h.step(&json!({"click":{"handle":"dialog:list:cloud_provider_git"}})).unwrap();
        h.frames(2);
        assert!(h.app.cloud.git_setup.is_some());
        assert!(!h.app.prefs_window.open);
        assert!(h.app.cloud.setup.is_none());
        h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
        h.frames(2);
        h.step(&json!({"click":{"handle":"cloud:add_storage"}})).unwrap();
        h.frames(2);
        h.step(&json!({"click":{"handle":"dialog:list:cloud_provider_github"}})).unwrap();
        h.frames(2);
        assert!(h.app.cloud.setup.is_some());
        assert!(h.app.cloud.git_setup.is_none());
        assert!(!h.app.prefs_window.open);
    }
}

#[test]
fn home_controls_have_consistent_heights_and_left_alignment() {
    for dark in [false, true] {
        let (mut h, worker, destination) = setup(dark);
        show_listing(&mut h, &worker, &destination);
        let connect = crate::scenario::rect_of("cloud:connect").unwrap();
        let repository = crate::scenario::rect_of("cloud:repositories").unwrap();
        assert!(connect.width() < repository.width(), "Add storage is an action-sized dropdown rather than a full-width form field");
        assert!((connect.left() - repository.left()).abs() < 1.0, "repository selector aligns with actions");
        for id in ["cloud:save", "cloud:settings", "cloud:repositories", "cloud:sync"] {
            let rect = crate::scenario::rect_of(id).unwrap();
            assert!((rect.height() - connect.height()).abs() < 1.0, "{id}: {rect:?} versus {connect:?}");
        }
        for width in [740.0, 480.0] {
            h.call("ui.resize", json!({"width":width,"height":1100}));
            h.frames(3);
            let page = crate::scenario::rect_of("home:page").unwrap();
            let controls: Vec<_> = ["cloud:connect", "cloud:save", "cloud:settings", "cloud:repositories", "cloud:sync"]
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
fn cloud_status_dots_remain_visible_and_outdated_rows_wait_for_synchronization() {
    use ui_kit::FileStatusTone;
    for dark in [false, true] {
        let (mut h, worker, destination) = setup(dark);
        h.ctx.global_style_mut(|style| style.animation_time = 0.0);
        let save = solidcraft_cloud::PendingSave::stage(
            &Connection::default(),
            &destination.project,
            &destination.file,
            None,
            solidcraft_engine::io::cloud::prepare_document(&Document::new("Bracket")).unwrap(),
        )
        .unwrap();
        let cache = solidcraft_cloud::CachedEntry::new(save.record.clone(), destination.clone(), Some(save.record.id), true).unwrap();
        let mut catalog = solidcraft_cloud::stored_designs(vec![cache], vec![]).unwrap();
        catalog[0].phase = Phase::Ready;
        catalog[0].outdated = true;
        show_listing(&mut h, &worker, &destination);
        let c = selected(&h.app).unwrap();
        request(&mut h.app, &c);
        let list = latest(&worker, "list");
        list.update(|s| {
            s.phase = Phase::Ready;
            s.result = Some(json!([destination]));
            s.catalog = catalog.clone();
        })
        .unwrap();
        h.frames(3);
        for width in [1500, 480] {
            h.call("ui.resize", json!({"width":width,"height":980}));
            h.frames(2);
            let dot = crate::scenario::rect_of("cloud:design:0:status-dot").unwrap();
            let expected = FileStatusTone::Attention.color();
            assert!(
                h.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Circle(c) if c.center == dot.center() && c.fill == expected)),
                "outdated dot stays red at {width} in either theme"
            );
            click_cloud(&mut h, "cloud:design:0");
            assert!(!h.app.file_load.busy(), "outdated rows cannot open before pulling");
            assert_eq!(status::presentation(h.app.cloud.browser.catalogs["github"].stored.first(), false).label, tr("cloud.status_outdated"));
        }
        let sync = h.app.run("cloud.sync", json!({"connection":"github"})).unwrap();
        let job = worker.jobs.lock().unwrap().iter().find(|(j, _)| j.status().unwrap().operation == sync["operation"]).unwrap().0.clone();
        job.update(|s| s.phase = Phase::Syncing).unwrap();
        h.frames(2);
        assert_eq!(status::presentation(h.app.cloud.browser.catalogs["github"].stored.first(), true).label, tr("cloud.status_pulling"));
        job.update(|s| s.phase = Phase::Synced).unwrap();
        h.frames(2);
        catalog[0].outdated = false;
        catalog[0].phase = Phase::Synced;
        latest(&worker, "list")
            .update(|s| {
                s.phase = Phase::Ready;
                s.result = Some(json!([destination]));
                s.catalog = catalog;
            })
            .unwrap();
        h.frames(3);
        let dot = crate::scenario::rect_of("cloud:design:0:status-dot").unwrap();
        assert!(
            h.shapes
                .iter()
                .any(|s| matches!(&s.shape, egui::Shape::Circle(c) if c.center == dot.center() && c.fill == FileStatusTone::Current.color()))
        );
        click_cloud(&mut h, "cloud:design:0");
        assert!(h.app.file_load.busy(), "a synchronized row becomes openable again");
    }
}

#[test]
fn queued_cloud_saves_show_a_short_yellow_pushing_status() {
    let (mut h, worker, destination) = setup(true);
    h.ctx.global_style_mut(|style| style.animation_time = 0.0);
    show_listing(&mut h, &worker, &destination);
    let save = solidcraft_cloud::PendingSave::stage(
        &Connection::default(),
        &destination.project,
        &destination.file,
        None,
        solidcraft_engine::io::cloud::prepare_document(&Document::new("Queued bracket")).unwrap(),
    )
    .unwrap();
    let c = selected(&h.app).unwrap();
    request(&mut h.app, &c);
    latest(&worker, "list")
        .update(|s| {
            s.phase = Phase::Ready;
            s.result = Some(json!([destination]));
            s.catalog = solidcraft_cloud::stored_designs(vec![], vec![save.record.clone()]).unwrap();
        })
        .unwrap();
    h.frames(3);
    let state = status::presentation(h.app.cloud.browser.catalogs["github"].stored.first(), false);
    assert_eq!(state.label, tr("cloud.status_pushing"));
    assert_eq!(state.tone, ui_kit::FileStatusTone::Pending);
    let dot = crate::scenario::rect_of("cloud:design:0:status-dot").unwrap();
    assert!(h.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Circle(c) if c.center == dot.center() && c.fill == state.tone.color())));
    click_cloud(&mut h, "cloud:design:0");
    assert!(h.app.file_load.busy(), "queued local work stays accessible");
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
        let connection = selected(&h.app).unwrap();
        request(&mut h.app, &connection);
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
    click_cloud(&mut h, "cloud:design:0");
    let job = latest(&worker, "open");
    assert!(!h.app.home.open, "cloud opening enters the workspace before progress appears, like local opening");
    assert_eq!(h.app.session.doc.name, "Unsaved first tab", "the existing design stays intact until the download succeeds");
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
    h.settle();
    assert_eq!(h.app.session.doc.name, "Cloud bracket");
    assert!(h.app.open_meshing.is_some(), "cloud designs use the same post-adoption meshing as local files");
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
        click_cloud(&mut h, "cloud:design:0");
        h.frames(2);
        assert!(h.app.cloud.browser.versions.is_some());
        assert!(!worker.jobs.lock().unwrap().iter().any(|(_, op)| matches!(op, Operation::Open { .. })));
        h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
        assert!(h.app.cloud.browser.versions.is_none());
        click_cloud(&mut h, "cloud:design:0");
        h.frames(2);
        h.step(&json!({"click":{"handle":"cloud:version:1"}})).unwrap();
        let selected = h.app.cloud.browser.catalogs["github"].stored[0].local_revisions[1].revision.clone();
        click_cloud(&mut h, "dialog:ok");
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
        h.settle();
        assert_eq!(h.app.session.cloud_binding().unwrap().revision.as_ref(), Some(&selected));
        assert!(!h.app.session.is_dirty());
    }
}

#[test]
fn save_picker_selects_existing_projects_or_reveals_a_new_name_field() {
    for dark in [false, true] {
        let (mut h, worker, destination) = setup(dark);
        show_listing(&mut h, &worker, &destination);
        h.app.start("cloud.save");
        h.frames(3);
        assert!(crate::scenario::rect_of("cloud:project").is_none(), "existing projects use a dropdown, not an editable name");
        let selector = crate::scenario::rect_of("cloud:project-choice").unwrap();
        let repository = crate::scenario::rect_of("cloud:save-repository").unwrap();
        assert!((selector.left() - repository.left()).abs() < 1.0);
        assert!((selector.width() - repository.width()).abs() < 1.0);
        click_cloud(&mut h, "cloud:project-choice");
        let existing = crate::scenario::rect_of("cloud:project-option:Robot").unwrap();
        let new = crate::scenario::rect_of("cloud:project-new").unwrap();
        assert!(new.top() > existing.bottom(), "New is the final dropdown row");
        click_cloud(&mut h, "cloud:project-new");
        let name = crate::scenario::rect_of("cloud:project").unwrap();
        let selector = crate::scenario::rect_of("cloud:project-choice").unwrap();
        assert!(name.top() > selector.bottom());
        assert!((name.left() - selector.left()).abs() < 1.0);
        assert!(!destination_valid(&h));
        click_cloud(&mut h, "cloud:project");
        h.call("ui.text", json!({"text":"New workshop"}));
        h.frames(2);
        assert!(destination_valid(&h));
        click_cloud(&mut h, "cloud:project-choice");
        click_cloud(&mut h, "cloud:project-option:Robot");
        assert!(crate::scenario::rect_of("cloud:project").is_none());
        assert_eq!(h.app.cloud.browser.picker.as_ref().unwrap().project, "Robot");
        click_cloud(&mut h, "cloud:save-repository");
        click_cloud(&mut h, "cloud:save-repository:work");
        assert_eq!(h.app.cloud.browser.picker.as_ref().unwrap().connection, "work");
        assert!(crate::scenario::rect_of("cloud:project").is_some(), "an empty repository starts with New selected");
        click_cloud(&mut h, "cloud:project-choice");
        assert!(crate::scenario::rect_of("cloud:project-option:Robot").is_none(), "projects belong to the selected repository");
        click_cloud(&mut h, "cloud:project-new");
        click_cloud(&mut h, "cloud:project");
        h.call("ui.key", json!({"key":"A","ctrl":true}));
        h.call("ui.text", json!({"text":"Work projects"}));
        h.frames(2);
        click_cloud(&mut h, "dialog:ok");
        let jobs = worker.jobs.lock().unwrap();
        let (_, Operation::Save { save, .. }) = jobs.iter().rev().find(|(_, op)| matches!(op, Operation::Save { .. })).unwrap() else {
            panic!("save required")
        };
        assert_eq!(save.record.destination.connection, "work");
        assert_eq!(save.record.destination.project, "Work projects");
    }
}

fn destination_valid(h: &Harness) -> bool {
    destination(&h.app, h.app.cloud.browser.picker.as_ref().unwrap()).2
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
        assert_eq!(phase(&h.app.cloud.browser.catalogs["github"].stored[0]), tr("cloud.status_pushing"));
        let connection = selected(&h.app).unwrap();
        request(&mut h.app, &connection);
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
    let (job, Operation::Thumbnail { destination, .. }) = jobs.last().unwrap() else { panic!("preview") };
    let (job, destination) = (job.clone(), destination.clone());
    drop(jobs);
    assert!(!previews::failed(&h.app, &destination), "a loading preview is not a failed preview");
    job.fail(solidcraft_cloud::Error::Storage("preview unavailable".into())).unwrap();
    previews::poll(&mut h.app, &[job.status().unwrap()]);
    assert!(previews::failed(&h.app, &destination), "the shared file row receives the terminal preview state");
    previews::clear(&mut h.app, &destination.connection);
    assert!(!previews::failed(&h.app, &destination), "refresh permits another preview attempt");
}

#[test]
fn home_loading_buttons_keep_their_positions_and_wait_for_terminal_results() {
    for dark in [false, true] {
        for width in [1500, 480] {
            let (mut h, worker, destination) = setup(dark);
            h.call("ui.resize", json!({"width":width,"height":1100}));
            show_listing(&mut h, &worker, &destination);
            let ids = ["cloud:connect", "cloud:save", "cloud:settings", "cloud:repositories", "cloud:sync"];
            let idle: Vec<_> = ids.iter().map(|id| crate::scenario::rect_of(id).unwrap()).collect();
            let count = || worker.jobs.lock().unwrap().iter().filter(|(_, op)| matches!(op, Operation::List(_))).count();
            let connection = selected(&h.app).unwrap();
            request(&mut h.app, &connection);
            let pending = latest(&worker, "list");
            let before = count();
            h.frames(3);
            assert_eq!(crate::scenario::count_of("cloud:sync:enabled"), Some(0.0));
            for (id, rect) in ids.iter().zip(&idle) {
                assert_eq!(crate::scenario::rect_of(id), Some(*rect), "loading must not move {id} at {width}");
            }
            h.step(&json!({"click":{"handle":"cloud:sync"}})).unwrap();
            assert_eq!(count(), before, "Sync cannot start while its catalog is pending");
            pending
                .update(|s| {
                    s.phase = Phase::Failed;
                    s.message = Some("request timed out".into());
                })
                .unwrap();
            h.frames(3);
            assert_eq!(crate::scenario::count_of("cloud:sync:enabled"), Some(1.0));
            h.app.status = None;
            h.frames(3);
            for terminal in [Phase::Synced, Phase::Failed, Phase::Cancelled] {
                h.step(&json!({"click":{"handle":"cloud:sync"}})).unwrap();
                let job = worker.jobs.lock().unwrap().iter().rev().find(|(_, op)| matches!(op, Operation::Synchronize(_))).unwrap().0.clone();
                let before = worker.jobs.lock().unwrap().len();
                h.frames(3);
                assert_eq!(crate::scenario::count_of("cloud:sync:enabled"), Some(0.0));
                for (id, rect) in ids.iter().zip(&idle) {
                    assert_eq!(crate::scenario::rect_of(id), Some(*rect), "synchronization must not move {id} at {width}");
                }
                h.step(&json!({"click":{"handle":"cloud:sync"}})).unwrap();
                assert_eq!(worker.jobs.lock().unwrap().len(), before);
                job.update(|s| s.phase = terminal).unwrap();
                h.frames(3);
                assert_eq!(crate::scenario::count_of("cloud:sync:enabled"), Some(f64::from(terminal != Phase::Synced)));
                // Successful synchronization refreshes the catalog; settle that request too.
                latest(&worker, "list")
                    .update(|s| {
                        s.phase = Phase::Ready;
                        s.result = Some(json!([destination]));
                    })
                    .unwrap();
                h.frames(3);
            }
        }
    }
}

#[test]
fn cloud_document_tabs_follow_captures_edits_background_tabs_and_newer_heads() {
    for dark in [false, true] {
        let (mut h, worker, destination) = setup(dark);
        h.app.home.open = false;
        h.frames(2);
        assert!(tabs::indicator(&h.app, &h.app.session).is_none(), "native documents keep their ordinary tab");
        h.app.session.doc_mut().name = "Local modified".into();
        h.frames(2);
        assert!(h.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Local modified •")));
        assert!(crate::scenario::rect_of("tab:design:0:sync-dot").is_none(), "local files retain their original white unsaved marker");
        h.app.run("cloud.save", json!({"destination":destination})).unwrap();
        h.frames(2);
        assert_eq!(tabs::indicator(&h.app, &h.app.session).unwrap().status.label, tr("cloud.status_saving"));
        assert!(
            !h.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Local modified •")),
            "cloud tabs replace the white marker instead of adding a second dot"
        );
        let dot = crate::scenario::rect_of("tab:design:0:sync-dot").unwrap();
        let name = h
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == "Local modified" => Some(text),
                _ => None,
            })
            .unwrap();
        assert!(dot.left() >= name.pos.x + name.galley.size().x, "the colored dot follows the name, in the ordinary unsaved marker position");
        let (job, record) = worker
            .jobs
            .lock()
            .unwrap()
            .iter()
            .find_map(|(job, operation)| if let Operation::Save { save, .. } = operation { Some((job.clone(), save.record.clone())) } else { None })
            .unwrap();
        job.update(|s| {
            s.saved_locally = true;
            s.destination = Some(destination.clone());
            s.revision = Some(record.id.clone());
            s.phase = Phase::SavedLocally;
        })
        .unwrap();
        h.frames(2);
        let indicator = tabs::indicator(&h.app, &h.app.session).unwrap();
        assert_eq!(indicator.status.label, tr("cloud.status_pushing"));
        assert_eq!(indicator.status.tone, ui_kit::FileStatusTone::Unsaved);
        assert!(indicator.status.pulse);
        assert!(indicator.tooltip.contains("Robot / parts/bracket"));
        h.app
            .cloud
            .browser
            .pending_since
            .insert(tabs::key(&destination, Some(&record.id)), web_time::Instant::now() - std::time::Duration::from_secs(31));
        let indicator = tabs::indicator(&h.app, &h.app.session).unwrap();
        assert_eq!(indicator.status.tone, ui_kit::FileStatusTone::Pending, "a long push becomes orange");
        assert!(indicator.status.pulse);
        job.fail(solidcraft_cloud::Error::Storage("network unavailable".into())).unwrap();
        h.frames(2);
        let indicator = tabs::indicator(&h.app, &h.app.session).unwrap();
        assert_eq!(indicator.status.tone, ui_kit::FileStatusTone::Attention, "a failed push becomes red");
        assert!(!indicator.status.pulse);
        job.update(|s| s.message = None).unwrap();
        job.update(|s| s.phase = Phase::Synced).unwrap();
        h.frames(2);
        assert!(tabs::indicator(&h.app, &h.app.session).is_none(), "saved cloud tabs look like saved local tabs");
        assert!(crate::scenario::rect_of("tab:design:0:sync-dot").is_none());
        let cached = solidcraft_cloud::CachedEntry::new(record.clone(), destination.clone(), Some(record.id.clone()), false).unwrap();
        let connection = selected(&h.app).unwrap();
        request(&mut h.app, &connection);
        latest(&worker, "list")
            .update(|s| {
                s.phase = Phase::Ready;
                s.result = Some(json!([destination]));
                s.catalog = solidcraft_cloud::stored_designs(vec![cached], vec![]).unwrap();
            })
            .unwrap();
        h.frames(2);
        assert!(tabs::indicator(&h.app, &h.app.session).is_none(), "a clean copy received from another client also has no dot");
        let original = h.app.session.doc.name.clone();
        h.app.session.doc_mut().name = "Edited during upload".into();
        let indicator = tabs::indicator(&h.app, &h.app.session).unwrap();
        assert_eq!(indicator.status.label, crate::i18n::tr_label("Not saved yet"));
        assert_eq!(indicator.status.tone, ui_kit::FileStatusTone::Unsaved);
        h.app.session.doc_mut().name = original.clone();
        documents::open_tab(&mut h.app, Session::default()).unwrap();
        h.frames(2);
        assert!(crate::scenario::rect_of("tab:design:0:sync-dot").is_none(), "saved background cloud tabs also have no dot");
        documents::switch(&mut h.app, 0);
        h.app.session.doc_mut().name = "Unsaved cloud edit".into();
        documents::switch(&mut h.app, 1);
        h.frames(2);
        assert!(crate::scenario::rect_of("tab:design:0:sync-dot").is_some(), "a background cloud tab keeps its status");
        assert!(tabs::indicator(&h.app, &h.app.session).is_none());
        documents::switch(&mut h.app, 0);
        for width in [1500, 480] {
            h.call("ui.resize", json!({"width":width,"height":980}));
            h.frames(2);
            let dot = crate::scenario::rect_of("tab:design:0:sync-dot").unwrap();
            let tab = crate::scenario::rect_of("tab:design:0").unwrap();
            let close = crate::scenario::rect_of("tab:close:0").unwrap();
            assert!(tab.contains_rect(dot) && !dot.intersects(close), "the sync dot fits without moving the close target");
        }
        h.app.session.doc_mut().name = original;
        show_listing(&mut h, &worker, &destination);
        let c = selected(&h.app).unwrap();
        request(&mut h.app, &c);
        let cache = solidcraft_cloud::CachedEntry::new(record, destination.clone(), Some("b".repeat(64)), true).unwrap();
        latest(&worker, "list")
            .update(|s| {
                s.phase = Phase::Ready;
                s.result = Some(json!([destination]));
                s.catalog = solidcraft_cloud::stored_designs(vec![cache], vec![]).unwrap();
            })
            .unwrap();
        h.frames(2);
        let indicator = tabs::indicator(&h.app, &h.app.session).unwrap();
        assert_eq!(indicator.status.label, tr("cloud.status_outdated"), "a newer cached head does not make the open older revision current");
        assert!(indicator.status.pulse);
    }
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn cloud_open_uses_the_file_worker_and_keeps_loading_frames_responsive() {
    let (mut h, worker, destination) = setup(true);
    h.app.session.doc_mut().name = "Keep my edits".into();
    let original = h.app.session.doc.clone();
    let (started, on_worker) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    h.app.file_load.worker_gate = Some((started, released));
    open_revision(&mut h.app, &destination, None);
    assert_ne!(on_worker.recv_timeout(std::time::Duration::from_secs(5)).unwrap(), std::thread::current().id());
    let job = latest(&worker, "open");
    job.update(|s| {
        s.destination = Some(destination.clone());
        s.revision = Some("a".repeat(64));
        s.download_progress = Some((2, 5));
    })
    .unwrap();
    h.frames(4);
    assert!(h.app.file_load.busy());
    assert!(crate::scenario::handle_at("file_loading:cancel").is_some(), "normal file-loading progress is shown during the sync check");
    assert!(Arc::ptr_eq(&original, &h.app.session.doc));
    job.opened(solidcraft_engine::io::cloud::prepare_document(&Document::new("Opened locally")).unwrap()).unwrap();
    release.send(()).unwrap();
    h.settle();
    assert_eq!(h.app.session.doc.name, "Opened locally");
    assert_eq!(h.app.docs.count(), 2);
}
