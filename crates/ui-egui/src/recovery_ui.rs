//! Autosave ticks and the "Recover unsaved design?" window shown at launch.

use serde_json::json;
use solidcraft_engine::recovery::{self, Autosaver, Entry};
use solidcraft_engine::{Result, Session};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::SolveApp;
use crate::ui_kit;

/// A command that ran at least this long counts as a big operation: autosave right after it.
pub const BIG_OPERATION_MS: f64 = 1500.0;

/// A tab's autosaver has no clock of its own: [`tick`] tells it when its turn has come.
const SHARED_CADENCE: Duration = Duration::MAX;

/// Shared settings and cadence; each tab owns its own entry and change tracking.
pub struct Autosaves {
    pub dir: PathBuf,
    pub interval: Duration,
    pub(crate) active: Option<Autosaver>,
    last_at: Instant,
}

impl Autosaves {
    /// Save the active design immediately, including when taking over a recovered entry.
    pub fn save(&mut self, session: &Session) -> Result<bool> {
        save_slot(&self.dir, &mut self.active, session, true, true)
    }
}

/// Bring one tab's autosave up to date. `due`: its turn has come. `wait`: write on this thread
/// and wait for it (Save, close, shutdown); otherwise the write runs on a worker thread, so a
/// scheduled autosave of several large designs never holds up the window (#3044).
fn save_slot(dir: &Path, slot: &mut Option<Autosaver>, session: &Session, due: bool, wait: bool) -> Result<bool> {
    // Clean tabs need no serialization or entry, even between scheduled saves.
    if !session.is_dirty() {
        if let Some(mut a) = slot.take() {
            // Rebase the active crash journal before deleting the autosave it may refer to.
            a.save(session)?;
            a.discard();
        }
        return Ok(false);
    }
    if !due {
        return Ok(false);
    }
    if slot.is_none() {
        *slot = Some(Autosaver::new(dir, SHARED_CADENCE)?);
    }
    match slot.as_mut() {
        Some(a) if wait => a.save(session),
        // Skips a tab whose previous write is still running.
        Some(a) => a.tick(session, true),
        None => Ok(false),
    }
}

/// Autosave every open design on the shared cadence, or a tab after its big operation.
pub fn tick(app: &mut SolveApp) {
    let big = std::mem::take(&mut app.big_operation);
    let Some(a) = app.autosave.as_mut() else { return };
    let due = a.last_at.elapsed() >= a.interval;
    if due {
        a.last_at = Instant::now();
    }
    let mut error = None;
    for slot in app.docs.background_mut() {
        let big = std::mem::take(&mut slot.big_operation);
        if let Err(e) = save_slot(&a.dir, &mut slot.autosave, &slot.session, due || big, false) {
            error.get_or_insert(e);
        }
    }
    // Background writes must not replace the active design's crash-journal checkpoint.
    if let Err(e) = save_slot(&a.dir, &mut a.active, &app.session, due || big, false) {
        error.get_or_insert(e);
    }
    if let Some(e) = error {
        let msg = format!("autosave failed: {e}");
        if app.status.as_ref().is_none_or(|(s, _, _)| *s != msg) {
            app.set_status(msg, true);
        }
    }
}

/// Start autosaving into `dir` and queue the designs that crashed apps left there.
pub fn start(app: &mut SolveApp, dir: &std::path::Path, interval: std::time::Duration) {
    match Autosaver::new(dir, SHARED_CADENCE) {
        Ok(a) => {
            app.recoverable = recovery::orphans(dir);
            app.autosave = Some(Autosaves { dir: dir.to_path_buf(), interval, active: Some(a), last_at: Instant::now() });
        }
        Err(e) => app.set_status(format!("autosave is off: {e}"), true),
    }
}

/// The app is closing: keep unsaved work for the next launch, drop everything else.
pub fn close(app: &mut SolveApp) {
    if let Some(mut a) = app.autosave.take() {
        for slot in app.docs.background_mut() {
            let _ = save_slot(&a.dir, &mut slot.autosave, &slot.session, true, true);
            if let Some(saver) = slot.autosave.take() {
                saver.close(&slot.session);
            }
        }
        let _ = a.save(&app.session);
        if let Some(saver) = a.active.take() {
            saver.close(&app.session);
        }
    }
}

pub(crate) fn ago(secs: u64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(secs);
    let d = now.saturating_sub(secs);
    match d {
        0..=59 => crate::i18n::tr("recovery.just_now").to_string(),
        60..=3599 => crate::i18n::format("recovery.min_ago", &[("count", &(d / 60).to_string())]),
        3600..=86_399 => crate::i18n::format("recovery.hours_ago", &[("count", &(d / 3600).to_string())]),
        _ => crate::i18n::format("recovery.days_ago", &[("count", &(d / 86_400).to_string())]),
    }
}

pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    // File view presents the same entries inline, without a second window covering them.
    if app.home.open || app.recoverable.is_empty() {
        app.recovery_discard_all = false;
        return;
    }
    if app.recovery_discard_all {
        confirm_discard_all(app, ctx);
        return;
    }
    let mut act: Option<(Entry, bool)> = None;
    let mut discard_all = false;
    let (closed, action) = ui_kit::Window::new(egui::Id::new("sc_recover_prompt"), "restore", crate::i18n::tr_label("Recover unsaved design?"))
        .actions("", crate::i18n::tr_label("Later"), None)
        .show_with_left(
            ctx,
            true,
            |ui| {
                ui_kit::text(
                    ui,
                    crate::i18n::tr_label("SolidCraft closed before these designs were saved. Their last autosave can be opened."),
                    ui_kit::Text::Report("intro", false),
                );
                for (index, e) in app.recoverable.iter().take(20).enumerate() {
                    ui_kit::divider(ui, &format!("entry:{index}"));
                    ui_kit::label_actions(ui, &e.name, |ui| {
                        if ui_kit::button(ui, crate::i18n::tr_label("Recover"), ui_kit::Button::Normal).clicked() {
                            act = Some((e.clone(), true));
                        }
                        if ui_kit::button(ui, crate::i18n::tr_label("Discard"), ui_kit::Button::Normal).clicked() {
                            act = Some((e.clone(), false));
                        }
                    });
                    let file = e.path.clone().unwrap_or_else(|| crate::i18n::tr("recovery.never_saved").to_string());
                    let detail =
                        crate::i18n::format("recovery.detail", &[("file", &file), ("features", &e.features.to_string()), ("when", &ago(e.saved_at))]);
                    ui_kit::text(ui, &detail, ui_kit::Text::Note(&format!("detail:{index}")));
                }
            },
            // Away from Later: it asks first, then discards every listed autosave (#9299).
            |ui| {
                let response = ui_kit::button(ui, crate::i18n::tr("recovery.discard_all"), ui_kit::Button::Normal);
                crate::scenario::publish_target("recovery:discard_all", response.rect);
                discard_all = response.clicked();
            },
        );
    if discard_all {
        app.recovery_discard_all = true;
        return;
    }
    let later = closed || action != ui_kit::Action::None;
    if let Some((entry, recover)) = act {
        apply(app, entry, recover);
    }
    if later {
        app.recoverable.clear();
    }
}

/// Discard all asks first; Cancel (or the close button) goes back to the list unchanged.
fn confirm_discard_all(app: &mut SolveApp, ctx: &egui::Context) {
    let count = app.recoverable.len();
    let title = if count == 1 {
        crate::i18n::tr("recovery.discard_all_title_one").to_string()
    } else {
        crate::i18n::format("recovery.discard_all_title", &[("count", &count.to_string())])
    };
    let (closed, action) = ui_kit::Confirm::new(egui::Id::new("sc_recover_discard_all"), "delete", &title)
        .tone(ui_kit::Tone::Danger)
        .actions(crate::i18n::tr("recovery.discard_all"), crate::i18n::tr("ui.cancel"), None)
        .show(ctx, true, |ui| {
            ui_kit::text(ui, crate::i18n::tr("recovery.discard_all_detail"), ui_kit::Text::Report("discard_all", false));
        });
    match action {
        ui_kit::Action::Ok => {
            app.recovery_discard_all = false;
            discard_all(app);
        }
        ui_kit::Action::Cancel => app.recovery_discard_all = false,
        _ if closed => app.recovery_discard_all = false,
        _ => {}
    }
}

/// Discard every listed autosave, each the way its own Discard button does.
fn discard_all(app: &mut SolveApp) {
    for entry in std::mem::take(&mut app.recoverable) {
        apply(app, entry, false);
    }
}

/// Shared recovery action for the launch prompt and File view; dirty work is never replaced.
pub(crate) fn apply(app: &mut SolveApp, entry: Entry, recover: bool) {
    let dir_s = app.autosave.as_ref().map(|a| a.dir.clone()).or_else(recovery::default_dir).map(|d| d.to_string_lossy().to_string());
    app.recoverable.retain(|x| x.id != entry.id);
    if recover {
        if app.session.is_dirty() {
            app.set_status("save or discard the open design's changes before recovering another", true);
            app.recoverable.insert(0, entry);
            return;
        }
        // Rebuilt off the window thread like any other file; once it is in, it is
        // autosaved under this app's entry before the old entry goes.
        let kind = crate::file_load::Kind::Recover { id: entry.id.clone(), dir: dir_s };
        let _ = crate::file_load::begin_request(app, crate::file_load::Request::new(&entry.name, kind));
    } else {
        let _ = app.run("doc.recovery_discard", json!({"id": entry.id, "dir": dir_s}));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Services, documents};
    use std::fs;

    fn app(tag: &str, interval: Duration) -> (SolveApp, PathBuf) {
        let dir = std::env::temp_dir().join(format!("solidcraft-recovery-tabs-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut app = SolveApp::new(Session::default(), Services::default());
        start(&mut app, &dir, interval);
        (app, dir)
    }

    fn edit(app: &mut SolveApp) {
        app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        // Tests control the cadence without relying on how long the command took.
        app.big_operation = false;
    }

    fn entries(dir: &Path) -> Vec<Entry> {
        let mut entries: Vec<Entry> = fs::read_dir(dir)
            .unwrap()
            .map(|p| p.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .map(|p| serde_json::from_slice(&fs::read(p).unwrap()).unwrap())
            .collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        entries
    }

    /// A tick, then wait for the writes it started on worker threads. Waiting writes nothing
    /// new: every design it would write was just written.
    fn tick_and_wait(app: &mut SolveApp) {
        tick(app);
        let Some(a) = app.autosave.as_mut() else { return };
        for slot in app.docs.background_mut() {
            if let Some(saver) = slot.autosave.as_mut() {
                let _ = saver.save(&slot.session);
            }
        }
        if let Some(saver) = a.active.as_mut() {
            let _ = saver.save(&app.session);
        }
    }

    fn name(app: &mut SolveApp, name: &str) {
        app.run("file.new", json!({"name": name})).unwrap();
    }

    #[test]
    fn scenario_crash_checkpoints_the_latest_zero_interval_autosave() {
        let mut h = crate::scenario::Harness::new();
        start(&mut h.app, &h.dir.join("recovery"), Duration::ZERO);
        edit(&mut h.app);
        h.app.autosave.as_mut().unwrap().save(&h.app.session).unwrap();
        let saved = h.app.session.doc.features.len();
        // The previous snapshot is saved, but the latest edit has not had a UI tick yet.
        edit(&mut h.app);
        let latest = h.app.session.doc.features.len();
        assert!(latest > saved);
        h.step(&json!({"restart":"crash"})).unwrap();
        h.run(&json!([{ "widget":"Recover" }])).unwrap();
        assert_eq!(h.app.session.doc.features.len(), latest, "the simulated crash must follow the zero-interval checkpoint");
    }

    #[test]
    fn dirty_background_tab_is_saved_and_offered_after_a_crash() {
        let (mut app, dir) = app("background", Duration::ZERO);
        name(&mut app, "Background A");
        edit(&mut app);
        let expected = app.session.doc.clone();
        documents::new_tab(&mut app);
        assert!(!app.session.is_dirty());
        tick_and_wait(&mut app);
        assert_eq!(entries(&dir).len(), 1);
        assert!(recovery::orphans(&dir).is_empty(), "all live entries stay locked");
        drop(app); // Simulate a crash: release locks without a final save.
        let restarted = restart(&dir);
        assert_eq!(restarted.recoverable.len(), 1);
        let entry = &restarted.recoverable[0];
        assert_eq!(entry.name, "Background A");
        assert_eq!(recovery::load(&dir, &entry.id).unwrap().0, *expected);
        drop(restarted);
        fs::remove_dir_all(dir).unwrap();
    }

    fn restart(dir: &Path) -> SolveApp {
        let mut app = SolveApp::new(Session::default(), Services::default());
        start(&mut app, dir, Duration::ZERO);
        app
    }

    #[test]
    fn switching_to_a_clean_tab_keeps_the_background_entry_and_its_id() {
        let (mut app, dir) = app("clean-tab", Duration::ZERO);
        edit(&mut app);
        tick_and_wait(&mut app);
        let before = entries(&dir);
        documents::new_tab(&mut app);
        tick_and_wait(&mut app);
        assert_eq!(entries(&dir), before, "the clean tab cannot delete another tab's entry");
        documents::switch(&mut app, 0);
        tick_and_wait(&mut app);
        assert_eq!(entries(&dir), before, "switching back preserves identity and skips unchanged content");
        drop(app);
        assert_eq!(recovery::orphans(&dir), before);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn several_dirty_tabs_keep_distinct_entries_and_recover_their_own_designs() {
        let (mut app, dir) = app("several", Duration::ZERO);
        let mut expected = Vec::new();
        for i in 0..3 {
            if i > 0 {
                documents::new_tab(&mut app);
            }
            name(&mut app, &format!("Design {i}"));
            for _ in 0..=i {
                edit(&mut app);
            }
            expected.push(app.session.doc.clone());
        }
        tick_and_wait(&mut app);
        let saved = entries(&dir);
        assert_eq!(saved.len(), 3);
        let ids: std::collections::HashSet<_> = saved.iter().map(|e| &e.id).collect();
        assert_eq!(ids.len(), 3);
        for i in [0, 2, 1, 0] {
            documents::switch(&mut app, i);
            tick_and_wait(&mut app);
            assert_eq!(entries(&dir), saved);
        }
        drop(app);
        let mut restarted = restart(&dir);
        assert_eq!(restarted.recoverable.len(), 3);
        for (entry, doc) in saved.iter().zip(expected) {
            assert_eq!(recovery::load(&dir, &entry.id).unwrap().0, *doc);
            restarted.run("doc.recover", json!({"dir": dir, "id": entry.id})).unwrap();
            assert_eq!(*restarted.session.doc, *doc);
            assert!(restarted.session.is_dirty());
        }
        drop(restarted);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn saving_and_closing_remove_only_the_affected_tabs_entry() {
        let (mut app, dir) = app("cleanup", Duration::ZERO);
        for i in 0..3 {
            if i > 0 {
                documents::new_tab(&mut app);
            }
            name(&mut app, &format!("Design {i}"));
            edit(&mut app);
        }
        tick_and_wait(&mut app);
        let saved = entries(&dir);
        // Cleanup is immediate, even when the next autosave is far away.
        app.autosave.as_mut().unwrap().interval = Duration::from_secs(3600);
        app.run("file.save_as", json!({"path": dir.join("saved.solidcraft")})).unwrap();
        tick_and_wait(&mut app);
        assert_eq!(entries(&dir), saved[..2]);
        documents::close(&mut app, 0, false);
        documents::answer_prompt(&mut app, documents::Answer::Cancel);
        assert_eq!(entries(&dir), saved[..2], "cancelling close keeps recovery");
        documents::close(&mut app, 0, true); // Discard a background tab.
        assert_eq!(entries(&dir), saved[1..2]);
        documents::switch(&mut app, 0);
        documents::close(&mut app, 0, true); // Discard the active tab.
        assert!(entries(&dir).is_empty());
        edit(&mut app);
        app.autosave.as_mut().unwrap().save(&app.session).unwrap();
        assert_eq!(entries(&dir).len(), 1);
        documents::close(&mut app, 0, true); // Last tab becomes a fresh blank design.
        assert!(entries(&dir).is_empty());
        drop(app);
        assert!(recovery::orphans(&dir).is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn shutdown_keeps_all_unsaved_tabs_but_confirmed_quit_discards_them() {
        for discard in [false, true] {
            let (mut app, dir) = app(if discard { "quit" } else { "shutdown" }, Duration::from_secs(3600));
            edit(&mut app);
            documents::new_tab(&mut app);
            edit(&mut app);
            if discard {
                // Make entries first, then exercise Don't Save for every design.
                app.autosave.as_mut().unwrap().interval = Duration::ZERO;
                tick_and_wait(&mut app);
                assert_eq!(entries(&dir).len(), 2);
                documents::request_quit(&mut app);
                documents::answer_prompt(&mut app, documents::Answer::DontSave);
                documents::answer_prompt(&mut app, documents::Answer::DontSave);
                assert!(app.quit_requested);
            }
            close(&mut app);
            assert_eq!(recovery::orphans(&dir).len(), if discard { 0 } else { 2 });
            assert!(app.docs.background_mut().all(|s| s.autosave.is_none()), "shutdown releases every lock");
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn cadence_and_big_operations_follow_the_document_and_preferences() {
        let (mut app, dir) = app("cadence", Duration::from_secs(3600));
        app.session.crash_journal = true;
        edit(&mut app);
        app.big_operation = true;
        documents::new_tab(&mut app);
        assert!(app.session.crash_journal);
        assert!(app.docs.background_mut().all(|s| !s.session.crash_journal));
        assert!(!app.big_operation);
        edit(&mut app);
        tick_and_wait(&mut app);
        assert_eq!(entries(&dir).len(), 1, "only the background big operation is due");
        assert!(app.docs.background_mut().all(|s| !s.big_operation));
        tick_and_wait(&mut app);
        assert_eq!(entries(&dir).len(), 1, "the ordinary active edit waits for the interval");
        app.autosave_minutes = 0.00001;
        crate::prefs::set_autosave(&mut app);
        app.autosave.as_mut().unwrap().last_at = Instant::now() - Duration::from_secs(1);
        tick_and_wait(&mut app);
        assert_eq!(entries(&dir).len(), 2, "changed preferences apply to every tab");
        drop(app);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn one_failed_autosave_does_not_skip_the_other_tabs() {
        let (mut app, dir) = app("failure", Duration::ZERO);
        name(&mut app, "A");
        edit(&mut app);
        tick_and_wait(&mut app);
        let id = entries(&dir)[0].id.clone();
        let blocked = dir.join(format!("{id}.sc"));
        fs::remove_file(&blocked).unwrap();
        fs::create_dir(&blocked).unwrap();
        edit(&mut app);
        documents::new_tab(&mut app);
        name(&mut app, "B");
        edit(&mut app);
        // The writes run on worker threads: a failure is reported by a later tick.
        let failed = |app: &SolveApp| app.status.as_ref().is_some_and(|(message, _, error)| *error && message.contains("autosave failed"));
        let deadline = Instant::now() + Duration::from_secs(60);
        tick(&mut app);
        while !failed(&app) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
            tick(&mut app);
        }
        assert!(failed(&app));
        tick_and_wait(&mut app);
        let saved = entries(&dir);
        assert_eq!(saved.len(), 2);
        assert_eq!(recovery::load(&dir, &saved[1].id).unwrap().0, *app.session.doc);
        // Retrying at the next tick also recovers the previously failing tab.
        fs::remove_dir(&blocked).unwrap();
        tick_and_wait(&mut app);
        assert_eq!(recovery::load(&dir, &id).unwrap().0.features.len(), 2);
        drop(app);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_tab_closed_while_its_autosave_is_written_leaves_nothing_to_recover() {
        let (mut app, dir) = app("discard-race", Duration::ZERO);
        name(&mut app, "Closed");
        // Enough of a design that its write is still running when the tab is closed.
        for i in 0..40 {
            app.run("solid.cylinder", json!({"radius": 3, "height": 5, "base": [i * 8, 0, 0], "operation": "new"})).unwrap();
        }
        app.big_operation = false;
        documents::new_tab(&mut app);
        tick(&mut app); // Starts writing the background tab on a worker thread.
        documents::close(&mut app, 0, true); // Don't Save, right away.
        let files = || fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name()).collect::<Vec<_>>();
        // A write that outlived the close would recreate the entry; give it time to.
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            assert!(entries(&dir).is_empty(), "the closed design came back: {:?}", files());
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(app);
        assert!(recovery::orphans(&dir).is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    fn entry(id: &str, name: &str) -> Entry {
        Entry { id: id.into(), path: None, name: name.into(), saved_at: 0, features: 3 }
    }

    #[test]
    fn file_view_recovery_is_inline_and_the_design_workspace_keeps_the_prompt() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = true;
        h.app.recoverable = vec![entry("a", "Bracket"), entry("b", "Housing")];
        h.frames(4);
        assert!(crate::scenario::handle_at("home:recovery:recover").is_some());
        assert!(h.widget("Later").is_none(), "the recovery window must not cover File view");
        assert_eq!(h.app.recoverable.len(), 2);
        h.step(&json!({"click": {"handle": "home:recover"}})).unwrap();
        h.frames(2);
        assert!(crate::scenario::handle_at("home:recovery:0:recover").is_some());
        assert!(crate::scenario::handle_at("home:recovery:1:discard").is_some());
        assert!(h.widget("Later").is_none());
        h.app.home.open = false;
        h.frames(4);
        assert!(h.widget("Later").is_some());
        assert_eq!(h.app.recoverable.len(), 2);
    }

    #[test]
    fn recovered_sessions_are_a_card_with_one_row_each_and_later_keeps_them_for_next_launch() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.recoverable = vec![entry("a", "Bracket"), entry("b", "Housing")];
        h.frames(4);
        assert!(crate::scenario::handle_at("dialog:close").is_some(), "a kit card");
        assert!(h.widget("Recover").is_some() && h.widget("Discard").is_some() && h.widget("Later").is_some());
        assert!(crate::scenario::handle_at("dialog:cell:Bracket").is_some(), "one row per design");
        assert!(crate::scenario::handle_at("dialog:cell:Housing").is_some());
        h.step(&json!({"widget": "Later"})).unwrap();
        h.frames(2);
        assert!(h.app.recoverable.is_empty(), "Later hides the card for this launch");
    }

    /// #9299: Discard all asks first; Cancel keeps every entry, confirming discards them all.
    #[test]
    fn discard_all_asks_first_then_discards_every_listed_autosave() {
        let (mut crashed, dir) = app("discard-all", Duration::ZERO);
        for i in 0..3 {
            if i > 0 {
                documents::new_tab(&mut crashed);
            }
            name(&mut crashed, &format!("Design {i}"));
            edit(&mut crashed);
        }
        tick_and_wait(&mut crashed);
        drop(crashed);
        assert_eq!(recovery::orphans(&dir).len(), 3);

        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        start(&mut h.app, &dir, Duration::from_secs(3600));
        h.frames(4);
        assert_eq!(h.app.recoverable.len(), 3);
        assert!(crate::scenario::handle_at("recovery:discard_all").is_some());
        h.step(&json!({"widget": "Discard all"})).unwrap();
        h.frames(2);
        assert!(h.app.recovery_discard_all, "Discard all opens its confirmation");
        assert!(h.widget("Cancel").is_some() && h.widget("Discard all").is_some(), "a confirmation first");
        assert!(h.widget("Discard 3 autosaves?").is_some(), "the title counts the autosaves");
        assert!(h.widget("Later").is_none(), "the confirmation replaces the list");
        assert_eq!(recovery::orphans(&dir).len(), 3, "nothing discarded before the answer");
        h.step(&json!({"widget": "Cancel"})).unwrap();
        h.frames(2);
        assert!(h.widget("Later").is_some(), "Cancel goes back to the list");
        assert_eq!(h.app.recoverable.len(), 3);
        assert_eq!(recovery::orphans(&dir).len(), 3);

        h.step(&json!({"widget": "Discard all"})).unwrap();
        h.frames(2);
        h.step(&json!({"widget": "Discard all"})).unwrap();
        h.frames(2);
        assert!(h.app.recoverable.is_empty());
        assert!(!h.app.recovery_discard_all);
        assert!(recovery::orphans(&dir).is_empty(), "every listed autosave is gone");
        assert!(h.widget("Later").is_none());
        drop(h);
        let _ = fs::remove_dir_all(dir);
    }
}
