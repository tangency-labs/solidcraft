//! Several open designs, as tabs in the application bar. The active design lives in the app as
//! always (`app.session`, `app.cam`…); the others wait here with their own session (undo,
//! selection), camera, agent conversation and per-design view state, and are swapped in when
//! their tab is picked.
//! Closing a design with unsaved changes asks first, and so does quitting: one "save changes?"
//! for the unsaved designs in this window ([`request_quit`]).

use crate::i18n::LocaleNames as _;
use egui::Rect;
use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_viewport::Camera;

use crate::SolveApp;
use crate::ui_kit;

/// Continue transient shot queues without adopting their document or view into the active tab.
pub(crate) fn advance_render_batches(app: &mut SolveApp, ctx: &egui::Context) {
    let mut error = None;
    for slot in app.docs.slots.iter_mut().flatten() {
        if let Err(failure) = slot.session.advance_render_batch() {
            error = Some(failure.to_string());
        }
        let queue = slot.session.render_queue_state();
        if queue.running != 0 || queue.waiting != 0 {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
    if let Some(error) = error {
        app.set_status(error, true);
    }
}

/// Most designs open at once.
pub const MAX_OPEN: usize = 32;

/// A design that is not the active one: everything that belongs to it alone.
pub struct DocSlot {
    pub(crate) session: Session,
    pub(crate) autosave: Option<solidcraft_engine::recovery::Autosaver>,
    pub(crate) big_operation: bool,
    cam: Camera,
    pub(crate) pending_feature_edit: Option<crate::feature_edit::PendingFeatureEdit>,
    pre_sketch_cam: Option<crate::PreSketchView>,
    pre_sketch_component: Option<crate::PreSketchComponent>,
    pre_edit_component: Option<crate::PreEditComponent>,
    view: DocView,
    occurrence_move: Option<crate::browser::OccMove>,
    reveals: crate::browser::PendingReveals,
    agent_chat: crate::agent_chat::AgentChat,
    /// Its linked design files as the design last read them (`Documents::link_stamps`).
    link_stamps: Vec<(u64, crate::linked_source::Stamp)>,
    edit_in_place: Option<crate::edit_in_place::PartContext>,
    /// The dialog or tool it had open and its toolbar tab, kept while another design is shown.
    work: DocWork,
    /// What it has computing off the window thread or waiting its turn.
    jobs: crate::deferred::DocJobs,
    /// Which tab it is ([`next_tab`]): the same for as long as the tab is open.
    pub(crate) tab: u64,
    /// Opened behind the shown design: framed when it is first shown.
    fit: bool,
}

/// What a design was doing when its tab was left, with the "Keep dialogs and tools open when
/// switching designs" preference (`keep_work_on_tab_switch`, #9360): the open dialog with its
/// picks and values, the tool, and the toolbar tab. They act on that design's session, which
/// waits in the same slot, so showing the tab again picks up exactly where it was left.
#[derive(Default)]
struct DocWork {
    /// Shared panel content belongs to the design, independently of its current placement.
    bom: Option<crate::bom_panel::BomPanel>,
    dialog: Option<crate::dialogs::Dialog>,
    editing_state: Option<crate::dialogs::EditingState>,
    tool: Option<crate::tools::Tool>,
    /// The toolbar tab shown, its contextual tab and the tab a sketch returns to (`None`: the
    /// design was left without the preference, and comes back on MODEL as before).
    tab: Option<(String, Option<String>, Option<String>)>,
}

/// Take the active design's dialog, tool and toolbar tab, to keep with its tab. A feature still
/// computing after OK can't wait (its result belongs to the window thread): that dialog is
/// cancelled, as without the preference.
fn take_work(app: &mut SolveApp) -> DocWork {
    let bom = app.bom.take();
    if !app.preferences.keep_work_on_tab_switch || app.applying.is_some() {
        crate::dialogs::cancel(app);
        app.tool = None;
        return DocWork { bom, ..DocWork::default() };
    }
    DocWork {
        bom,
        dialog: app.dialog.take(),
        editing_state: app.editing_state.take(),
        tool: app.tool.take(),
        tab: Some((app.ui.tab.clone(), app.ui.ribbon_context.clone(), app.ui.sketch_return_tab.clone())),
    }
}

/// Give the incoming design back what [`take_work`] kept of it, after its session is in the app.
/// Kept work comes back even when the preference was turned off since: the dialog may hold the
/// design's history rolled back for an edit, and only the dialog's Cancel or OK puts it back.
/// `before`: the session's revision before the switch moved it on to refresh the views.
fn put_work(app: &mut SolveApp, mut work: DocWork, before: u64) {
    app.bom = work.bom;
    crate::bom_panel::set_focus_pending(app, true);
    // That refresh is not a change to the design: the edit's Cancel must still see the design
    // it left (`Session::restore_editor` compares revisions), as `PendingFeatureEdit` does.
    if let Some(state) = &mut work.editing_state {
        state.source.revision = state.source.revision.saturating_add(app.session.revision.saturating_sub(before));
    }
    app.dialog = work.dialog;
    app.editing_state = work.editing_state;
    app.tool = work.tool;
    match work.tab {
        Some((tab, context, back)) => {
            app.ui.tab = tab;
            app.ui.ribbon_context = context;
            app.ui.sketch_return_tab = back;
        }
        None => {
            app.ui.ribbon_context = None;
            app.ui.tab = "MODEL".into();
        }
    }
    crate::workspace::sync_context(app);
}

/// The per-design part of the UI state the session doesn't keep (what is hidden, shown or locked
/// is in the session's `visibility`, saved with the design).
#[derive(Clone, Debug, Default)]
struct DocView {
    render: crate::render_workspace::ViewState,
    /// The runs and folders opened and folded in its History, and which part of the design it
    /// shows.
    history_folds: crate::history_folds::Folds,
    timeline_scope: crate::timeline_scope::Scope,
    timeline_followed: Option<Option<u64>>,
    /// Bodies, components and features copied or cut in this design: their names and paths mean nothing
    /// in another one (copied features stay with the session the same way). Another design
    /// pastes them from `SolveApp::foreign_copy`.
    copied_bodies: Vec<String>,
    copied_placements: Vec<Vec<u64>>,
    cut: crate::edit_clipboard::CutSet,
    /// `SolveApp::own_clipboard_stamp`.
    own_clipboard_stamp: u64,
}

impl DocSlot {
    fn new(session: Session) -> DocSlot {
        DocSlot {
            session,
            autosave: None,
            big_operation: false,
            cam: Camera::default(),
            pending_feature_edit: None,
            pre_sketch_cam: None,
            pre_sketch_component: None,
            pre_edit_component: None,
            view: DocView::default(),
            occurrence_move: None,
            reveals: Default::default(),
            agent_chat: Default::default(),
            link_stamps: Vec::new(),
            edit_in_place: None,
            work: DocWork::default(),
            jobs: Default::default(),
            tab: next_tab(),
            fit: false,
        }
    }
}

/// A new tab's number ([`DocSlot::tab`], [`Documents::active_tab`]).
pub(crate) fn next_tab() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// The open designs: one slot per tab; the active tab's slot is empty (its design is in the app).
#[derive(Default)]
pub struct Documents {
    slots: Vec<Option<DocSlot>>,
    pub active: usize,
    /// A tab waiting for the answer to "save changes?".
    pub closing: Option<usize>,
    /// Quitting: the tabs answered so far with Don't Save (`None`: not quitting).
    pub quitting: Option<Vec<usize>>,
    /// The "save changes?" button the arrow keys have picked; Enter gives it.
    pub choice: Answer,
    /// A grouped prompt and its save queue; row actions update the remaining tab indices.
    group: Option<SaveGroup>,
    /// Save As is answering a Close/Quit prompt.
    saving: bool,
    /// Close Other Tabs / Close All Tabs: the tabs still to close, in increasing order (the last
    /// is closed first, so the ones before it keep their places). One with unsaved changes asks
    /// first; Cancel stops the rest (#3055).
    pub batch: Option<Vec<usize>>,
    /// The shown design's linked design files as it last read them: their modification time and
    /// size when Update Links last read each without an error, or when the design first showed
    /// the link (opened, adopted in place, the link just made).
    /// Coming back to its tab reads again those that differ (`linked_source::update_changed`).
    pub(crate) link_stamps: Vec<(u64, crate::linked_source::Stamp)>,
    /// The shown tab's number ([`DocSlot::tab`]); 0 until it is first asked for ([`active_tab`]).
    active_tab: u64,
}

struct SaveGroup {
    rows: Vec<(usize, bool)>,
    pending: Vec<usize>,
    /// A row save restores the shown tab and prompt on cancellation or failure.
    saving_row: Option<(usize, usize)>,
}

/// Start one confirmation before any tab (including clean tabs) is removed.
fn start_group(app: &mut SolveApp, tabs: &[usize]) -> bool {
    if app.preferences.save_prompt_one_at_a_time {
        return false;
    }
    let rows: Vec<_> = tabs.iter().copied().filter(|i| *i < app.docs.count() && dirty(app, *i)).map(|i| (i, true)).collect();
    if rows.len() < 2 {
        return false;
    }
    app.docs.closing = rows.first().map(|(i, _)| *i);
    app.docs.choice = Answer::Save;
    app.docs.group = Some(SaveGroup { rows, pending: Vec::new(), saving_row: None });
    true
}

/// Finish only after all selected writes succeed. Unselected designs are discarded together.
fn finish_group(app: &mut SolveApp) {
    let Some(group) = app.docs.group.take() else { return };
    app.docs.closing = None;
    if let Some(skip) = app.docs.quitting.as_mut() {
        skip.extend(group.rows.into_iter().map(|(i, _)| i));
        quit_step(app);
    } else if let Some(tabs) = app.docs.batch.clone() {
        for i in tabs.into_iter().rev() {
            close(app, i, true);
        }
        app.docs.batch = None;
        crate::linked_source::update_changed(app);
    }
}

fn save_group_next(app: &mut SolveApp) {
    let next = app.docs.group.as_mut().and_then(|g| g.pending.pop());
    let Some(i) = next else {
        finish_group(app);
        return;
    };
    switch(app, i);
    app.docs.closing = Some(i);
    app.docs.saving = true;
    crate::toolbar::save(app);
    if !app.file_picker_busy() {
        saved_for_close(app);
    }
}

/// Answer one grouped row through the same save flow as Save Selected.
fn answer_row(app: &mut SolveApp, i: usize, save: bool) {
    if app.file_picker_busy() || app.docs.saving || !app.docs.group.as_ref().is_some_and(|g| g.rows.iter().any(|(row, _)| *row == i)) {
        return;
    }
    if !save {
        close_group_row(app, i);
        return;
    }
    let previous = (app.docs.active, app.docs.closing.unwrap_or(i));
    if let Some(group) = app.docs.group.as_mut() {
        group.saving_row = Some(previous);
    }
    switch(app, i);
    app.docs.closing = Some(i);
    app.docs.saving = true;
    crate::toolbar::save(app);
    if !app.file_picker_busy() {
        saved_for_close(app);
    }
}

/// Remove an answered tab and adjust every remaining target after the tab strip shifts.
fn close_group_row(app: &mut SolveApp, i: usize) {
    let Some(mut group) = app.docs.group.take() else { return };
    let mut batch = app.docs.batch.clone();
    let mut quitting = app.docs.quitting.clone();
    close(app, i, true);
    group.rows.retain(|(row, _)| *row != i);
    for (row, _) in &mut group.rows {
        if *row > i {
            *row -= 1;
        }
    }
    for indices in [&mut batch, &mut quitting].into_iter().flatten() {
        indices.retain(|row| *row != i);
        for row in indices {
            if *row > i {
                *row -= 1;
            }
        }
    }
    app.docs.batch = batch;
    app.docs.quitting = quitting;
    app.docs.closing = group.rows.first().map(|(row, _)| *row);
    let done = group.rows.is_empty();
    app.docs.group = Some(group);
    if done {
        finish_group(app);
    }
}

impl Documents {
    pub(crate) fn background_mut(&mut self) -> impl Iterator<Item = &mut DocSlot> {
        self.slots.iter_mut().filter_map(Option::as_mut)
    }

    /// The background slot (a tab not shown) holding the design with `uid`.
    pub(crate) fn background_slot_mut(&mut self, uid: u64) -> Option<&mut DocSlot> {
        self.slots.iter_mut().flatten().find(|slot| slot.session.uid == uid)
    }

    /// The designs in the tabs not shown.
    pub(crate) fn background(&self) -> impl Iterator<Item = &Session> {
        self.slots.iter().filter_map(Option::as_ref).map(|slot| &slot.session)
    }

    /// The shown tab's number: a file operation for its design waits for it to be shown again.
    pub(crate) fn active_tab(&mut self) -> u64 {
        if self.active_tab == 0 {
            self.active_tab = next_tab();
        }
        self.active_tab
    }

    /// [`Self::active_tab`], without numbering a tab not asked for yet (0).
    pub(crate) fn current_tab(&self) -> u64 {
        self.active_tab
    }

    /// Whether the tab numbered `tab` is still open: the one shown, or one behind it. A file
    /// operation bound to a tab that is gone has nothing left to land in.
    pub(crate) fn tab_open(&self, tab: u64) -> bool {
        self.active_tab == tab || self.slots.iter().flatten().any(|slot| slot.tab == tab)
    }

    /// Number of open designs (at least the active one).
    pub fn count(&self) -> usize {
        self.slots.len().max(1)
    }
}

/// Route native render completion by the document-lifetime owner, never by the
/// selected tab or a mutable path/name. The engine capsule verifies its registry.
pub(crate) fn render_owner(app: &SolveApp, uid: u64) -> Option<&Session> {
    if app.session.uid == uid { Some(&app.session) } else { app.docs.background().find(|s| s.uid == uid) }
}
pub(crate) fn with_render_owner<T>(
    app: &mut SolveApp,
    uid: u64,
    apply: impl FnOnce(&mut Session, &mut crate::render_workspace::ViewState) -> T,
) -> Option<T> {
    if app.session.uid == uid {
        Some(apply(&mut app.session, &mut app.render_workspace.view))
    } else {
        let slot = app.docs.background_mut().find(|slot| slot.session.uid == uid)?;
        Some(apply(&mut slot.session, &mut slot.view.render))
    }
}

/// The host renamed a design file: every open tab of that path follows it, including the
/// borrowed design name the tab title shows (#11375).
pub fn follow_file_rename(app: &mut SolveApp, old_path: &str, new_path: &str) {
    app.session.follow_file_rename(old_path, new_path);
    for slot in app.docs.background_mut() {
        slot.session.follow_file_rename(old_path, new_path);
    }
}

fn take_view(app: &mut SolveApp) -> DocView {
    DocView {
        render: std::mem::take(&mut app.render_workspace.view),
        history_folds: std::mem::take(&mut app.tree.history_folds),
        timeline_scope: std::mem::take(&mut app.tree.timeline_scope),
        timeline_followed: app.tree.timeline_followed.take(),
        copied_bodies: std::mem::take(&mut app.copied_bodies),
        copied_placements: std::mem::take(&mut app.copied_placements),
        cut: std::mem::take(&mut app.cut),
        own_clipboard_stamp: app.own_clipboard_stamp,
    }
}

fn put_view(app: &mut SolveApp, v: DocView) {
    app.render_workspace.view = v.render;
    crate::render_workspace::invalidate_images(app);
    crate::render_workspace::invalidate_scene(app);
    crate::render_workspace::sync_backend(app);
    app.tree.history_folds = v.history_folds;
    app.tree.timeline_scope = v.timeline_scope;
    app.tree.timeline_followed = v.timeline_followed;
    app.copied_bodies = v.copied_bodies;
    app.copied_placements = v.copied_placements;
    app.cut = v.cut;
    app.own_clipboard_stamp = v.own_clipboard_stamp;
    crate::edit_clipboard::drop_stale(app);
}

/// Put `slot` into the app and return what was there.
fn swap_in(app: &mut SolveApp, mut slot: DocSlot) -> DocSlot {
    let reveals = crate::browser::take_reveals(app);
    crate::semantic_content::leave_document(app);
    if crate::render_workspace::active(app) {
        let _ = crate::render_workspace::enter(app, false);
    }
    // Whatever was going on in the design leaves with it: cancelled, or kept with its tab
    // (`take_work`).
    crate::timeline::stop_playback(app);
    crate::sketch_text_move::cancel_drag(app);
    let work = take_work(app);
    crate::dim_view::cancel();
    app.params = None;
    app.design_properties = None;
    app.cancel_view_animation();
    app.viewport.orbit_free = None;
    app.preview.reset();
    app.viewport.hover = None;
    crate::design_properties::close_object(app);
    crate::sketch_attachment_ui::clear(app);
    crate::agent_chat::leave_document(app);
    app.agent_chat.swap_document(&mut slot.agent_chat);
    let out_rev = app.session.revision;
    // A click's point means nothing in the other design.
    app.edit_anchor_hint = None;
    let mut out = DocSlot {
        session: std::mem::replace(&mut app.session, slot.session),
        autosave: app.autosave.as_mut().and_then(|a| std::mem::replace(&mut a.active, slot.autosave)),
        big_operation: std::mem::replace(&mut app.big_operation, slot.big_operation),
        cam: std::mem::replace(&mut app.cam, slot.cam),
        pending_feature_edit: std::mem::replace(&mut app.pending_feature_edit, slot.pending_feature_edit),
        pre_sketch_cam: std::mem::replace(&mut app.pre_sketch_cam, slot.pre_sketch_cam),
        pre_sketch_component: std::mem::replace(&mut app.pre_sketch_component, slot.pre_sketch_component),
        pre_edit_component: std::mem::replace(&mut app.pre_edit_component, slot.pre_edit_component),
        view: take_view(app),
        occurrence_move: std::mem::replace(&mut app.tree.occurrence_move, slot.occurrence_move),
        reveals,
        agent_chat: slot.agent_chat,
        link_stamps: std::mem::replace(&mut app.docs.link_stamps, slot.link_stamps),
        edit_in_place: std::mem::replace(&mut app.edit_in_place.active, slot.edit_in_place),
        work,
        jobs: app.swap_jobs(slot.jobs),
        tab: std::mem::replace(&mut app.docs.active_tab, slot.tab),
        fit: false,
    };
    crate::browser::restore_reveals(app, slot.reveals);
    // Only the displayed design owns the process-wide crash replay journal.
    app.session.crash_journal = out.session.crash_journal;
    out.session.crash_journal = false;
    put_view(app, slot.view);
    // Components picked in the browser are the outgoing design's: Copy must not copy them here.
    app.tree.picked_components.clear();
    // Preferences the session holds go with the user, not the tab (#680).
    app.session.auto_project = out.session.auto_project;
    app.session.project_construction = out.session.project_construction;
    app.session.set_validation(out.session.validation());
    // So does the host's pre-command check: every tab is held to it.
    app.session.set_command_check(out.session.command_check());
    app.session.set_single_surface_analysis(app.preferences.single_surface_analysis);
    crate::print_face_ui::sync(&mut app.session, &app.preferences);
    // Views cache by revision: the incoming design must not look unchanged.
    let before = app.session.revision;
    app.session.invalidate_view(out_rev);
    if let Some(pending) = &mut app.pending_feature_edit {
        pending.view_changed(before, app.session.revision);
    }
    app.jobs_view_changed(before, app.session.revision);
    crate::browser::forget_found(app);
    put_work(app, slot.work, before);
    out
}

/// Move tab `from` to place `to`, the tabs between shifting over (#3187); each design keeps its
/// state and the active one stays active. Not while a "save changes?" prompt is open or tabs are
/// being closed one after another (their tab numbers would change under them).
pub fn move_tab(app: &mut SolveApp, from: usize, to: usize) {
    let n = app.docs.slots.len();
    if from >= n || to >= n || from == to || app.docs.closing.is_some() || app.docs.quitting.is_some() || app.docs.batch.is_some() {
        return;
    }
    let slot = app.docs.slots.remove(from);
    app.docs.slots.insert(to, slot);
    app.docs.active = moved_index(app.docs.active, from, to);
}

/// Where the tab at `i` ends up when the tab at `from` moves to `to`.
pub fn moved_index(i: usize, from: usize, to: usize) -> usize {
    if i == from {
        to
    } else if from < i && i <= to {
        i - 1
    } else if to <= i && i < from {
        i + 1
    } else {
        i
    }
}

/// Make tab `i` the active design.
pub fn switch(app: &mut SolveApp, i: usize) {
    if app.file_picker_busy() {
        return;
    }
    app.home.open = false;
    if i == app.docs.active || i >= app.docs.slots.len() {
        return;
    }
    let Some(incoming) = app.docs.slots.get_mut(i).and_then(Option::take) else {
        return;
    };
    crate::linked_source::note_unread(app);
    let fit = incoming.fit;
    let out = swap_in(app, incoming);
    if fit {
        app.fit_view();
    }
    let a = app.docs.active;
    if let Some(s) = app.docs.slots.get_mut(a) {
        *s = Some(out);
    }
    app.docs.active = i;
    // Closing several tabs, quitting, or a "save changes?" for this tab goes through them, and a
    // save then must not wait for a read: what they missed waits for their return.
    // An assembly reading its links for a part edited in place takes them all up once it is done.
    if app.docs.batch.is_none()
        && app.docs.quitting.is_none()
        && app.docs.closing.is_none()
        && crate::edit_in_place::refreshing_assembly(app).is_none()
    {
        crate::linked_source::update_changed(app);
    }
    crate::edit_in_place::attach(app);
}

/// Show the next design tab (`forward`) or the previous one, going round from the last to the
/// first (Ctrl+Tab and Ctrl+Shift+Tab, #8565). Not while tabs are being closed one after another
/// or a "save changes?" prompt is open, as for `move_tab`.
pub fn cycle(app: &mut SolveApp, forward: bool) {
    let n = app.docs.slots.len();
    if n < 2 || app.docs.closing.is_some() || app.docs.quitting.is_some() || app.docs.batch.is_some() {
        return;
    }
    let a = app.docs.active;
    switch(app, if forward { (a + 1) % n } else { (a + n - 1) % n });
}

/// The open design saved at `path`, shown or in the background.
pub(crate) fn session_at<'a>(app: &'a SolveApp, path: &str) -> Option<&'a Session> {
    let saved_at = |s: &Session| s.path.as_deref().is_some_and(|p| same_path(p, path));
    if saved_at(&app.session) {
        return Some(&app.session);
    }
    app.docs.slots.iter().flatten().map(|s| &s.session).find(|s| saved_at(s))
}

/// Every edit in place: of the tab shown, and of the tabs in the background.
pub(crate) fn edits_mut(app: &mut SolveApp) -> Vec<&mut crate::edit_in_place::PartContext> {
    let mut edits: Vec<_> = app.docs.slots.iter_mut().flatten().filter_map(|s| s.edit_in_place.as_mut()).collect();
    edits.extend(app.edit_in_place.active.as_mut());
    edits
}

/// The open design saved at `path` read every link again without an error, from the files as
/// `stamps` found them: its record of them (`Documents::link_stamps`, or its tab's) is now that.
pub(crate) fn record_stamps_at(app: &mut SolveApp, path: &str, stamps: &[(u64, crate::linked_source::Stamp)]) {
    let record = if app.session.path.as_deref().is_some_and(|p| same_path(p, path)) {
        &mut app.docs.link_stamps
    } else {
        let saved_at = |s: &DocSlot| s.session.path.as_deref().is_some_and(|p| same_path(p, path));
        let Some(slot) = app.docs.slots.iter_mut().flatten().find(|s| saved_at(s)) else {
            return;
        };
        &mut slot.link_stamps
    };
    for (c, s) in stamps {
        crate::linked_source::record(record, *c, *s);
    }
}

/// [`session_at`], to change it.
pub(crate) fn session_at_mut<'a>(app: &'a mut SolveApp, path: &str) -> Option<&'a mut Session> {
    let saved_at = |s: &Session| s.path.as_deref().is_some_and(|p| same_path(p, path));
    if saved_at(&app.session) {
        return Some(&mut app.session);
    }
    app.docs.slots.iter_mut().flatten().map(|s| &mut s.session).find(|s| saved_at(s))
}

/// Select an already-open saved design without reloading or framing it. Identity is derived
/// from the current session path so Save As immediately changes which file the tab owns.
pub(crate) fn reuse_path(app: &mut SolveApp, path: &str) -> Result<bool, String> {
    solidcraft_engine::io::vfs::check(path).map_err(|e| e.to_string())?;
    // Imports create unsaved designs, rather than owning their source file.
    if solidcraft_engine::io::is_step_path(path) || solidcraft_engine::io::is_mesh_path(path) {
        return Ok(false);
    }
    let matches = |s: &Session| s.path.as_deref().is_some_and(|p| same_path(p, path));
    let index = if matches(&app.session) {
        Some(app.docs.active)
    } else {
        app.docs.slots.iter().position(|s| s.as_ref().is_some_and(|s| matches(&s.session)))
    };
    let Some(index) = index else { return Ok(false) };
    if app.agent_chat.following_document && index != app.docs.active {
        return Err("the design is already open in another tab; select that tab to continue in its own conversation".into());
    }
    if app.file_picker_busy() {
        return Err("finish choosing a file before switching designs".into());
    }
    switch(app, index);
    Ok(true)
}

pub(crate) fn same_path(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // canonicalize resolves symlinks and Windows long names/case using the filesystem.
        // Never lowercase: Windows can also host case-sensitive directories.
        if let (Ok(a), Ok(b)) = (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
            return a == b;
        }
        // A deleted file can still select its live editing session. Path equality also
        // accepts redundant separators and `.` without guessing how symlinks resolve `..`.
        if let (Ok(a), Ok(b)) = (std::path::absolute(a), std::path::absolute(b)) {
            return a == b;
        }
    }
    // Browser VFS keys are exact strings, independent of the host's path conventions.
    false
}

/// Open `session` in a new tab after the active one, leaving the shown design as it is (a file
/// opened while the person is busy in another design). Its view is framed when it is shown.
pub(crate) fn open_tab_behind(app: &mut SolveApp, session: Session) -> Result<(), String> {
    if app.docs.slots.is_empty() {
        app.docs.slots.push(None);
        app.docs.active = 0;
    }
    if app.docs.slots.len() >= MAX_OPEN {
        return Err(format!("at most {MAX_OPEN} designs can be open"));
    }
    let mut slot = DocSlot::new(session);
    slot.fit = true;
    app.docs.slots.insert(app.docs.active + 1, Some(slot));
    Ok(())
}

/// Open `session` in a new tab after the active one and switch to it (`fit`: frame its model).
pub fn open_tab(app: &mut SolveApp, session: Session) -> Result<(), String> {
    if app.docs.group.is_some() {
        return Err(app.tr("document.finish_save_prompt").to_owned());
    }
    if app.docs.slots.is_empty() {
        app.docs.slots.push(None);
        app.docs.active = 0;
    }
    if app.docs.slots.len() >= MAX_OPEN {
        return Err(format!("at most {MAX_OPEN} designs can be open"));
    }
    let at = app.docs.active + 1;
    app.docs.slots.insert(at, Some(DocSlot::new(session)));
    crate::agent_chat::follow_new_tab(app, |app| switch(app, at));
    crate::history_folds::load(app);
    app.fit_view();
    Ok(())
}

/// Is the active design an untouched new one (it can be replaced instead of opening a tab)?
pub fn active_is_blank(app: &SolveApp) -> bool {
    let s = &app.session;
    s.path.is_none() && s.doc.features.is_empty() && s.doc.params.is_empty() && s.doc.components.is_empty() && !s.is_dirty()
}

/// Show `session` (a design built or read elsewhere): in place of a blank active design,
/// otherwise in a new tab. Whether it is shown (no tab left for it: the status says so).
pub fn adopt(app: &mut SolveApp, session: Session) -> bool {
    if active_is_blank(app) {
        let rev = app.session.revision;
        crate::browser::discard_reveals(app);
        crate::semantic_content::leave_document(app);
        crate::dialogs::cancel(app);
        app.tool = None;
        app.bom = None;
        crate::design_properties::close_object(app);
        crate::sketch_attachment_ui::clear(app);
        app.session.replace_design(session);
        reset_chat(app);
        // Another design in the tab: what waits for the blank one's tab no longer does.
        app.docs.active_tab = next_tab();
        // The blank design's record of its links goes with it; the new one's start now.
        app.docs.link_stamps.clear();
        crate::history_folds::load(app);
        crate::timeline_scope::reset(app);
        crate::render_workspace::reset_document(app);
        app.session.invalidate_view(rev);
        app.cam = Camera::default();
        app.fit_view();
        // The linked designs that changed while it was closed are read again (#9739): last, as a
        // read is taken up only by the design revision it started from.
        crate::linked_source::update_changed(app);
        true
    } else if let Err(e) = open_tab(app, session) {
        app.set_status(e, true);
        false
    } else {
        true
    }
}

/// The active design as its file holds it (`document.export`): what a host that keeps the files
/// itself writes (#8517). `text` is the design's JSON, the same a `.json` design file holds, and
/// `revision` is the one `document.revision` names this state by.
pub fn export(app: &mut SolveApp) -> Result<Value, String> {
    let revision = app.session.design_revision();
    let design = app.session.design();
    let text = String::from_utf8(solidcraft_engine::io::write_design(&design)).map_err(|e| e.to_string())?;
    Ok(json!({"name": design.name, "revision": revision, "dirty": app.session.is_dirty(), "format": "json", "text": text}))
}

/// Replace the active design with one a host kept (`document.import {text, name?}`): the text of
/// a design file (its JSON, as `document.export` gives it). It opens saved, with nothing to undo,
/// in the active design's place: as a design opened in a tab of its own, with that tab closed
/// without asking. A text that does not read changes nothing.
pub fn import(app: &mut SolveApp, p: &Value) -> Result<Value, String> {
    if app.file_picker_busy() || app.file_load.busy() || app.docs.closing.is_some() || app.docs.quitting.is_some() || app.docs.batch.is_some() {
        return Err("a file is being opened or saved, or designs are being closed: try again".into());
    }
    let text = p.get("text").and_then(Value::as_str).ok_or("missing `text` (the design's JSON)")?;
    let mut doc = solidcraft_engine::io::read_design(text.as_bytes()).map_err(|e| e.to_string())?;
    if let Some(name) = p.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()) {
        doc.name = name.to_string();
    }
    // Like a design opened from a file, it comes in without its print faces unless they are kept
    // (#11512); dropped before the session starts, so it still opens saved with nothing to undo.
    if !app.preferences.keep_print_faces {
        doc.print_faces.clear();
    }
    let replaced = app.docs.active;
    open_tab(app, Session::opened(doc, None))?;
    close(app, replaced, true);
    let errors = app.session.model.results.iter().filter(|r| r.error.is_some()).count();
    let revision = app.session.design_revision();
    Ok(json!({"name": app.session.doc.name, "features": app.session.doc.features.len(), "errors": errors, "revision": revision}))
}

/// The host saved the design at `revision` (`document.saved`): it counts as saved from there.
/// Refused when the design has changed since.
pub fn saved(app: &mut SolveApp, p: &Value) -> Result<Value, String> {
    let revision = p.get("revision").and_then(Value::as_u64).ok_or("missing `revision`")?;
    let now = app.session.design_revision();
    if revision != now {
        return Err(format!("the design changed since revision {revision} (now {now})"));
    }
    app.session.mark_saved();
    Ok(json!({"revision": revision, "dirty": app.session.is_dirty()}))
}

/// A new, empty design in its own tab. One that is already empty keeps its tab and still
/// takes Units of new designs (#1205): a launch starts on that empty design, so returning
/// here used to leave it in millimetres.
pub fn new_design(app: &mut SolveApp) {
    if app.file_picker_busy() {
        return;
    }
    app.home.open = false;
    if active_is_blank(app) {
        crate::prefs::apply_new_design(app);
        return;
    }
    if let Err(e) = open_tab(app, Session::default()) {
        app.set_status(e, true);
    } else {
        crate::prefs::apply_new_design(app);
    }
}

/// Create a new tab directly; the File view chooses units before using [`new_design`].
/// An untouched blank launch design is reused.
pub fn new_tab(app: &mut SolveApp) {
    if app.file_picker_busy() {
        return;
    }
    if app.home.open && active_is_blank(app) {
        new_design(app);
        return;
    }
    app.home.open = false;
    if let Err(e) = open_tab(app, Session::default()) {
        app.set_status(e, true);
    } else {
        crate::prefs::apply_new_design(app);
    }
}

/// Before opening a file: a fresh tab unless the active design is blank. The empty design,
/// new or already showing, takes Units of new designs so a cancelled open is not millimetres
/// when the preference says otherwise.
pub fn prepare_open(app: &mut SolveApp) {
    if active_is_blank(app) {
        crate::prefs::apply_new_design(app);
        return;
    }
    if let Err(e) = open_tab(app, Session::default()) {
        app.set_status(e, true);
    } else {
        crate::prefs::apply_new_design(app);
    }
}

/// Close tab `i`: asks first when it has unsaved changes (`force`: close anyway).
pub fn close(app: &mut SolveApp, i: usize, force: bool) {
    if app.docs.group.is_some() || app.file_picker_busy() {
        return;
    }
    let n = app.docs.count();
    if i >= n {
        return;
    }
    if !force && dirty(app, i) {
        app.docs.closing = Some(i);
        return;
    }
    app.docs.closing = None;
    let closing_uid = if i == app.docs.active { Some(app.session.uid) } else { app.docs.slots[i].as_ref().map(|slot| slot.session.uid) };
    if let Some(uid) = closing_uid {
        crate::render_workspace::prepare::document_closing(app, uid);
    }
    if n <= 1 {
        crate::browser::discard_reveals(app);
        crate::semantic_content::leave_document(app);
        // The last design: a blank one takes its place.
        reset_chat(app);
        if let Some(a) = app.autosave.as_mut().and_then(|a| a.active.take()) {
            a.discard();
        }
        // What it had computing goes with it: the blank design waits for none of it.
        app.cancel_computing();
        app.big_operation = false;
        crate::sketch_text_move::cancel_drag(app);
        let rev = app.session.revision;
        crate::dialogs::cancel(app);
        app.tool = None;
        app.bom = None;
        app.session.replace_design(Session::default());
        app.session.revision = rev + 1;
        crate::prefs::apply_new_design(app);
        app.cam = Camera::default();
        app.fit_view();
        put_view(app, DocView::default());
        app.docs = Documents::default();
        app.edit_in_place = Default::default();
        app.home.open = true;
        return;
    }
    if i == app.docs.active {
        // Show a neighbour first, then drop this one.
        let next = if i + 1 < n { i + 1 } else { i - 1 };
        switch(app, next);
    }
    if let Some(slot) = app.docs.slots.remove(i) {
        slot.session.stop_computing();
        if let Some(a) = slot.autosave {
            a.discard();
        }
    }
    if app.docs.active > i {
        app.docs.active -= 1;
    }
}

/// A replacement starts a fresh conversation, unless the current chat itself requested it.
pub(crate) fn reset_chat(app: &mut SolveApp) {
    if app.agent_chat.following_document {
        crate::agent_chat::continue_in_document(app);
        return;
    }
    // A chat not asked anything yet (an agent connected at start, say) holds nothing of the old
    // design: it starts over in place, and its agent stays connected.
    if !app.agent_chat.items.iter().any(|i| matches!(i, crate::agent_chat::Item::User { .. })) {
        crate::agent_chat::new_chat(app);
        app.agent_chat.input.clear();
        return;
    }
    crate::agent_chat::leave_document(app);
    app.agent_chat.swap_document(&mut crate::agent_chat::AgentChat::default());
}

/// Hidden conversations must also stop when AI is disabled or the application quits.
pub(crate) fn disconnect_background_chats(app: &mut SolveApp, wait: bool) {
    for slot in app.docs.background_mut() {
        slot.agent_chat.disconnect_document(&mut slot.session, wait);
    }
}

/// Stop only the hidden conversations whose endpoint permission was revoked.
pub(crate) fn disconnect_background_endpoint(app: &mut SolveApp, id: &str) {
    for slot in app.docs.background_mut() {
        slot.agent_chat.disconnect_endpoint(&mut slot.session, id);
    }
}

/// Close several tabs: group unsaved designs before closing anything, unless the preference
/// requests the original one-at-a-time prompts (#3055).
pub fn close_many(app: &mut SolveApp, mut tabs: Vec<usize>) {
    if app.file_picker_busy() || app.docs.quitting.is_some() || app.docs.closing.is_some() {
        return;
    }
    tabs.sort_unstable();
    tabs.dedup();
    tabs.retain(|i| *i < app.docs.count());
    app.docs.batch = Some(tabs.clone());
    if !start_group(app, &tabs) {
        batch_step(app);
    }
}

/// Close the next tab of [`close_many`]'s batch, or ask about it when it has unsaved changes.
fn batch_step(app: &mut SolveApp) {
    if app.docs.closing.is_some() || app.docs.quitting.is_some() {
        return;
    }
    if app.docs.batch.is_none() {
        return;
    }
    // The tabs left stay in `batch` while each closes, so the tabs shown on the way know it.
    while let Some(i) = app.docs.batch.as_mut().and_then(Vec::pop) {
        if i >= app.docs.count() {
            continue;
        }
        if dirty(app, i) {
            app.docs.closing = Some(i);
            return;
        }
        close(app, i, true);
    }
    app.docs.batch = None;
    // The tab left in front reads now what it skipped on the way.
    crate::linked_source::update_changed(app);
}

/// Quit this window: confirm its unsaved designs, then set `app.quit_requested` for the host.
/// Cancel, or a Save that didn't save, stays open.
pub fn request_quit(app: &mut SolveApp) {
    if app.file_picker_busy() {
        return;
    }
    if app.docs.quitting.is_none() {
        // Quit replaces a pending tab-close group with this window's complete set. The
        // legacy preference keeps an existing individual question before moving on.
        if app.docs.group.take().is_some() || !app.preferences.save_prompt_one_at_a_time {
            app.docs.closing = None;
            app.docs.choice = Answer::Save;
        }
        app.docs.quitting = Some(Vec::new());
        app.docs.batch = None;
        if app.docs.closing.is_none() && start_group(app, &(0..app.docs.count()).collect::<Vec<_>>()) {
            return;
        }
    }
    quit_step(app);
}

/// Ask about the next design with unsaved changes, or quit when none is left.
fn quit_step(app: &mut SolveApp) {
    let Some(skip) = app.docs.quitting.clone() else {
        return;
    };
    if app.docs.closing.is_some() {
        return;
    }
    match (0..app.docs.count()).find(|i| !skip.contains(i) && dirty(app, *i)) {
        Some(i) => {
            switch(app, i);
            app.docs.closing = Some(i);
        }
        None => {
            // Don't Save means discard: no recovery copy is kept for the next launch either.
            for i in skip {
                switch(app, i);
                app.session.mark_saved();
            }
            app.docs.quitting = None;
            app.quit_requested = true;
            // The next launch reopens the designs saved to a file (#5542).
            let sessions: Vec<&solidcraft_engine::Session> = match app.docs.slots.is_empty() {
                true => vec![&app.session],
                false => app.docs.slots.iter().map(|s| s.as_ref().map_or(&app.session, |d| &d.session)).collect(),
            };
            app.preferences.open_designs = sessions.into_iter().filter_map(|s| s.path.clone()).collect();
        }
    }
}

/// Is any open design unsaved (the web page asks before it is left)?
pub fn any_dirty(app: &SolveApp) -> bool {
    (0..app.docs.count()).any(|i| dirty(app, i))
}

/// Every open design, in tab order (the shown one in its place): what the web app keeps a copy
/// of in the browser when "keep unsaved work in this browser" is on (#10220).
pub fn open_sessions(app: &SolveApp) -> Vec<&Session> {
    if app.docs.slots.is_empty() {
        return vec![&app.session];
    }
    app.docs.slots.iter().map(|s| s.as_ref().map_or(&app.session, |d| &d.session)).collect()
}

fn dirty(app: &SolveApp, i: usize) -> bool {
    if i == app.docs.active || app.docs.slots.is_empty() {
        return app.session.is_dirty();
    }
    app.docs.slots.get(i).and_then(Option::as_ref).is_some_and(|s| s.session.is_dirty())
}

/// A tab's name: the file a saved design was opened or saved as (without its extension), so a
/// Save As shows the new name; an unsaved design, or one renamed since it was saved, shows its
/// design name. The Browser's top row shows the same name (#7267).
///
/// With the `names_from_file` preference off, every tab shows the design's own name, as before.
pub(crate) fn tab_name(app: &SolveApp, session: &Session) -> String {
    if app.preferences.names_from_file { session.save_as_name() } else { session.doc.name.clone() }
}

/// Tab names (with an unsaved-changes flag), in order.
pub fn tabs_info(app: &SolveApp) -> Vec<(String, bool)> {
    if app.docs.slots.is_empty() {
        return vec![(tab_name(app, &app.session), app.session.is_dirty())];
    }
    app.docs
        .slots
        .iter()
        .map(|s| match s {
            Some(d) => (tab_name(app, &d.session), d.session.is_dirty()),
            None => (tab_name(app, &app.session), app.session.is_dirty()),
        })
        .collect()
}

/// Cached title inputs, separate from document identity: equal visible titles need no update.
#[derive(Default)]
pub(crate) struct WindowTitle {
    initialized: bool,
    design: Option<(String, String, bool)>,
}

impl WindowTitle {
    fn update(&mut self, name: Option<&str>, dirty: bool, language: &str) -> Option<String> {
        let unchanged = match (&self.design, name) {
            (None, None) => true,
            (Some((old_name, old_language, old_dirty)), Some(name)) => old_name == name && old_language == language && *old_dirty == dirty,
            _ => false,
        };
        if self.initialized && unchanged {
            return None;
        }
        self.initialized = true;
        self.design = name.map(|name| (name.to_owned(), language.to_owned(), dirty));
        Some(match name {
            Some(name) => {
                let name = solidcraft_ui_text::Locale::for_language(language).given_name(name);
                format!("{name}{} — SolidCraft", if dirty { " *" } else { "" })
            }
            None => "SolidCraft".into(),
        })
    }
}

/// Derive from the live session after UI actions, including tab switches and asynchronous saves.
/// Embedded applications leave the host's page/window chrome entirely alone.
pub(crate) fn sync_window_title(app: &mut SolveApp, ctx: &egui::Context) {
    if app.embedded || app.embed_editor {
        return;
    }
    // The start page can overlay existing designs; only its untouched launch/last-close
    // session represents no open design. Documents::count includes that placeholder.
    let no_design = app.home.open && app.docs.count() == 1 && active_is_blank(app);
    let name = (!no_design).then_some(app.session.doc.name.as_str());
    if let Some(title) = app.window_title.update(name, app.session.is_dirty(), &app.preferences.language) {
        ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Title(title));
    }
}

/// A design open in another tab, as Insert Design offers it (#8570): its name, its file (`None`:
/// not saved yet, so there is nothing to link) and whether it has unsaved changes.
pub(crate) struct OpenDesign {
    pub name: String,
    pub path: Option<String>,
    pub unsaved: bool,
}

/// The designs open in the other tabs, in tab order: none on the shown design's file, and no
/// blank one (an empty new design has nothing to insert).
pub(crate) fn other_designs(app: &SolveApp) -> Vec<OpenDesign> {
    let shown = app.session.path.as_deref();
    app.docs
        .background()
        .filter(|s| s.path.is_some() || !s.doc.features.is_empty())
        .filter(|s| !s.path.as_deref().zip(shown).is_some_and(|(a, b)| same_path(a, b)))
        .map(|s| OpenDesign { name: s.doc.name.clone(), path: s.path.clone(), unsaved: s.is_dirty() })
        .collect()
}

/// Where each tab's design is saved, in `tabs_info`'s order (`None`: not saved yet).
fn tab_paths(app: &SolveApp) -> Vec<Option<String>> {
    if app.docs.slots.is_empty() {
        return vec![app.session.path.clone()];
    }
    app.docs.slots.iter().map(|s| s.as_ref().map_or(&app.session, |d| &d.session).path.clone()).collect()
}

/// Where a tab's design is saved (home shortened to `~`), or that it isn't yet; below, that a
/// saved design has changes not saved yet (`dirty`), which the bullet after a cut name no longer
/// shows (#11408), and what its signature says. The tab puts its full name above when it cuts it
/// short (#5868).
fn tab_place(path: Option<&str>, dirty: bool, security: Option<&str>) -> String {
    let mut place = path.map_or_else(|| solidcraft_ui_text::tr_label("Not saved yet").to_string(), crate::home::friendly_path);
    if dirty && path.is_some() {
        place.push('\n');
        place.push_str(solidcraft_ui_text::tr("document.tab_unsaved_changes"));
    }
    if let Some(security) = security {
        place.push('\n');
        place.push_str(security);
    }
    place
}

/// What a design's signature says, for its tab's tooltip: the last verification when one ran, or
/// that the file carries records that have not been checked in this session.
fn tab_security(app: &SolveApp) -> Vec<Option<String>> {
    open_sessions(app)
        .into_iter()
        .map(|session| {
            if let Some(report) = &session.provenance {
                if session.is_dirty() {
                    return Some(solidcraft_ui_text::tr("ui.open_signature_changed").to_string());
                }
                let trusted = report.newest().is_some_and(|record| signer_trusted(app, record));
                return Some(signature_line(report, trusted));
            }
            let signed = session.provenance_chain.as_ref().is_some_and(|chain| !chain.records.is_empty());
            signed.then(|| solidcraft_ui_text::tr("ui.open_signature_unverified").to_string())
        })
        .collect()
}

/// Whether a record's key is in the user's trusted-fingerprint list. The embedded user id is not
/// proof of anything; this is the app's one trust decision.
pub(crate) fn signer_trusted(app: &SolveApp, record: &solidcraft_engine::io::ProvenanceRecord) -> bool {
    trusted_fingerprint(&app.preferences.trusted_signing_keys, &record.signer.fingerprint())
}

/// Whether `fingerprint` is one of the comma/space separated fingerprints in `trusted`.
pub(crate) fn trusted_fingerprint(trusted: &str, fingerprint: &str) -> bool {
    let fingerprint = fingerprint.trim();
    if fingerprint.is_empty() {
        return false;
    }
    // Commas/newlines separate entries; spaces inside an entry are ignored, because
    // `gpg --fingerprint` prints a single fingerprint in groups of four.
    trusted
        .split([',', ';', '\n'])
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| entry.chars().filter(|c| !c.is_whitespace()).collect::<String>())
        .any(|entry| entry.eq_ignore_ascii_case(fingerprint))
}

/// The shield each design tab shows, from the same last check as its tooltip.
fn tab_badges(app: &SolveApp) -> Vec<Option<crate::app_bar::TabBadge>> {
    use crate::app_bar::TabBadge;
    open_sessions(app)
        .into_iter()
        .map(|session| {
            if let Some(report) = &session.provenance {
                if session.is_dirty() {
                    return Some(TabBadge::Signed);
                }
                return Some(match &report.status {
                    solidcraft_engine::io::ProvenanceStatus::Valid => {
                        if report.newest().is_some_and(|record| signer_trusted(app, record)) {
                            TabBadge::Verified
                        } else {
                            TabBadge::Signed
                        }
                    }
                    solidcraft_engine::io::ProvenanceStatus::NotCovered => TabBadge::NotCovered,
                    solidcraft_engine::io::ProvenanceStatus::Invalid(_) => TabBadge::Invalid,
                    solidcraft_engine::io::ProvenanceStatus::Unsupported(_) => TabBadge::Unsupported,
                });
            }
            let signed = session.provenance_chain.as_ref().is_some_and(|chain| !chain.records.is_empty());
            signed.then_some(TabBadge::Signed)
        })
        .collect()
}

/// A verification report as one line for a tab tooltip.
pub(crate) fn signature_line(report: &solidcraft_engine::io::ProvenanceReport, trusted: bool) -> String {
    match &report.status {
        solidcraft_engine::io::ProvenanceStatus::Valid => match (trusted, report.newest()) {
            (true, Some(record)) => {
                let key = signer_name(record);
                solidcraft_ui_text::format("ui.open_signature_valid", &[("key", &key)])
            }
            (_, Some(record)) => {
                let fingerprint = record.signer.fingerprint();
                solidcraft_ui_text::format("ui.open_signature_valid_untrusted", &[("key", &fingerprint)])
            }
            _ => solidcraft_ui_text::tr("ui.open_signature_unverified").to_string(),
        },
        solidcraft_engine::io::ProvenanceStatus::NotCovered => solidcraft_ui_text::tr("ui.open_signature_not_covered").to_string(),
        solidcraft_engine::io::ProvenanceStatus::Invalid(why) => solidcraft_ui_text::format("ui.open_signature_invalid", &[("reason", why)]),
        solidcraft_engine::io::ProvenanceStatus::Unsupported(_) => solidcraft_ui_text::tr("ui.open_signature_unsupported").to_string(),
    }
}

/// The name to show for the key that signed a record.
fn signer_name(record: &solidcraft_engine::io::ProvenanceRecord) -> String {
    match &record.signer {
        solidcraft_engine::io::ProvenanceSigner::Ed25519 { key_id, .. } => {
            use std::fmt::Write as _;
            let mut hex = String::with_capacity(16);
            for byte in &key_id[..8] {
                // Writing to a `String` cannot fail.
                let _ = write!(&mut hex, "{byte:02x}");
            }
            hex
        }
        solidcraft_engine::io::ProvenanceSigner::OpenPgp { fingerprint, user_id } => user_id.clone().unwrap_or_else(|| fingerprint.clone()),
    }
}

/// The designs a launch with no file to open reopens: those open at the last quit that still
/// exist, when Preferences › General asks for it (#5542).
pub fn designs_to_reopen(app: &SolveApp) -> Vec<String> {
    if !app.preferences.reopen_designs {
        return Vec::new();
    }
    app.preferences.open_designs.iter().filter(|p| std::path::Path::new(p.as_str()).is_file()).take(MAX_OPEN).cloned().collect()
}

/// What this launch reopens (#5542), with the crash guard applied (#5064): when the recovery
/// folder still holds the marker of a session that died mid-reopen, nothing is reopened this
/// time and the designs it died on come back as the reason, so the app starts usable instead
/// of hanging on the same design every launch. The marker is written here and cleared as each
/// design finishes opening (`file_load`), so a healthy session leaves nothing behind.
pub fn reopen_plan(app: &SolveApp, dir: Option<&std::path::Path>) -> (Vec<String>, Vec<String>) {
    let Some(dir) = dir else {
        return (designs_to_reopen(app), Vec::new());
    };
    let stuck = solidcraft_engine::recovery::stuck_reopen(dir);
    if !stuck.is_empty() {
        solidcraft_engine::recovery::clear_reopen(dir);
        return (Vec::new(), stuck);
    }
    let reopen = designs_to_reopen(app);
    solidcraft_engine::recovery::begin_reopen(dir, &reopen);
    (reopen, Vec::new())
}

/// The control channel's `ui.documents`: list, `new`, `switch`, `close` (by index; `force`
/// closes without asking), `close_others` (all but `index`), `close_all`, `move` (`from`, `to`)
/// or `quit`; `answer`: save | dont_save | cancel answers "save changes?". `save_indices`
/// sets every tick in a grouped prompt; `save_row` / `discard_row` answer one tab index;
/// `save_prompt` lists the remaining rows and their current tab indices. `open_elsewhere`:
/// read_only | anyway | cancel and `changed_on_disk`: save_copy | overwrite | cancel answer those
/// questions (#5467).
pub fn control(app: &mut SolveApp, p: &Value) -> Value {
    let i = p.get("index").and_then(Value::as_u64).map(|x| x as usize);
    match p.get("action").and_then(Value::as_str) {
        Some("new") => new_design(app),
        Some("switch") => switch(app, i.unwrap_or(app.docs.active)),
        Some("close") => close(app, i.unwrap_or(app.docs.active), p.get("force").and_then(Value::as_bool).unwrap_or(false)),
        Some("close_others") => {
            let keep = i.unwrap_or(app.docs.active);
            close_many(app, (0..app.docs.count()).filter(|k| *k != keep).collect());
        }
        Some("close_all") => close_many(app, (0..app.docs.count()).collect()),
        Some("move") => {
            let at = |k: &str| p.get(k).and_then(Value::as_u64).map(|x| x as usize);
            if let (Some(from), Some(to)) = (at("from"), at("to")) {
                move_tab(app, from, to);
            }
        }
        Some("quit") => request_quit(app),
        _ => {}
    }
    if let Some(selected) = p.get("save_indices") {
        let Some(indices) = selected.as_array() else { return json!({"error": "save_indices must be an array of tab indices"}) };
        let Some(group) = app.docs.group.as_mut().filter(|_| !app.docs.saving) else { return json!({"error": "no grouped prompt waiting"}) };
        if indices.len() > MAX_OPEN
            || indices.iter().any(|v| v.as_u64().is_none_or(|i| !group.rows.iter().any(|(row, _)| u64::try_from(*row).ok() == Some(i))))
        {
            return json!({"error": "save_indices must identify designs in the prompt"});
        }
        for (i, checked) in &mut group.rows {
            *checked = indices.iter().any(|v| v.as_u64() == u64::try_from(*i).ok());
        }
    }
    for (key, save) in [("save_row", true), ("discard_row", false)] {
        if let Some(value) = p.get(key) {
            let index = value.as_u64().and_then(|i| usize::try_from(i).ok());
            let Some(index) = index.filter(|i| app.docs.group.as_ref().is_some_and(|g| g.rows.iter().any(|(row, _)| row == i))) else {
                return json!({"error": "row must identify a design in the grouped prompt"});
            };
            answer_row(app, index, save);
        }
    }
    let answer = match p.get("answer").and_then(Value::as_str) {
        Some("save") => Some(Answer::Save),
        Some("dont_save") => Some(Answer::DontSave),
        Some("cancel") => Some(Answer::Cancel),
        _ => None,
    };
    if let Some(a) = answer {
        answer_prompt(app, a);
    }
    use crate::open_elsewhere::{DiskAnswer, LockAnswer, answer_disk, answer_lock};
    match p.get("open_elsewhere").and_then(Value::as_str) {
        Some("read_only") => answer_lock(app, LockAnswer::ReadOnly),
        Some("anyway") => answer_lock(app, LockAnswer::Anyway),
        Some("cancel") => answer_lock(app, LockAnswer::Cancel),
        _ => {}
    }
    match p.get("changed_on_disk").and_then(Value::as_str) {
        Some("save_copy") => answer_disk(app, DiskAnswer::SaveCopy),
        Some("overwrite") => answer_disk(app, DiskAnswer::Overwrite),
        Some("cancel") => answer_disk(app, DiskAnswer::Cancel),
        _ => {}
    }
    let tabs: Vec<Value> = tabs_info(app).into_iter().map(|(name, dirty)| json!({"name": name, "dirty": dirty})).collect();
    let save_prompt = app.docs.group.as_ref().map(|group| {
        group
            .rows
            .iter()
            .map(|(index, selected)| {
                let session = if *index == app.docs.active {
                    Some(&app.session)
                } else {
                    app.docs.slots.get(*index).and_then(Option::as_ref).map(|s| &s.session)
                };
                json!({"index": index, "name": session.map(|s| &s.doc.name), "path": session.and_then(|s| s.path.as_ref()), "selected": selected})
            })
            .collect::<Vec<_>>()
    });
    json!({
        "save_prompt": save_prompt,
        "documents": tabs,
        "active": app.docs.active,
        "closing": app.docs.closing,
        "quitting": app.docs.quitting.is_some(),
        "batch": app.docs.batch,
        "read_only": app.session.read_only,
        "open_elsewhere": app.lock_prompts.len(),
        "changed_on_disk": app.disk_prompt,
    })
}

/// Where SolidCraft's tab strip publishes its parts (scripts and scenarios use these names).
const TAB_HANDLES: crate::app_bar::TabHandles<'static> = crate::app_bar::TabHandles {
    tab: "tab:design:",
    close: "tab:close:",
    more: "tab:overflow",
    new: "tab:new",
    rename: "tab:rename",
    menu_row: "tab_menu:",
    more_row: "document:",
};

/// A tab's menu: close it, the others, or all of them.
const TAB_MENU: [crate::app_bar::TabMenuRow<'static>; 3] = [
    crate::app_bar::TabMenuRow { id: "ui.closeTab", label: "Close", key_on_selected: "Ctrl+W", needs_others: false },
    crate::app_bar::TabMenuRow { id: "ui.closeOtherTabs", label: "Close Other Tabs", key_on_selected: "", needs_others: true },
    crate::app_bar::TabMenuRow { id: "ui.closeAllTabs", label: "Close All Tabs", key_on_selected: "", needs_others: false },
];

/// The document tabs in the application bar, from `x0` to `x1` ([`crate::app_bar::tabs`]): click
/// to switch, × to close, drag to reorder, + for the start page.
pub fn tabs(app: &mut SolveApp, ui: &mut egui::Ui, r: Rect, x0: f32, x1: f32) {
    let info = tabs_info(app);
    let paths = tab_paths(app);
    let n = info.len();
    let locale = solidcraft_ui_text::Locale::for_language(&app.preferences.language);
    let names: Vec<String> = info.iter().map(|(name, _)| locale.given_name(name)).collect();
    let security = tab_security(app);
    let badges = tab_badges(app);
    let places: Vec<String> = info
        .iter()
        .zip(&paths)
        .zip(&security)
        .map(|(((_, dirty), path), security)| tab_place(path.as_deref(), *dirty, security.as_deref()))
        .collect();
    let selected = if app.docs.slots.is_empty() { 0 } else { app.docs.active };
    let mut rename = app.menu.rename.take();
    let renaming = renaming_tab_state(rename.as_ref());
    let new_tip = app.tr("ui.start_page").to_string();
    let mut tab_menu = TAB_MENU;
    if crate::keymap::nx_active(app) && !app.embed_editor {
        for row in &mut tab_menu {
            if row.id == "ui.closeTab" {
                row.key_on_selected = "";
            }
        }
    }
    let actions = {
        let mut strip = crate::app_bar::TabStrip {
            tabs: info
                .iter()
                .zip(&names)
                .zip(&places)
                .zip(&badges)
                .map(|((((list_name, dirty), name), place), security)| crate::app_bar::DocTab {
                    name,
                    list_name,
                    dirty: *dirty,
                    place,
                    security: *security,
                })
                .collect(),
            selected,
            shown_selected: (!app.home.open).then_some(selected),
            title_bar: app.custom_titlebar || app.integrated_titlebar,
            double_click_renames: app.preferences.tab_double_click_renames,
            rename: rename.as_mut().filter(|_| renaming).map(|r| crate::app_bar::TabRename {
                text: &mut r.text,
                focused: &mut r.focused,
                max: MAX_DESIGN_NAME,
            }),
            menu: &tab_menu,
            close_name: solidcraft_ui_text::tr("ui.close_tab"),
            close_tip: "Close",
            more_tip: solidcraft_ui_text::tr_label("All open designs"),
            new_tip: &new_tip,
            id: tab_strip_id(),
            handles: TAB_HANDLES,
        };
        crate::app_bar::tabs(ui, &mut strip, r, x0, x1)
    };
    // The name typed into the selected tab: kept when it changed and isn't blank, given up on
    // Escape, still being edited otherwise.
    let mut kept = None;
    for action in &actions {
        match action {
            crate::app_bar::TabAction::RenameDone => kept = Some(true),
            crate::app_bar::TabAction::RenameCancelled => kept = Some(false),
            _ => {}
        }
    }
    match (kept, rename) {
        (Some(true), Some(done)) => {
            if names.get(selected).is_some_and(|shown| done.text.trim() != shown.trim()) {
                let _ = crate::context_menu::commit_rename(app, &done);
            }
        }
        (Some(false), _) | (_, None) => {}
        (None, Some(editing)) => app.menu.rename = Some(editing),
    }
    // A file loading into the window keeps the tabs as they are (dragging still moves it).
    let idle = !app.file_load_holds();
    let (mut shut, mut pick, mut many, mut dropped) = (None, None, None, None);
    for action in actions {
        match action {
            crate::app_bar::TabAction::Select(i) => pick = Some(i),
            crate::app_bar::TabAction::Close(i) => shut = Some(i),
            crate::app_bar::TabAction::Menu { tab, id } => match id.as_str() {
                "ui.closeTab" => shut = Some(tab),
                "ui.closeOtherTabs" => many = Some((0..n).filter(|k| *k != tab).collect::<Vec<_>>()),
                "ui.closeAllTabs" => many = Some((0..n).collect()),
                _ => {}
            },
            crate::app_bar::TabAction::Reorder { from, to } => dropped = Some((from, to)),
            crate::app_bar::TabAction::StartRename { tab, rect } => {
                if let Some((name, _)) = info.get(tab) {
                    start_tab_rename(app, name, rect);
                }
            }
            crate::app_bar::TabAction::Maximize => crate::titlebar::toggle_maximize(ui.ctx()),
            crate::app_bar::TabAction::New => {
                if idle {
                    app.home.show(app.session.revision);
                }
            }
            crate::app_bar::TabAction::RenameDone | crate::app_bar::TabAction::RenameCancelled => {}
        }
    }
    match (shut, pick) {
        _ if !idle => {}
        _ if many.is_some() => close_many(app, many.unwrap_or_default()),
        (Some(i), _) => close(app, i, false),
        (None, Some(i)) => switch(app, i),
        (None, None) => {
            if let Some((from, to)) = dropped {
                move_tab(app, from, to);
            }
        }
    }
}

/// The longest name a design takes (as `component.rename` accepts).
const MAX_DESIGN_NAME: usize = 128;

fn renaming_tab_state(rename: Option<&crate::context_menu::Rename>) -> bool {
    rename.is_some_and(|r| r.inline && matches!(r.what, crate::context_menu::RenameWhat::Component(0)))
}

/// Starts editing the selected design's name in its tab, with the name as shown selected.
fn start_tab_rename(app: &mut SolveApp, name: &str, tab: Rect) {
    let text = solidcraft_ui_text::Locale::for_language(&app.preferences.language).given_name(name);
    app.menu.rename = Some(crate::context_menu::Rename {
        what: crate::context_menu::RenameWhat::Component(0),
        text,
        at: tab.left_bottom(),
        focused: false,
        inline: true,
        inline_id: None,
    });
}

/// What "save changes?" was answered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Answer {
    #[default]
    Save,
    DontSave,
    Cancel,
}

/// The "save changes?" prompt for a tab being closed, or for each unsaved design when quitting.
pub fn prompt(app: &mut SolveApp, ctx: &egui::Context) {
    let _language = solidcraft_ui_text::scope(&app.preferences.language);
    quit_step(app);
    // Saved, it asks the Capture Position question first (#9736): that question is the one to
    // answer meanwhile, not this one again.
    if app.tree.capture_prompt.is_some() {
        return;
    }
    let Some(i) = app.docs.closing else { return };
    let name = tabs_info(app).get(i).map(|x| x.0.clone()).unwrap_or_default();
    let name_display = crate::i18n::tr_name(&name);
    let mut answer: Option<Answer> = None;
    // The original single-design prompt keeps its arrow-key choice. The grouped prompt
    // always gives Save Selected on Enter.
    let order = [Answer::DontSave, Answer::Cancel, Answer::Save];
    let (left, right, enter) = ctx.input_mut(|x| {
        (
            x.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft),
            x.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight),
            x.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
        )
    });
    let at = order.iter().position(|a| *a == app.docs.choice).unwrap_or(order.len() - 1);
    let at = if left {
        at.saturating_sub(1)
    } else if right {
        (at + 1).min(order.len() - 1)
    } else {
        at
    };
    let choice = if app.docs.group.is_some() { Answer::Save } else { order.get(at).copied().unwrap_or_default() };
    app.docs.choice = choice;
    if enter {
        answer = Some(choice);
    }
    let grouped = app.docs.group.is_some();
    let title = if let Some(group) = &app.docs.group {
        solidcraft_ui_text::format("document.save_many", &[("n", &group.rows.len().to_string())])
    } else {
        solidcraft_ui_text::format("document.save_changes", &[("name", &name_display)])
    };
    let save_label = solidcraft_ui_text::tr(if grouped { "document.save_selected" } else { "ui.save" });
    let mut show_tab = None;
    let mut row_action = None;
    let (closed, action) = ui_kit::Confirm::new(egui::Id::unique("sc_close_prompt"), "status_warning", &title)
        .tone(ui_kit::Tone::Warning)
        .actions(save_label, solidcraft_ui_text::tr("ui.cancel"), Some(solidcraft_ui_text::tr("ui.don_t_save")))
        .secondary_left()
        .selected(match choice {
            Answer::Save => ui_kit::Action::Ok,
            Answer::DontSave => ui_kit::Action::Apply,
            Answer::Cancel => ui_kit::Action::Cancel,
        })
        .show(ctx, true, |ui| {
            if let Some(group) = app.docs.group.as_mut() {
                let t = solidcraft_theme::Tokens::get();
                let row_height = t.control_height + t.font_small + t.space_8 + t.space_8;
                let chrome = t.form_close_size + t.form_footer_height + t.space_24 * 3.0;
                let height = (row_height * 8.0).min((ctx.content_rect().height() - chrome).max(t.control_height));
                let list = egui::ScrollArea::vertical().max_height(height).show(ui, |ui| {
                    for (index, checked) in &mut group.rows {
                        let session = if *index == app.docs.active {
                            Some(&app.session)
                        } else {
                            app.docs.slots.get(*index).and_then(Option::as_ref).map(|s| &s.session)
                        };
                        if let Some(session) = session {
                            ui.push_id(*index, |ui| {
                                ui.horizontal_top(|ui| {
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                                        ui.spacing_mut().item_spacing.x = t.space_8;
                                        for (key, save) in [("document.discard_row", false), ("document.save_row", true)] {
                                            let button = ui_kit::button(ui, solidcraft_ui_text::tr(key), ui_kit::Button::Small);
                                            let action = if save { "save" } else { "discard" };
                                            crate::scenario::publish_target(&format!("save_many:{action}:{index}"), button.rect);
                                            if button.clicked() {
                                                row_action = Some((*index, save));
                                            }
                                        }
                                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                                            let tick = ui_kit::checkbox(ui, "", checked);
                                            tick.widget_info(|| {
                                                egui::WidgetInfo::selected(
                                                    egui::Role::CheckBox,
                                                    ui.is_enabled(),
                                                    *checked,
                                                    crate::i18n::tr_name(&session.doc.name),
                                                )
                                            });
                                            crate::scenario::publish_target(&format!("save_many:tick:{index}"), tick.rect);
                                            ui.vertical(|ui| {
                                                ui.spacing_mut().item_spacing.y = 0.0;
                                                ui.spacing_mut().button_padding.x = 0.0;
                                                let name = ui_kit::button(ui, crate::i18n::tr_name(&session.doc.name), ui_kit::Button::Ghost);
                                                crate::scenario::publish_target(&format!("save_many:name:{index}"), name.rect);
                                                if name.clicked() {
                                                    show_tab = Some(*index);
                                                }
                                                let path = ui_kit::text(
                                                    ui,
                                                    session.path.as_deref().unwrap_or(solidcraft_ui_text::tr("document.not_saved_yet")),
                                                    ui_kit::Text::Note("save_path"),
                                                );
                                                crate::scenario::publish_rect(&format!("save_many:path:{index}"), path.rect);
                                            });
                                        });
                                    });
                                });
                            });
                            ui.add_space(t.space_8);
                        }
                    }
                });
                crate::scenario::publish_rect("save_many:list", list.inner_rect);
            } else {
                ui_kit::text(
                    ui,
                    &solidcraft_ui_text::format("document.unsaved_changes", &[("name", &name_display)]),
                    ui_kit::Text::Report("unsaved", false),
                );
            }
        });
    if let Some((index, save)) = row_action {
        answer_row(app, index, save);
        return;
    }
    if let Some(index) = show_tab {
        switch(app, index);
    }
    answer = match action {
        ui_kit::Action::Ok => Some(Answer::Save),
        ui_kit::Action::Apply => Some(Answer::DontSave),
        ui_kit::Action::Cancel => Some(Answer::Cancel),
        ui_kit::Action::None if closed => Some(Answer::Cancel),
        ui_kit::Action::None => answer,
    };
    if ctx.input(|x| x.key_pressed(egui::Key::Escape)) {
        answer = Some(Answer::Cancel);
        app.esc_handled = true;
    }
    if let Some(a) = answer {
        answer_prompt(app, a);
    }
}

/// Act on the "save changes?" answer (also the control channel's `ui.documents {answer}`).
pub fn answer_prompt(app: &mut SolveApp, answer: Answer) {
    if app.file_picker_busy() || app.tree.capture_prompt.is_some() {
        return;
    }
    let Some(i) = app.docs.closing else { return };
    app.docs.choice = Answer::Save;
    if app.docs.group.is_some() {
        match answer {
            Answer::Save => {
                if let Some(group) = app.docs.group.as_mut() {
                    group.pending = group.rows.iter().rev().filter(|(_, checked)| *checked).map(|(i, _)| *i).collect();
                }
                save_group_next(app);
                return;
            }
            Answer::DontSave => {
                finish_group(app);
                return;
            }
            Answer::Cancel => {
                app.docs.group = None;
            }
        }
    }
    match answer {
        Answer::Save => {
            switch(app, i);
            app.docs.saving = true;
            crate::toolbar::save(app);
            if !app.file_picker_busy() {
                saved_for_close(app);
            }
        }
        Answer::DontSave => {
            if let Some(skip) = app.docs.quitting.as_mut() {
                skip.push(i);
                app.docs.closing = None;
            } else {
                close(app, i, true);
            }
        }
        Answer::Cancel => {
            app.docs.closing = None;
            app.docs.quitting = None;
            app.docs.batch = None;
            // The tab left in front was shown on the way without reading what it missed.
            crate::linked_source::update_changed(app);
        }
    }
    quit_step(app);
    batch_step(app);
}

/// Resume Close/Quit only after Save As has answered and the write succeeded.
pub(crate) fn saved_for_close(app: &mut SolveApp) {
    // Save asks the Capture Position question first while a move isn't captured (#9736): the
    // close waits for its answer.
    if app.tree.capture_prompt.is_some() {
        return;
    }
    if !std::mem::take(&mut app.docs.saving) || app.docs.closing.is_none() {
        return;
    }
    let saved = !app.session.is_dirty();
    let row_save = app.docs.group.as_mut().and_then(|g| g.saving_row.take());
    if let Some((previous, closing)) = row_save {
        if saved {
            close_group_row(app, app.docs.active);
        } else {
            switch(app, previous);
            app.docs.closing = Some(closing);
        }
        return;
    }
    if app.docs.group.is_some() {
        if saved {
            save_group_next(app);
        } else {
            answer_prompt(app, Answer::Cancel);
        }
        return;
    }
    app.docs.closing = None;
    if app.docs.quitting.is_some() {
        if !saved {
            app.docs.quitting = None;
            crate::linked_source::update_changed(app);
        }
    } else if saved {
        close(app, app.docs.active, true);
    } else {
        // Save As cancelled: the tabs not closed yet stay, the one in front reads what it missed.
        app.docs.batch = None;
        crate::linked_source::update_changed(app);
    }
    quit_step(app);
    batch_step(app);
}

/// Where SolidCraft's document tab strip keeps its state.
fn tab_strip_id() -> egui::Id {
    egui::Id::unique("sc_doc_tabs")
}

/// Whether the document tab strip's list of every open design is open.
pub fn tab_list_open(ctx: &egui::Context) -> bool {
    crate::app_bar::tab_list_open(ctx, tab_strip_id())
}

/// A tab's tooltip ([`crate::app_bar::tab_tip`] over [`tab_place`]).
#[cfg(test)]
fn tab_tooltip(name: &str, path: Option<&str>, cut: bool, dirty: bool) -> String {
    crate::app_bar::tab_tip(name, &tab_place(path, dirty, None), cut)
}

#[cfg(test)]
use crate::app_bar::{drop_slot, tab_layout as layout, tab_shape};
#[cfg(test)]
use egui::{pos2, vec2};
#[cfg(test)]
use solidcraft_theme::Tokens;

/// Whether the selected tab's design name is being edited in the tab.
#[cfg(test)]
fn renaming_tab(app: &SolveApp) -> bool {
    renaming_tab_state(app.menu.rename.as_ref())
}

#[cfg(test)]
mod tests {
    fn title_commands(app: &mut SolveApp) -> Vec<String> {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(Default::default(), |ui| sync_window_title(app, ui.ctx()));
        output.textures_delta.clear();
        output
            .viewport_output
            .into_iter()
            .filter(|(id, _)| *id == egui::ViewportId::ROOT)
            .flat_map(|(_, output)| output.commands)
            .filter_map(|command| match command {
                egui::ViewportCommand::Title(title) => Some(title),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn only_a_listed_fingerprint_is_trusted() {
        assert!(trusted_fingerprint("0123456789ABCDEF, fedcba9876543210", "0123456789abcdef"));
        assert!(trusted_fingerprint("0123456789ABCDEF; fedcba9876543210", "FEDCBA9876543210"));
        // `gpg --fingerprint` prints a fingerprint in groups of four.
        assert!(trusted_fingerprint("0123 4567 89AB CDEF", "0123456789ABCDEF"));
        assert!(!trusted_fingerprint("", "0123456789ABCDEF"));
        assert!(!trusted_fingerprint("1111111111111111", "0123456789ABCDEF"));
        assert!(!trusted_fingerprint("0123456789ABCDEF", ""));
        // Two fingerprints must be separated by a comma; a run of spaces is one grouped fingerprint.
        assert!(!trusted_fingerprint("0123456789ABCDEF 1111111111111111", "0123456789ABCDEF"));
    }

    #[test]
    fn an_untrusted_user_id_is_not_shown_for_a_valid_signature() {
        let record = solidcraft_engine::io::ProvenanceRecord {
            signer: solidcraft_engine::io::ProvenanceSigner::OpenPgp {
                fingerprint: "0123456789ABCDEF".into(),
                user_id: Some("Alice <alice@example.invalid>".into()),
            },
            author: Some("Alice".into()),
            tool: None,
            note: None,
            created: None,
            app: "test".into(),
            format: "solidcraft/55".into(),
            pending: Vec::new(),
            document: [0; 32],
            container: [0; 32],
            parts: 0,
            signature_valid: true,
            covered: true,
        };
        let report = solidcraft_engine::io::ProvenanceReport {
            chain: Default::default(),
            records: vec![record],
            status: solidcraft_engine::io::ProvenanceStatus::Valid,
        };
        let record = report.newest().unwrap();
        assert_eq!(record.signer.fingerprint(), "0123456789ABCDEF");
        assert_eq!(signer_name(record), "Alice <alice@example.invalid>");
        assert!(!trusted_fingerprint("", &record.signer.fingerprint()));
        assert!(trusted_fingerprint("0123 4567 89AB CDEF", &record.signer.fingerprint()));
    }

    #[test]
    fn tab_badges_show_the_last_check_or_that_the_file_is_signed() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let record = solidcraft_engine::io::ProvenanceRecord {
            signer: solidcraft_engine::io::ProvenanceSigner::OpenPgp { fingerprint: "0123456789ABCDEF".into(), user_id: None },
            author: None,
            tool: None,
            note: None,
            created: None,
            app: "test".into(),
            format: "solidcraft/55".into(),
            pending: Vec::new(),
            document: [0; 32],
            container: [0; 32],
            parts: 0,
            signature_valid: true,
            covered: true,
        };
        h.app.session.provenance = Some(solidcraft_engine::io::ProvenanceReport {
            chain: Default::default(),
            records: vec![record],
            status: solidcraft_engine::io::ProvenanceStatus::Valid,
        });
        h.app.session.mark_saved();
        h.app.preferences.trusted_signing_keys = "0123456789ABCDEF".into();
        assert_eq!(tab_badges(&h.app), vec![Some(crate::app_bar::TabBadge::Verified)]);
        h.app.session.provenance = Some(solidcraft_engine::io::ProvenanceReport {
            chain: Default::default(),
            records: vec![],
            status: solidcraft_engine::io::ProvenanceStatus::Valid,
        });
        h.app.preferences.trusted_signing_keys.clear();
        assert_eq!(tab_badges(&h.app), vec![Some(crate::app_bar::TabBadge::Signed)]);
        h.app.session.provenance = None;
        h.app.session.provenance_chain = Some(solidcraft_engine::io::ProvenanceChain { records: vec![vec![1]], truncated: false });
        assert_eq!(tab_badges(&h.app), vec![Some(crate::app_bar::TabBadge::Signed)]);
    }

    #[test]
    fn window_title_tracks_live_design_save_rename_switch_and_open() {
        let mut a = app();
        a.home.open = false;
        let initial = a.session.doc.name.clone();
        assert_eq!(title_commands(&mut a), [format!("{initial} — SolidCraft")]);
        assert!(title_commands(&mut a).is_empty(), "unchanged frames send no command");
        a.session.execute("component.rename", &json!({"component": 0, "name": "Bracket"})).unwrap();
        assert_eq!(title_commands(&mut a), ["Bracket * — SolidCraft"]);
        let path = std::env::temp_dir().join(format!("solidcraft-window-title-{}.solidcraft", std::process::id()));
        let path = path.to_string_lossy().into_owned();
        a.session.execute("file.save", &json!({"path": path})).unwrap();
        let saved = a.session.doc.name.clone();
        assert_eq!(title_commands(&mut a), [format!("{saved} — SolidCraft")]);
        a.session.execute("component.rename", &json!({"component": 0, "name": "Housing"})).unwrap();
        assert_eq!(title_commands(&mut a), ["Housing * — SolidCraft"]);
        a.session.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(title_commands(&mut a), [format!("{saved} — SolidCraft")]);
        new_tab(&mut a);
        let blank = a.session.doc.name.clone();
        assert_eq!(title_commands(&mut a), [format!("{blank} — SolidCraft")]);
        switch(&mut a, 0);
        assert_eq!(title_commands(&mut a), [format!("{saved} — SolidCraft")]);
        switch(&mut a, 1);
        title_commands(&mut a);
        a.open_path(&path);
        assert_eq!(title_commands(&mut a), [format!("{saved} — SolidCraft")]);
        a.session.execute("component.rename", &json!({"component": 0, "name": "Saved copy"})).unwrap();
        assert_eq!(title_commands(&mut a), ["Saved copy * — SolidCraft"]);
        let copy = format!("{path}.copy.solidcraft");
        a.session.execute("file.save_as", &json!({"path": copy})).unwrap();
        let copy_name = std::path::Path::new(&copy).file_stem().unwrap().to_str().unwrap();
        assert_eq!(title_commands(&mut a), [format!("{copy_name} — SolidCraft")]);
        a.open_path(&path);
        assert_eq!(title_commands(&mut a), [format!("{saved} — SolidCraft")]);
        std::fs::remove_file(copy).unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn window_title_respects_home_and_embedded_hosts() {
        let mut a = app();
        a.home.open = true;
        assert_eq!(title_commands(&mut a), ["SolidCraft"]);
        assert!(title_commands(&mut a).is_empty());
        a.home.open = false;
        let name = a.session.doc.name.clone();
        assert_eq!(title_commands(&mut a), [format!("{name} — SolidCraft")]);
        for editor in [false, true] {
            let mut embedded = app();
            embedded.home.open = false;
            embedded.embedded = !editor;
            embedded.embed_editor = editor;
            assert!(title_commands(&mut embedded).is_empty(), "the host owns the title");
        }
    }

    #[test]
    fn window_title_keeps_existing_design_when_start_page_is_shown() {
        let mut a = app();
        a.home.open = false;
        a.session.execute("component.rename", &json!({"component": 0, "name": "Bracket"})).unwrap();
        assert_eq!(title_commands(&mut a), ["Bracket * — SolidCraft"]);
        a.home.open = true;
        assert!(title_commands(&mut a).is_empty(), "opening the start page does not close the design");
        close(&mut a, 0, true);
        assert_eq!(title_commands(&mut a), ["SolidCraft"], "closing the last design leaves only the blank placeholder");
    }

    #[test]
    fn window_title_formats_dirty_state_and_suppresses_unchanged_titles() {
        let mut title = WindowTitle::default();
        assert_eq!(title.update(Some("Plate"), false, "en").as_deref(), Some("Plate — SolidCraft"));
        assert!(title.update(Some("Plate"), false, "en").is_none());
        assert_eq!(title.update(Some("Plate"), true, "en").as_deref(), Some("Plate * — SolidCraft"));
        assert_eq!(title.update(None, true, "en").as_deref(), Some("SolidCraft"));
        assert!(title.update(None, false, "fr").is_none());
    }

    #[test]
    fn reopening_saved_design_preserves_dirty_session_and_view() {
        let mut a = super::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let path = std::env::temp_dir().join(format!("solidcraft-reopen-{}.solidcraft", std::process::id()));
        let path = path.to_string_lossy().into_owned();
        a.session.execute("file.save", &serde_json::json!({"path": path})).unwrap();
        a.session.execute("solid.box", &serde_json::json!({"length": 10, "width": 12, "height": 14})).unwrap();
        a.session.selection = vec![solidcraft_engine::Sel::Body { name: "Body1".into() }];
        a.cam.distance = 123.0;
        a.session.visibility.hidden_profiles = vec![7];
        let doc = a.session.doc.clone();
        let undo = a.session.undo.len();
        let cam = a.cam;
        a.open_path(&path);
        assert_eq!(a.docs.count(), 1);
        assert!(std::sync::Arc::ptr_eq(&doc, &a.session.doc));
        assert!(a.session.is_dirty());
        assert_eq!(a.session.undo.len(), undo);
        assert_eq!(a.cam, cam);
        assert_eq!(a.session.selection.len(), 1);
        assert_eq!(a.session.visibility.hidden_profiles, vec![7]);
        super::new_tab(&mut a);
        a.open_path(&path);
        assert_eq!(a.docs.count(), 2);
        assert_eq!(a.docs.active, 0);
        assert!(std::sync::Arc::ptr_eq(&doc, &a.session.doc));
        assert_eq!(a.cam, cam);
        assert_eq!(a.session.selection.len(), 1);
        assert_eq!(a.session.visibility.hidden_profiles, vec![7]);
        assert_eq!(a.session.undo.len(), undo);
        assert!(a.session.is_dirty());
        std::fs::remove_file(&path).unwrap();
        a.open_path(&path);
        assert_eq!(a.docs.count(), 2);
        assert!(std::sync::Arc::ptr_eq(&doc, &a.session.doc));
    }

    #[test]
    fn follow_file_rename_updates_every_open_tab_title() {
        let mut a = app();
        a.session.doc_mut().name = "Plate".into();
        a.session.path = Some("opfs://Plate.solidcraft".into());
        a.session.user_named = false;
        new_tab(&mut a);
        a.session.doc_mut().name = "Other".into();
        a.session.path = Some("opfs://Other.solidcraft".into());
        // Background tab is Plate; active is Other.
        follow_file_rename(&mut a, "opfs://Plate.solidcraft", "opfs://Bracket.solidcraft");
        assert_eq!(a.session.doc.name, "Other");
        assert_eq!(a.session.path.as_deref(), Some("opfs://Other.solidcraft"));
        let bg = a.docs.background().next().expect("background Plate");
        assert_eq!(bg.doc.name, "Bracket");
        assert_eq!(bg.path.as_deref(), Some("opfs://Bracket.solidcraft"));
        switch(&mut a, 0);
        assert_eq!(a.session.doc.name, "Bracket");
    }

    #[test]
    fn reuse_tracks_save_as_missing_files_and_full_capacity() {
        let mut a = app();
        a.session.path = Some("original.solidcraft".into());
        let original = a.session.doc.clone();
        // Save As updates the live path; lookup must not retain the old identity.
        a.session.path = Some("renamed.solidcraft".into());
        assert!(!reuse_path(&mut a, "original.solidcraft").unwrap());
        for _ in 1..MAX_OPEN {
            new_tab(&mut a);
        }
        assert_eq!(a.docs.count(), MAX_OPEN);
        assert!(reuse_path(&mut a, "renamed.solidcraft").unwrap());
        assert_eq!(a.docs.active, 0);
        assert!(std::sync::Arc::ptr_eq(&original, &a.session.doc));
        assert!(!reuse_path(&mut a, "elsewhere/renamed.solidcraft").unwrap());
        assert_eq!(a.docs.count(), MAX_OPEN);
        a.home.open = true;
        assert!(reuse_path(&mut a, "renamed.solidcraft").unwrap());
        assert!(!a.home.open);
    }

    #[test]
    fn saved_tabs_show_file_names_and_unsaved_tabs_keep_design_names() {
        let mut a = app();
        let design = a.session.doc.name.clone();
        assert_eq!(tabs_info(&a)[0].0, design);
        a.session.path = Some("parts/bracket.solidcraft".into());
        assert_eq!(tabs_info(&a)[0].0, "bracket");
        // Save As under a new revision name: the tab follows the file, not the design's first name.
        a.session.path = Some("parts/bracket rev B.solidcraft".into());
        assert_eq!(tabs_info(&a)[0].0, "bracket rev B");
        new_tab(&mut a);
        let names: Vec<String> = tabs_info(&a).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names.first().map(String::as_str), Some("bracket rev B"));
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn reuse_does_not_claim_a_switch_while_a_picker_is_open() {
        let mut a = app();
        a.session.path = Some("picker.solidcraft".into());
        new_tab(&mut a);
        a.services.pick_open = Some(Box::new(|_| Box::pin(std::future::pending())));
        a.pick_open(|_, _| {});
        assert!(a.file_picker_busy());
        assert!(reuse_path(&mut a, "picker.solidcraft").is_err());
        assert_eq!(a.docs.active, 1);
        assert_eq!(a.docs.count(), 2);
    }

    #[test]
    fn saved_path_identity_uses_the_filesystem_without_folding_case() {
        let dir = std::env::temp_dir().join(format!("solidcraft-tab-paths-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("other")).unwrap();
        let path = dir.join("design.solidcraft");
        std::fs::write(&path, b"identity test").unwrap();
        let text = |p: &std::path::Path| p.to_string_lossy().into_owned();
        assert!(same_path(&text(&path), &text(&dir.join("other/../design.solidcraft"))));
        assert!(!same_path(&text(&path), &text(&dir.join("other/design.solidcraft"))));
        #[cfg(windows)]
        assert!(same_path(&text(&path), &text(&dir.join("DESIGN.solidcraft"))));
        #[cfg(unix)]
        {
            let link = dir.join("alias.solidcraft");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert!(same_path(&text(&path), &text(&link)));
            std::fs::write(dir.join("DESIGN.solidcraft"), b"distinct").unwrap();
            if std::fs::read(&path).unwrap() != b"distinct" {
                assert!(!same_path(&text(&path), &text(&dir.join("DESIGN.solidcraft"))));
            }
        }
        std::fs::remove_file(&path).unwrap();
        assert!(same_path(&text(&path), &text(&dir.join("./design.solidcraft"))));
        std::fs::remove_dir_all(dir).unwrap();
    }

    use super::*;
    use crate::Services;
    use egui::Color32;

    /// A harness with the design open (not the start page).
    fn design_open() -> crate::scenario::Harness {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.frames(2);
        h
    }

    /// A double-click on design tab `i`, as a person gives it.
    fn double_click_tab(h: &mut crate::scenario::Harness, i: usize) {
        h.step(&json!({"click": {"handle": format!("tab:design:{i}")}, "double": true})).unwrap();
        h.frames(2);
    }

    #[test]
    fn double_clicking_the_design_tab_renames_the_design_with_enter() {
        let mut h = design_open();
        let before = h.app.session.doc.name.clone();
        double_click_tab(&mut h, 0);
        assert!(renaming_tab(&h.app), "the tab's name is being edited");
        assert!(crate::scenario::rect_of("tab:rename").is_some(), "the field is in the tab");
        // The whole name is selected, so typing replaces it.
        h.step(&json!({"text": "Bracket"})).unwrap();
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.session.doc.name, "Bracket");
        assert_eq!(tabs_info(&h.app).first().map(|(name, _)| name.as_str()), Some("Bracket"), "the tab shows it");
        assert!(!renaming_tab(&h.app) && crate::scenario::rect_of("tab:rename").is_none(), "the field closed");
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.name, before, "a rename is undone like any change");
    }

    #[test]
    fn clicking_elsewhere_keeps_the_new_name() {
        let mut h = design_open();
        double_click_tab(&mut h, 0);
        let narrow = crate::scenario::rect_of("tab:rename").unwrap().width();
        h.step(&json!({"text": "Hinge bracket, left"})).unwrap();
        h.frames(1);
        let wide = crate::scenario::rect_of("tab:rename").unwrap().width();
        assert!(wide > narrow, "the tab grows with the name typed: {narrow} then {wide}");
        h.step(&json!({"click": [800.0, 600.0]})).unwrap();
        h.frames(2);
        assert_eq!(h.app.session.doc.name, "Hinge bracket, left");
        assert!(!renaming_tab(&h.app));
    }

    #[test]
    fn escape_a_blank_name_or_the_same_name_change_nothing() {
        for finish in ["escape", "blank", "same"] {
            let mut h = design_open();
            let (before, undo) = (h.app.session.doc.name.clone(), h.app.session.undo.len());
            double_click_tab(&mut h, 0);
            match finish {
                "escape" => {
                    h.step(&json!({"text": "Discarded"})).unwrap();
                    h.step(&json!({"key": "Escape"})).unwrap();
                }
                "blank" => {
                    h.step(&json!({"text": "   "})).unwrap();
                    h.step(&json!({"key": "Enter"})).unwrap();
                }
                _ => h.step(&json!({"key": "Enter"})).unwrap(),
            }
            h.frames(2);
            assert_eq!((h.app.session.doc.name.clone(), h.app.session.undo.len()), (before, undo), "{finish}: unchanged");
            assert!(!h.app.session.is_dirty(), "{finish}: nothing to save");
            assert!(!renaming_tab(&h.app), "{finish}: the field closed");
        }
    }

    #[test]
    fn a_name_longer_than_a_design_takes_is_cut_to_fit() {
        let mut h = design_open();
        double_click_tab(&mut h, 0);
        h.step(&json!({"text": "x".repeat(MAX_DESIGN_NAME + 40)})).unwrap();
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.session.doc.name, "x".repeat(MAX_DESIGN_NAME), "the command takes it, cut to 128 characters");
    }

    #[test]
    fn with_the_preference_off_a_double_click_does_not_rename() {
        let mut h = design_open();
        h.call("ui.prefs", json!({"tab_double_click_renames": false}));
        h.frames(2);
        double_click_tab(&mut h, 0);
        assert!(!renaming_tab(&h.app) && crate::scenario::rect_of("tab:rename").is_none());
    }

    /// #5868: a tab's tooltip says where its design is saved (home as `~`), or that it isn't
    /// yet, with the full name above when the tab cut the name short. #11408: a saved design with
    /// changes not saved yet says so below, since a cut name hides the tab's bullet.
    #[test]
    fn tab_tooltip_says_where_the_design_is_saved() {
        crate::i18n::install();
        assert_eq!(tab_tooltip("plate", Some("/srv/designs/plate.solidcraft"), false, false), "/srv/designs/plate.solidcraft");
        assert_eq!(tab_tooltip("a-long-name", Some("/srv/a-long-name.solidcraft"), true, false), "a-long-name\n/srv/a-long-name.solidcraft");
        assert_eq!(tab_tooltip("Untitled", None, false, false), "Not saved yet");
        assert_eq!(tab_tooltip("Untitled", None, true, false), "Untitled\nNot saved yet");
        assert_eq!(tab_tooltip("plate", Some("/srv/designs/plate.solidcraft"), false, true), "/srv/designs/plate.solidcraft\nUnsaved changes");
        assert_eq!(
            tab_tooltip("a-long-name", Some("/srv/a-long-name.solidcraft"), true, true),
            "a-long-name\n/srv/a-long-name.solidcraft\nUnsaved changes"
        );
        // A design never saved says so already, changed or not.
        assert_eq!(tab_tooltip("Untitled", None, true, true), "Untitled\nNot saved yet");
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).filter(|h| !h.is_empty()) {
            // The abbreviation keeps the relative path's native separators after `~/`.
            let relative = std::path::Path::new("Designs").join("bracket.solidcraft");
            let path = std::path::Path::new(&home).join(&relative);
            assert_eq!(tab_tooltip("bracket", path.to_str(), false, false), format!("~/{}", relative.display()));
        }
    }

    fn app() -> SolveApp {
        SolveApp::new(Session::default(), Services::default())
    }

    /// Each tab keeps its own design, undo, selection and camera; closing asks when there are
    /// unsaved changes; the last tab closing leaves a blank design.
    #[test]
    fn the_tab_strip_overflows_into_a_list_keeping_the_active_tab() {
        assert_eq!(layout(2, 0, 800.0), (0, 2, 220.0));
        let (first, shown, w) = layout(12, 11, 600.0);
        assert!(shown < 12 && w == 90.0);
        assert!((first..first + shown).contains(&11), "the active tab stays visible");
        assert!(shown as f32 * w + 26.0 + 34.0 <= 600.0 + 0.5);
        assert_eq!(layout(12, 0, 600.0).0, 0);
    }

    /// Active and hovered document tabs keep the bar's light text readable in both themes.
    #[test]
    fn title_bar_tabs_keep_their_text_readable_on_hover() {
        fn luminance(c: Color32) -> f32 {
            let c = egui::Rgba::from(c);
            0.2126 * c.r() + 0.7152 * c.g() + 0.0722 * c.b()
        }
        // egui blends premultiplied colours in gamma space.
        fn over(top: Color32, bottom: Color32) -> Color32 {
            let ch = |t: u8, b: u8| t.saturating_add(((u16::from(b) * u16::from(255 - top.a())) / 255) as u8);
            Color32::from_rgb(ch(top.r(), bottom.r()), ch(top.g(), bottom.g()), ch(top.b(), bottom.b()))
        }
        for t in [Tokens::light(), Tokens::dark()] {
            let r = Rect::from_min_size(pos2(10.0, 5.0), vec2(80.0, 30.0));
            for (active, hovered) in [(false, false), (false, true), (true, false)] {
                let ctx = crate::fonts::test_context();
                let mut out = ctx.run_ui(egui::RawInput::default(), |ui| tab_shape(ui.painter(), &t, r, active, hovered));
                out.textures_delta.clear();
                let rects: Vec<_> = out
                    .shapes
                    .iter()
                    .filter_map(|s| match &s.shape {
                        egui::Shape::Rect(rect) => Some((rect.clone(), s.clip_rect)),
                        _ => None,
                    })
                    .collect();
                let fill = rects.iter().find(|(rect, _)| rect.fill.a() > 0).map(|(rect, _)| rect.fill).unwrap_or_default();
                let bg = over(fill, t.app_bar);
                let (hi, lo) = (luminance(t.app_bar_text).max(luminance(bg)), luminance(t.app_bar_text).min(luminance(bg)));
                assert!((hi + 0.05) / (lo + 0.05) >= 4.5, "text on a tab (hovered: {hovered}) must stay readable: {bg:?} {t:?}");
                assert!(rects.iter().all(|(rect, _)| rect.stroke.width == 0.0), "tabs have no outline (active: {active})");
            }
        }
    }

    /// Four tabs: the first and third with a part (unsaved), the second and fourth blank.
    fn four_tabs() -> SolveApp {
        let mut a = app();
        a.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        open_tab(&mut a, Session::default()).unwrap();
        open_tab(&mut a, Session::default()).unwrap();
        a.run("solid.box", json!({"length": 5, "width": 5, "height": 5})).unwrap();
        open_tab(&mut a, Session::default()).unwrap();
        assert_eq!(a.docs.count(), 4);
        a
    }

    /// #10220: every open design in tab order, the shown one in its place.
    #[test]
    fn open_sessions_lists_every_tab_in_order() {
        let mut a = app();
        assert_eq!(open_sessions(&a).len(), 1);
        let mut a4 = four_tabs();
        switch(&mut a4, 1);
        let uids: Vec<u64> = open_sessions(&a4).iter().map(|s| s.uid).collect();
        assert_eq!(uids.len(), 4);
        assert_eq!(uids.get(1), Some(&a4.session.uid));
        let dirty: Vec<bool> = open_sessions(&a4).iter().map(|s| s.is_dirty()).collect();
        assert_eq!(dirty, [true, false, true, false]);
        a.run("solid.box", json!({"length": 1, "width": 1, "height": 1})).unwrap();
        assert!(open_sessions(&a).iter().all(|s| s.is_dirty()));
    }

    /// #3055: Close Other Tabs on the first tab closes the blank fourth, asks about the third
    /// (unsaved), and after Don't Save closes it and the blank second: the first is left, its
    /// part as it was.
    #[test]
    fn close_other_tabs_asks_about_unsaved_ones_and_keeps_the_tab() {
        let mut a = four_tabs();
        let v = control(&mut a, &json!({"action": "close_others", "index": 0}));
        assert_eq!(v["closing"], json!(2), "{v}");
        assert_eq!(a.docs.count(), 3, "the blank fourth closed without asking");
        let v = control(&mut a, &json!({"answer": "dont_save"}));
        assert_eq!(a.docs.count(), 1, "{v}");
        assert!(a.docs.closing.is_none() && a.docs.batch.is_none());
        let volume = a.run("inspect.measure", json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
        assert!((volume - 1000.0).abs() < 1e-6, "the first tab's part: {volume}");
    }

    /// #3055: Close All Tabs, and Cancel at the first question: the tabs not closed yet stay.
    /// Close All Tabs on blank designs leaves one blank design.
    #[test]
    fn close_all_tabs_stops_at_cancel_and_ends_on_a_blank_design() {
        let mut a = four_tabs();
        a.preferences.save_prompt_one_at_a_time = true;
        control(&mut a, &json!({"action": "close_all"}));
        assert_eq!((a.docs.count(), a.docs.closing), (3, Some(2)));
        control(&mut a, &json!({"answer": "cancel"}));
        assert_eq!((a.docs.count(), a.docs.closing, a.docs.batch.clone()), (3, None, None), "Cancel keeps the rest");
        control(&mut a, &json!({"action": "close_all"}));
        control(&mut a, &json!({"answer": "dont_save"}));
        control(&mut a, &json!({"answer": "dont_save"}));
        assert_eq!(a.docs.count(), 1);
        assert!(active_is_blank(&a), "all closed: a blank design");
    }

    #[test]
    fn grouped_close_lists_only_targets_and_cancel_keeps_every_tab() {
        let mut a = four_tabs();
        let v = control(&mut a, &json!({"action": "close_all"}));
        assert_eq!(a.docs.count(), 4, "even clean tabs wait for the answer");
        assert_eq!(
            v["save_prompt"],
            json!([
                {"index":0,"name":"Untitled","path":null,"selected":true},
                {"index":2,"name":"Untitled","path":null,"selected":true}
            ])
        );
        control(&mut a, &json!({"save_indices":[2]}));
        assert_eq!(control(&mut a, &json!({}))["save_prompt"][0]["selected"], false);
        control(&mut a, &json!({"answer":"cancel"}));
        assert_eq!(a.docs.count(), 4);
        assert!(dirty(&a, 0) && dirty(&a, 2));
        assert!(a.docs.group.is_none() && a.docs.batch.is_none());
        let v = control(&mut a, &json!({"action":"close_others", "index":0}));
        assert!(v["save_prompt"].is_null(), "one dirty target keeps the original prompt");
        assert_eq!(a.docs.closing, Some(2));
    }

    #[test]
    fn grouped_dont_save_closes_all_and_quit_uses_the_same_prompt() {
        let mut a = four_tabs();
        control(&mut a, &json!({"action":"close_all"}));
        control(&mut a, &json!({"answer":"dont_save"}));
        assert!(active_is_blank(&a) && a.home.open);
        let mut a = four_tabs();
        let v = control(&mut a, &json!({"action":"quit"}));
        assert_eq!(v["save_prompt"].as_array().unwrap().len(), 2);
        control(&mut a, &json!({"answer":"cancel"}));
        assert!(!a.quit_requested && a.docs.count() == 4);
        control(&mut a, &json!({"action":"quit"}));
        control(&mut a, &json!({"answer":"dont_save"}));
        assert!(a.quit_requested && !any_dirty(&a));
    }

    #[test]
    fn grouped_quit_replaces_a_pending_tab_close_with_all_window_designs() {
        for targets in [vec![2], vec![0, 2]] {
            let mut a = four_tabs();
            a.run("solid.box", json!({"length":3,"width":3,"height":3})).unwrap();
            close_many(&mut a, targets);
            assert!(a.docs.closing.is_some());
            let v = control(&mut a, &json!({"action":"quit"}));
            let indices: Vec<_> = v["save_prompt"].as_array().unwrap().iter().map(|row| row["index"].clone()).collect();
            assert_eq!(indices, [json!(0), json!(2), json!(3)]);
            assert_eq!(a.docs.count(), 4, "quitting closes nothing before its answer");
            control(&mut a, &json!({"answer":"dont_save"}));
            assert!(a.quit_requested && !any_dirty(&a));
        }
    }

    #[test]
    fn grouped_row_save_closes_only_its_tab_and_keeps_the_last_row_grouped() {
        let mut h = crate::scenario::Harness::new();
        h.app = four_tabs();
        let file = h.dir.join("row.solidcraft");
        let to = file.to_string_lossy().into_owned();
        h.app.services.pick_save = Some(Box::new(move |_: &str, _: &[&str]| Box::pin(std::future::ready(Some(to.clone())))));
        control(&mut h.app, &json!({"action":"close_all", "save_indices":[2]}));
        control(&mut h.app, &json!({"save_row":0}));
        assert_eq!(h.app.docs.count(), 4, "wait for the write");
        crate::file_picker::tick(&mut h.app, &h.ctx);
        crate::save_job::tick(&mut h.app, true);
        assert_eq!(h.app.docs.count(), 3);
        let rows = control(&mut h.app, &json!({}))["save_prompt"].clone();
        assert_eq!(rows.as_array().unwrap().len(), 1);
        assert_eq!((rows[0]["index"].clone(), rows[0]["selected"].clone()), (json!(1), json!(true)));
        assert!(dirty(&h.app, 1));
        let design = solidcraft_engine::io::read_design(&std::fs::read(file).unwrap()).unwrap();
        assert_eq!(design.features.len(), 1);
        control(&mut h.app, &json!({"answer":"cancel"}));
        assert_eq!(h.app.docs.count(), 3, "Cancel keeps the remaining tabs");
    }

    #[test]
    fn grouped_row_discard_closes_only_its_tab_without_saving() {
        let mut h = crate::scenario::Harness::new();
        h.app = four_tabs();
        let file = h.dir.join("discard.solidcraft");
        switch(&mut h.app, 0);
        h.app.run("file.save_as", json!({"path":file.to_string_lossy()})).unwrap();
        let before = std::fs::read(&file).unwrap();
        h.app.run("solid.sphere", json!({"diameter":4})).unwrap();
        control(&mut h.app, &json!({"action":"close_all", "discard_row":0}));
        assert_eq!(h.app.docs.count(), 3);
        assert_eq!(std::fs::read(file).unwrap(), before);
        assert!(dirty(&h.app, 1));
        assert_eq!(control(&mut h.app, &json!({}))["save_prompt"][0]["index"], 1);
    }

    #[test]
    fn grouped_last_row_finishes_close_or_quit() {
        for quit in [false, true] {
            let mut h = crate::scenario::Harness::new();
            h.app = four_tabs();
            let file = h.dir.join("last.solidcraft");
            let to = file.to_string_lossy().into_owned();
            h.app.services.pick_save = Some(Box::new(move |_: &str, _: &[&str]| Box::pin(std::future::ready(Some(to.clone())))));
            control(&mut h.app, &json!({"action":if quit { "quit" } else { "close_all" }}));
            control(&mut h.app, &json!({"discard_row":2}));
            assert_eq!(h.app.docs.count(), 3);
            control(&mut h.app, &json!({"save_row":0}));
            crate::file_picker::tick(&mut h.app, &h.ctx);
            crate::save_job::tick(&mut h.app, true);
            assert!(h.app.docs.group.is_none() && h.app.docs.closing.is_none());
            assert_eq!(h.app.quit_requested, quit);
            if !quit {
                assert!(active_is_blank(&h.app) && h.app.home.open);
            }
        }
        for quit in [false, true] {
            let mut a = four_tabs();
            close(&mut a, 3, true);
            close(&mut a, 1, true);
            control(&mut a, &json!({"action":if quit { "quit" } else { "close_all" }}));
            control(&mut a, &json!({"discard_row":0}));
            control(&mut a, &json!({"discard_row":0}));
            assert!(a.docs.group.is_none() && a.docs.closing.is_none());
            assert!(active_is_blank(&a));
            assert_eq!(a.quit_requested, quit, "last real tab closes too");
        }
        let mut a = four_tabs();
        close_many(&mut a, vec![0, 2]);
        control(&mut a, &json!({"discard_row":0}));
        control(&mut a, &json!({"discard_row":1}));
        assert_eq!(a.docs.count(), 2, "only the targeted tabs close");
        assert!(a.docs.group.is_none() && a.docs.batch.is_none());
    }

    #[test]
    fn grouped_row_cancelled_picker_or_failed_save_keeps_the_row() {
        for fail in [false, true] {
            let mut h = crate::scenario::Harness::new();
            h.app = four_tabs();
            let bad = h.dir.join("missing").join("row.solidcraft").to_string_lossy().into_owned();
            h.app.services.pick_save = Some(Box::new(move |_: &str, _: &[&str]| Box::pin(std::future::ready(fail.then(|| bad.clone())))));
            let before = control(&mut h.app, &json!({"action":"quit", "save_indices":[2]}));
            control(&mut h.app, &json!({"save_row":0}));
            crate::file_picker::tick(&mut h.app, &h.ctx);
            crate::save_job::tick(&mut h.app, true);
            let after = control(&mut h.app, &json!({}));
            for key in ["save_prompt", "documents", "active", "closing", "quitting", "batch"] {
                assert_eq!(after[key], before[key], "{key}, failed write: {fail}");
            }
            assert!(!h.app.quit_requested);
        }
    }

    #[test]
    fn grouped_save_selected_writes_only_checked_designs() {
        let mut h = crate::scenario::Harness::new();
        h.app = four_tabs();
        let file = h.dir.join("selected.solidcraft");
        let to = file.to_string_lossy().into_owned();
        h.app.services.pick_save = Some(Box::new(move |_: &str, _: &[&str]| Box::pin(std::future::ready(Some(to.clone())))));
        control(&mut h.app, &json!({"action":"close_all"}));
        control(&mut h.app, &json!({"save_indices":[2], "answer":"save"}));
        assert_eq!(h.app.docs.count(), 4);
        crate::file_picker::tick(&mut h.app, &h.ctx);
        crate::save_job::tick(&mut h.app, true);
        assert!(active_is_blank(&h.app));
        let design = solidcraft_engine::io::read_design(&std::fs::read(file).unwrap()).unwrap();
        assert_eq!(design.features.len(), 1);
        assert_eq!(std::fs::read_dir(&h.dir).unwrap().count(), 1, "only the selected design was saved");
    }

    #[test]
    fn grouped_existing_paths_save_only_selected_and_failure_keeps_tabs() {
        let mut h = crate::scenario::Harness::new();
        h.app = four_tabs();
        let first = h.dir.join("first.solidcraft");
        let second = h.dir.join("second.solidcraft");
        for (i, file) in [(0, &first), (2, &second)] {
            switch(&mut h.app, i);
            h.app.run("file.save_as", json!({"path":file.to_string_lossy()})).unwrap();
            h.app.run("solid.sphere", json!({"diameter":4})).unwrap();
        }
        h.app.services.pick_save = None;
        control(&mut h.app, &json!({"action":"quit"}));
        control(&mut h.app, &json!({"save_indices":[2],"answer":"save"}));
        crate::save_job::tick(&mut h.app, true);
        assert!(h.app.quit_requested);
        for (file, features) in [(&first, 1), (&second, 2)] {
            let design = solidcraft_engine::io::read_design(&std::fs::read(file).unwrap()).unwrap();
            assert_eq!(design.features.len(), features);
        }
        h.app = four_tabs();
        let bad = h.dir.join("missing").join("bad.solidcraft").to_string_lossy().into_owned();
        h.app.services.pick_save = Some(Box::new(move |_: &str, _: &[&str]| Box::pin(std::future::ready(Some(bad.clone())))));
        control(&mut h.app, &json!({"action":"quit"}));
        control(&mut h.app, &json!({"answer":"save"}));
        crate::file_picker::tick(&mut h.app, &h.ctx);
        crate::save_job::tick(&mut h.app, true);
        assert!(!h.app.quit_requested && h.app.docs.quitting.is_none());
        assert_eq!(h.app.docs.count(), 4);
        assert!(dirty(&h.app, 0) && dirty(&h.app, 2));
    }

    #[test]
    fn grouped_later_save_as_cancel_keeps_all_tabs_and_saves_in_order() {
        let mut h = crate::scenario::Harness::new();
        h.app = four_tabs();
        let file = h.dir.join("first.solidcraft");
        let to = file.to_string_lossy().into_owned();
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let picked = calls.clone();
        h.app.services.pick_save = Some(Box::new(move |_: &str, _: &[&str]| {
            let answer = (picked.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0).then(|| to.clone());
            Box::pin(std::future::ready(answer))
        }));
        control(&mut h.app, &json!({"action":"close_all"}));
        control(&mut h.app, &json!({"answer":"save"}));
        assert_eq!(h.app.docs.active, 0);
        crate::file_picker::tick(&mut h.app, &h.ctx);
        crate::save_job::tick(&mut h.app, true);
        assert_eq!(h.app.docs.active, 2, "second unsaved tab is next");
        crate::file_picker::tick(&mut h.app, &h.ctx);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert!(file.exists() && !dirty(&h.app, 0));
        assert!(dirty(&h.app, 2));
        assert_eq!(h.app.docs.count(), 4, "no tabs closed despite the successful first save");
        assert!(h.app.docs.group.is_none() && h.app.docs.batch.is_none() && h.app.docs.closing.is_none());
    }

    #[test]
    fn grouped_prompt_names_select_tabs_and_keyboard_answers() {
        let mut h = crate::scenario::Harness::new();
        h.app = four_tabs();
        h.app.home.open = false;
        h.app.preferences.language = "en".into();
        switch(&mut h.app, 0);
        let file = h.dir.join("Bracket.sc");
        h.app.run("file.save_as", json!({"path":file.to_string_lossy()})).unwrap();
        h.app.run("solid.sphere", json!({"diameter":4})).unwrap();
        switch(&mut h.app, 1);
        h.app.run("solid.box", json!({"length":8,"width":8,"height":8})).unwrap();
        let second_file = h.dir.join("Housing.sc");
        h.app.run("file.save_as", json!({"path":second_file.to_string_lossy()})).unwrap();
        h.app.run("solid.sphere", json!({"diameter":4})).unwrap();
        switch(&mut h.app, 3);
        control(&mut h.app, &json!({"action":"close_all"}));
        // Capture the actual confirmation and its font atlas, without needing a window server.
        h.ctx = crate::fonts::test_context();
        h.ctx.enable_accesskit();
        let mut textures = std::collections::HashMap::new();
        let mut output = egui::FullOutput::default();
        for _ in 0..3 {
            output = h.ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))),
                    focused: true,
                    ..Default::default()
                },
                |ui| {
                    h.app.logic(ui.ctx());
                    h.app.ui(ui);
                },
            );
            ui_kit::textures_update(&mut textures, &output.textures_delta);
            output.textures_delta.clear();
        }
        h.output = output.platform_output.clone();
        assert!(h.widget("Save changes to 3 designs?").is_some());
        assert!(h.widget("Save Selected").is_some());
        assert!(h.widget("Not saved yet").is_some());
        assert!(h.widget(&file.to_string_lossy()).is_some());
        assert!(h.widget(&second_file.to_string_lossy()).is_some());
        let mut previous_path: Option<Rect> = None;
        for index in 0..3 {
            let name = crate::scenario::rect_of(&format!("save_many:name:{index}")).unwrap();
            let path = crate::scenario::rect_of(&format!("save_many:path:{index}")).unwrap();
            assert!((name.left() - path.left()).abs() < 1.0, "path aligns with the name");
            assert!(path.top() >= name.bottom(), "path is below its own name");
            let save = crate::scenario::rect_of(&format!("save_many:save:{index}")).unwrap();
            let discard = crate::scenario::rect_of(&format!("save_many:discard:{index}")).unwrap();
            assert!(save.left() >= name.right(), "row actions follow the name");
            assert!(discard.left() >= save.right(), "Discard follows Save");
            assert!(crate::scenario::rect_of("dialog:window").unwrap().contains_rect(discard), "row actions stay inside the window");
            if let Some(previous) = previous_path {
                assert!(name.top() - previous.bottom() >= Tokens::get().space_8, "designs have breathing room");
            }
            previous_path = Some(path);
        }
        if let Ok(dir) = std::env::var("SOLIDCRAFT_SAVE_MANY_EVIDENCE") {
            let bounds = crate::scenario::rect_of("dialog:window").unwrap().expand(Tokens::get().space_24);
            let image = ui_kit::raster_test_shapes(&h.ctx, output.shapes, output.pixels_per_point, bounds, &textures);
            std::fs::create_dir_all(&dir).unwrap();
            image.save(std::path::Path::new(&dir).join("save-many.png")).unwrap();
        }
        h.step(&json!({"click":{"handle":"save_many:name:0"}})).unwrap();
        assert_eq!(h.app.docs.active, 0);
        h.step(&json!({"click":{"handle":"save_many:tick:0"}})).unwrap();
        assert!(!h.app.docs.group.as_ref().unwrap().rows[0].1);
        h.step(&json!({"key":"Escape"})).unwrap();
        assert_eq!(h.app.docs.count(), 4);
        assert!(h.app.docs.group.is_none());
        control(&mut h.app, &json!({"action":"close_all"}));
        control(&mut h.app, &json!({"save_indices":[]}));
        h.step(&json!({"key":"ArrowLeft"})).unwrap();
        h.step(&json!({"key":"Enter"})).unwrap();
        assert!(active_is_blank(&h.app), "Enter gives Save Selected");
    }

    #[test]
    fn grouped_prompt_twelve_unsaved_designs_stay_on_screen() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        for index in 0..12 {
            if index > 0 {
                new_design(&mut h.app);
            }
            h.app.run("solid.box", json!({"length":1,"width":1,"height":1})).unwrap();
        }
        request_quit(&mut h.app);
        for (width, height) in [(1600, 1000), (1024, 600)] {
            h.call("ui.resize", json!({"width":width,"height":height}));
            h.frames(4);
            let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(width as f32, height as f32));
            let window = crate::scenario::rect_of("dialog:window").unwrap();
            assert!(screen.contains_rect(window), "confirmation stays on screen: {window:?} in {screen:?}");
            let list = crate::scenario::rect_of("save_many:list").unwrap();
            let last = crate::scenario::rect_of("save_many:path:11").unwrap();
            assert!(last.bottom() > list.bottom(), "long lists scroll inside the window");
            assert!(window.contains(crate::scenario::handle_at("dialog:ok").unwrap()), "save remains reachable");
        }
    }

    #[test]
    fn grouped_empty_selection_discards_targets_and_keeps_other_tabs() {
        let mut a = four_tabs();
        close_many(&mut a, vec![0, 2, usize::MAX]);
        let bad = control(&mut a, &json!({"save_indices":[1],"answer":"save"}));
        assert!(bad.get("error").is_some());
        let oversized = control(&mut a, &json!({"save_indices":vec![0; MAX_OPEN + 1],"answer":"save"}));
        assert!(oversized.get("error").is_some());
        assert_eq!(a.docs.count(), 4);
        control(&mut a, &json!({"save_indices":[],"answer":"save"}));
        assert_eq!(a.docs.count(), 2);
        assert!(!any_dirty(&a));
    }

    /// #3055: a right-click on a tab opens its menu; Close Other Tabs from it keeps that tab.
    #[test]
    fn a_tab_right_clicked_offers_close_others() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.custom_titlebar = true;
        h.call("ui.resize", json!({"width": 1400, "height": 720}));
        open_tab(&mut h.app, Session::default()).unwrap();
        open_tab(&mut h.app, Session::default()).unwrap();
        assert_eq!(h.app.docs.count(), 3);
        h.frames(3);
        h.step(&json!({"click": {"handle": "tab:design:1"}, "button": "right"})).unwrap();
        h.frames(2);
        h.step(&json!({"click": {"handle": "tab_menu:ui.closeOtherTabs"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.docs.count(), 1);
    }

    /// A middle click on a tab closes it as its × does: at once when it has nothing to save,
    /// after asking when it has.
    #[test]
    fn a_tab_middle_clicked_closes() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.custom_titlebar = true;
        h.call("ui.resize", json!({"width": 1400, "height": 720}));
        open_tab(&mut h.app, Session::default()).unwrap();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        open_tab(&mut h.app, Session::default()).unwrap();
        assert_eq!(h.app.docs.count(), 3);
        h.frames(3);
        // The second tab holds unsaved changes: closing it asks first
        h.step(&json!({"click": {"handle": "tab:design:1"}, "button": "middle"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.docs.count(), 3);
        assert!(h.app.docs.closing.is_some(), "a design with changes asks before it closes");
        h.call("ui.documents", json!({"answer": "dont_save"}));
        h.frames(2);
        assert_eq!(h.app.docs.count(), 2);
        // The first has nothing to save: it closes at once
        h.step(&json!({"click": {"handle": "tab:design:0"}, "button": "middle"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.docs.count(), 1);
        // A middle click on the ×, which lies over the tab and takes its clicks, closes it too
        open_tab(&mut h.app, Session::default()).unwrap();
        h.frames(3);
        h.step(&json!({"click": {"handle": "tab:close:0"}, "button": "middle"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.docs.count(), 1);
    }

    /// #3187: a tab moved to another place keeps its design, and the active design stays
    /// active wherever it ends up.
    #[test]
    fn a_moved_tab_keeps_its_design_and_the_active_one_stays_active() {
        let mut a = app();
        a.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        for k in 1..3 {
            open_tab(&mut a, Session::default()).unwrap();
            for _ in 0..=k {
                a.run("solid.sphere", json!({"diameter": 4})).unwrap();
            }
        }
        // Tabs hold 1, 2 and 3 features; the third is active.
        let features = |a: &mut SolveApp| {
            let back = a.docs.active;
            let v: Vec<usize> = (0..a.docs.count())
                .map(|i| {
                    switch(a, i);
                    a.session.doc.features.len()
                })
                .collect();
            switch(a, back);
            v
        };
        assert_eq!(features(&mut a), [1, 2, 3]);
        assert_eq!(a.docs.active, 2);
        move_tab(&mut a, 0, 2);
        assert_eq!(features(&mut a), [2, 3, 1]);
        assert_eq!(a.docs.active, 1, "the active design moved over with the others");
        assert_eq!(a.session.doc.features.len(), 3);
        move_tab(&mut a, 1, 0);
        assert_eq!(features(&mut a), [3, 2, 1]);
        assert_eq!(a.docs.active, 0);
        move_tab(&mut a, 0, 9);
        assert_eq!(features(&mut a), [3, 2, 1], "out of range: nothing moves");
        assert_eq!(moved_index(5, 5, 1), 1);
        assert_eq!(drop_slot(0.0, 100.0, &[90.0; 4], 2, 2), 2);
        assert_eq!(drop_slot(240.0, 100.0, &[90.0; 4], 2, 2), 4);
        assert_eq!(drop_slot(9000.0, 100.0, &[90.0; 4], 2, 2), 5);
        // Tabs fit their names: the slots follow the other tabs' own widths.
        assert_eq!(drop_slot(90.0, 0.0, &[80.0, 200.0, 80.0], 0, 0), 0);
        assert_eq!(drop_slot(150.0, 0.0, &[80.0, 200.0, 80.0], 0, 0), 1);
        assert_eq!(drop_slot(260.0, 0.0, &[80.0, 200.0, 80.0], 0, 0), 2);
    }

    /// #8565: Ctrl+Tab shows the tab to the right and Ctrl+Shift+Tab the one to the left, going
    /// round at either end; both are commands with their keys on the Keyboard Shortcuts page, and
    /// `engine.execute` switches the same way. A "save changes?" prompt keeps the tab shown.
    #[test]
    fn ctrl_tab_and_ctrl_shift_tab_go_round_the_design_tabs() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        for k in 1..3 {
            open_tab(&mut h.app, Session::default()).unwrap();
            for _ in 0..k {
                h.app.run("solid.sphere", json!({"diameter": 4})).unwrap();
            }
        }
        h.frames(2);
        assert_eq!((h.app.docs.active, h.app.session.doc.features.len()), (2, 2));
        let press = |h: &mut crate::scenario::Harness, shift: bool| {
            h.call("ui.key", json!({"key": "Tab", "cmd": true, "shift": shift}));
            h.frames(2);
            (h.app.docs.active, h.app.session.doc.features.len())
        };
        assert_eq!(press(&mut h, false), (0, 0), "after the last tab, the first");
        assert_eq!(press(&mut h, false), (1, 1));
        assert_eq!(press(&mut h, true), (0, 0));
        assert_eq!(press(&mut h, true), (2, 2), "before the first tab, the last");
        assert_eq!(crate::keymap::effective(&h.app, "view.next_tab").as_deref(), Some("Ctrl+Tab"));
        assert_eq!(crate::keymap::effective(&h.app, "view.previous_tab").as_deref(), Some("Ctrl+Shift+Tab"));
        assert_eq!(h.call("engine.execute", json!({"command": "view.next_tab"}))["ok"], true);
        assert_eq!(h.app.docs.active, 0);
        assert_eq!(h.call("engine.execute", json!({"command": "view.previous_tab"}))["ok"], true);
        assert_eq!(h.app.docs.active, 2);
        control(&mut h.app, &json!({"action": "close"}));
        assert_eq!(h.app.docs.closing, Some(2), "the unsaved design asks first");
        assert_eq!(press(&mut h, false).0, 2, "the asking tab stays shown");
        control(&mut h.app, &json!({"answer": "cancel"}));
        let mut one = app();
        cycle(&mut one, true);
        assert_eq!((one.docs.active, one.docs.count()), (0, 1), "a single design: nothing to go to");
    }

    /// #3187: dragging a tab along the strip drops it where it is released; a drag mostly down
    /// the window (it moves the window) or one Escape cancelled leaves the order as it was.
    #[test]
    fn dragging_a_tab_along_the_strip_moves_it() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.custom_titlebar = true;
        h.call("ui.resize", json!({"width": 1400, "height": 720}));
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        open_tab(&mut h.app, Session::default()).unwrap();
        open_tab(&mut h.app, Session::default()).unwrap();
        switch(&mut h.app, 0);
        h.frames(3);
        let at = |h: &mut crate::scenario::Harness, i: usize| {
            let [x, y] = h.at(&json!({"handle": format!("tab:design:{i}")})).unwrap();
            egui::pos2(x as f32, y as f32)
        };
        let button =
            |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        let path = |a: egui::Pos2, b: egui::Pos2| (1..=8).map(move |k| egui::Event::PointerMoved(a + (b - a) * (k as f32 / 8.0)));
        let (t0, t2) = (at(&mut h, 0), at(&mut h, 2));
        // Down first: that moves the window, and coming back along the bar moves no tab.
        let down = t0 + vec2(0.0, 20.0);
        h.app.synthetic.extend([egui::Event::PointerMoved(t0), button(t0, true)]);
        h.app.synthetic.extend(path(t0, down).chain(path(down, t2)));
        h.app.synthetic.push(button(t2, false));
        h.frames(24);
        assert_eq!(h.app.docs.active, 0, "a drag down the window leaves the tabs");
        assert_eq!(h.app.session.doc.features.len(), 1);
        h.app.synthetic.extend([egui::Event::PointerMoved(t0), button(t0, true)]);
        h.app.synthetic.extend(path(t0, t2));
        h.app.synthetic.push(button(t2, false));
        h.frames(16);
        assert_eq!(h.app.docs.active, 2, "the dragged tab is now last");
        assert_eq!(h.app.session.doc.features.len(), 1, "and still shows its design");
        let (t2, t1) = (at(&mut h, 2), at(&mut h, 1));
        let escape = egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
        h.app.synthetic.extend([egui::Event::PointerMoved(t2), button(t2, true)]);
        h.app.synthetic.extend(path(t2, t1));
        h.app.synthetic.extend([escape, egui::Event::PointerMoved(t1), button(t1, false)]);
        h.frames(16);
        assert_eq!(h.app.docs.active, 2, "Escape put it back");
    }

    /// #5366: Ctrl+W closes the design shown: a saved one at once (the last one leaves the start
    /// page), an unsaved one after "save changes?", and on the start page it does nothing (the
    /// browser closes its tab there).
    #[test]
    fn ctrl_w_closes_the_design_shown() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        new_design(&mut h.app);
        h.app.run("solid.sphere", json!({"diameter": 8})).unwrap();
        h.app.session.mark_saved();
        assert_eq!(h.app.docs.count(), 2);
        h.frames(2);
        h.step(&json!({"key": "W", "cmd": true})).unwrap();
        h.frames(2);
        assert_eq!((h.app.docs.count(), h.app.docs.closing), (1, None), "the saved design closed without asking");
        assert!(!h.app.home.open, "the other design is shown");

        h.step(&json!({"key": "W", "cmd": true})).unwrap();
        h.frames(2);
        assert_eq!((h.app.docs.count(), h.app.docs.closing), (1, Some(0)), "the unsaved one asks first");
        answer_prompt(&mut h.app, Answer::DontSave);
        assert!(active_is_blank(&h.app) && h.app.home.open, "the last one closed: the start page");

        let revision = h.app.session.revision;
        h.step(&json!({"key": "W", "cmd": true})).unwrap();
        h.frames(2);
        assert_eq!((h.app.docs.closing, h.app.session.revision), (None, revision), "the start page has nothing to close");
        assert!(h.app.home.open);
    }

    #[test]
    fn tabs_keep_their_own_design() {
        let mut a = app();
        a.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        a.session.selection = vec![solidcraft_engine::Sel::Body { name: "Body1".into() }];
        a.cam.distance = 123.0;
        new_design(&mut a);
        assert_eq!(a.docs.count(), 2);
        assert_eq!(a.docs.active, 1);
        assert!(a.session.doc.features.is_empty() && a.session.selection.is_empty());
        a.run("solid.sphere", json!({"diameter": 8})).unwrap();
        a.run("solid.sphere", json!({"diameter": 4})).unwrap();
        switch(&mut a, 0);
        assert_eq!(a.session.doc.features.len(), 1);
        assert_eq!(a.session.selection.len(), 1);
        assert!((a.cam.distance - 123.0).abs() < 1e-9);
        a.run("edit.undo", json!({})).unwrap();
        assert!(a.session.doc.features.is_empty(), "undo is this design's");
        switch(&mut a, 1);
        assert_eq!(a.session.doc.features.len(), 2, "the other design kept its features");
        // Unsaved: closing asks, Don't Save closes.
        close(&mut a, 1, false);
        assert_eq!(a.docs.closing, Some(1));
        assert_eq!(a.docs.count(), 2);
        close(&mut a, 1, true);
        assert_eq!(a.docs.count(), 1);
        assert_eq!(a.docs.active, 0);
        close(&mut a, 0, true);
        assert!(active_is_blank(&a) && a.home.open);
    }

    /// Direct tab creation remains available, including reuse of an untouched launch design.
    #[test]
    fn direct_new_tab_reuses_the_launch_design_then_opens_a_tab_every_time() {
        let mut a = app();
        a.home.open = true;
        new_tab(&mut a);
        assert_eq!(a.docs.count(), 1, "the launch's blank design is used");
        assert!(!a.home.open);
        new_tab(&mut a);
        new_tab(&mut a);
        assert_eq!(a.docs.count(), 3);
        assert_eq!(a.docs.active, 2);
    }

    /// Quitting asks once per design with unsaved changes: Don't Save moves on (and keeps no
    /// recovery copy), Save saves and moves on, and the app quits after the last answer.
    #[test]
    fn quit_asks_about_each_unsaved_design_then_quits() {
        let mut a = app();
        request_quit(&mut a);
        assert!(a.quit_requested, "nothing unsaved: quits at once");

        let mut a = app();
        a.preferences.save_prompt_one_at_a_time = true;
        let file = std::env::temp_dir().join(format!("solidcraft-quit-save-{}.solidcraft", std::process::id()));
        let _ = std::fs::remove_file(&file);
        a.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        a.run("file.save_as", json!({"path": file.to_string_lossy()})).unwrap();
        a.run("solid.sphere", json!({"diameter": 4})).unwrap();
        new_design(&mut a);
        a.run("solid.sphere", json!({"diameter": 8})).unwrap();
        assert!(dirty(&a, 0) && dirty(&a, 1));

        request_quit(&mut a);
        assert!(!a.quit_requested);
        assert_eq!(a.docs.closing, Some(0), "the first unsaved design is asked about first");
        assert_eq!(a.docs.active, 0, "and shown");
        answer_prompt(&mut a, Answer::Save);
        crate::save_job::tick(&mut a, true);
        assert!(!dirty(&a, 0), "saved");
        assert_eq!(
            std::fs::read(&file).ok().and_then(|b| solidcraft_engine::io::read_design(&b).ok()).map(|d| d.features.len()),
            Some(2),
            "the file holds the change"
        );
        assert_eq!(a.docs.closing, Some(1), "then the next one");
        assert!(!a.quit_requested);
        answer_prompt(&mut a, Answer::DontSave);
        assert_eq!(a.docs.closing, None);
        assert!(a.quit_requested, "every design answered: quit");
        assert!(!any_dirty(&a), "Don't Save discards: nothing is kept for recovery");
        // The next launch reopens the saved design; the never-saved one has no file (#5542).
        let path = file.to_string_lossy().to_string();
        assert_eq!(a.preferences.open_designs, vec![path.clone()]);
        assert_eq!(designs_to_reopen(&a), vec![path]);
        a.preferences.reopen_designs = false;
        assert!(designs_to_reopen(&a).is_empty(), "turned off in Preferences");
        a.preferences.reopen_designs = true;
        let _ = std::fs::remove_file(&file);
        assert!(designs_to_reopen(&a).is_empty(), "a file that is gone is skipped");
    }

    /// #5064: a design that kills the app while it is being reopened is skipped once, with the
    /// reason said, and the launch after that reopens normally again.
    #[test]
    fn reopen_plan_skips_what_killed_the_last_session() {
        let dir = std::env::temp_dir().join(format!("solidcraft-reopen-plan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("part.solidcraft");
        std::fs::write(&file, []).unwrap();
        let path = file.to_string_lossy().to_string();
        let mut a = app();
        a.preferences.open_designs = vec![path.clone()];

        // A normal launch: it reopens the design and marks that it is doing so.
        assert_eq!(reopen_plan(&a, Some(&dir)), (vec![path.clone()], Vec::new()));
        // The session dies while opening it: the marker survives, so the next launch skips it
        // and says what it skipped.
        assert_eq!(
            reopen_plan(&a, Some(&dir)),
            (Vec::new(), vec![path.clone()]),
            "the design that killed the last session is skipped, with the reason"
        );
        // The skip cleared the marker: the launch after it works normally again.
        assert_eq!(reopen_plan(&a, Some(&dir)), (vec![path.clone()], Vec::new()));
        // No recovery folder on this system: the guard is off, the plain list is used.
        assert_eq!(reopen_plan(&a, None).0, vec![path]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Cancel, Esc's answer, or a Save that saves nothing (a never-saved design whose Save As
    /// picker is cancelled) stay open with the changes.
    #[test]
    fn quit_cancelled_or_unsaved_stays_open() {
        let mut a = app();
        a.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        request_quit(&mut a);
        assert_eq!(a.docs.closing, Some(0));
        answer_prompt(&mut a, Answer::Cancel);
        assert!(!a.quit_requested && a.docs.closing.is_none() && a.docs.quitting.is_none());
        assert!(a.session.is_dirty(), "the changes are still there");

        // No Save As picker answer (the default services have none): not saved, so no quit.
        request_quit(&mut a);
        answer_prompt(&mut a, Answer::Save);
        assert!(!a.quit_requested && a.docs.quitting.is_none() && a.session.is_dirty());

        // A picked file: saved, then quit.
        let file = std::env::temp_dir().join(format!("solidcraft-quit-saveas-{}.solidcraft", std::process::id()));
        let _ = std::fs::remove_file(&file);
        let to = file.to_string_lossy().to_string();
        a.services.pick_save = Some(Box::new(move |_: &str, _: &[&str]| Box::pin(std::future::ready(Some(to.clone())))));
        request_quit(&mut a);
        answer_prompt(&mut a, Answer::Save);
        assert!(!a.quit_requested && a.file_picker_busy());
        crate::file_picker::tick(&mut a, &crate::fonts::test_context());
        crate::save_job::tick(&mut a, true);
        assert!(a.quit_requested && file.exists());
        let _ = std::fs::remove_file(&file);
    }

    /// #680: "Project face edges into new sketches" is a preference: it holds in new tabs, in
    /// the tab switched to, in a design adopted over a blank one and after the last tab closes,
    /// and it is what the preferences file keeps.
    #[test]
    fn auto_project_follows_the_user_across_tabs() {
        let mut a = app();
        a.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        a.run("sketch.auto_project", json!({"value": false})).unwrap();
        new_design(&mut a);
        assert_eq!(a.docs.count(), 2);
        assert!(!a.session.auto_project, "a new tab");
        a.run("solid.box", json!({"length": 5, "width": 5, "height": 5})).unwrap();
        a.run("sketch.auto_project", json!({"value": true})).unwrap();
        switch(&mut a, 0);
        assert!(a.session.auto_project, "the tab switched to");
        a.run("sketch.auto_project", json!({"value": false})).unwrap();
        switch(&mut a, 1);
        assert!(!a.session.auto_project);
        let prefs: Value = serde_json::from_str(&a.prefs()).unwrap();
        assert_eq!(prefs["auto_project"], false);
        close(&mut a, 1, true);
        close(&mut a, 0, true);
        assert!(active_is_blank(&a) && !a.session.auto_project, "after the last tab closes");
        adopt(&mut a, Session::default());
        assert_eq!(a.docs.count(), 1);
        assert!(!a.session.auto_project, "a design adopted over a blank one");
    }

    /// Projecting references as construction geometry is the user's too (#13142), like
    /// auto-projection: it holds in new tabs, the tab switched to, after the last tab closes and
    /// in a design adopted over a blank one, and the preferences file keeps it.
    #[test]
    fn project_construction_follows_the_user_across_tabs() {
        let mut a = app();
        a.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        a.session.project_construction = true;
        new_design(&mut a);
        assert!(a.session.project_construction, "a new tab");
        a.run("solid.box", json!({"length": 5, "width": 5, "height": 5})).unwrap();
        switch(&mut a, 0);
        assert!(a.session.project_construction, "the tab switched to");
        let prefs: Value = serde_json::from_str(&a.prefs()).unwrap();
        assert_eq!(prefs["project_construction"], true);
        close(&mut a, 1, true);
        close(&mut a, 0, true);
        assert!(active_is_blank(&a) && a.session.project_construction, "after the last tab closes");
        adopt(&mut a, Session::default());
        assert!(a.session.project_construction, "a design adopted over a blank one");
    }

    /// The host's pre-command check holds in every tab: new ones, the one switched to, a design
    /// adopted over a blank one and after the last tab closes.
    #[test]
    fn the_command_check_follows_the_host_across_tabs() {
        let mut a = app();
        a.session.set_command_check(Some(std::sync::Arc::new(
            |id: &str, _: &Value, _: &Session| {
                if id == "solid.box" { Err("no boxes".to_string()) } else { Ok(()) }
            },
        )));
        assert_eq!(a.run("solid.box", json!({"length": 10, "width": 10, "height": 10})), Err("no boxes".to_string()));
        a.run("solid.cylinder", json!({"radius": 5, "height": 10})).unwrap();
        new_design(&mut a);
        assert_eq!(a.docs.count(), 2);
        assert!(a.run("solid.box", json!({"length": 5, "width": 5, "height": 5})).is_err(), "a new tab");
        switch(&mut a, 0);
        assert!(a.run("solid.box", json!({"length": 5, "width": 5, "height": 5})).is_err(), "the tab switched to");
        close(&mut a, 1, true);
        close(&mut a, 0, true);
        assert!(a.run("solid.box", json!({"length": 5, "width": 5, "height": 5})).is_err(), "after the last tab closes");
        adopt(&mut a, Session::default());
        assert!(a.run("solid.box", json!({"length": 5, "width": 5, "height": 5})).is_err(), "a design adopted over a blank one");
    }

    /// Hide Origin by Default hides the Origin of designs that don't keep its eye; a design that
    /// does keeps it, along with the rest of the browser's view, in its tab and in its file.
    #[test]
    fn the_browsers_view_follows_each_design_and_the_origin_preference() {
        let mut a = app();
        assert!(a.origin_shown());
        a.preferences.hide_origin = true;
        assert!(!a.origin_shown() && !a.session.is_dirty(), "the preference is not a change to the design");
        let mut b = app();
        b.load_prefs(&a.prefs());
        assert!(b.preferences.hide_origin && !b.origin_shown(), "the preference is kept between runs");
        // This design shows its Origin and locks its body and hides its Sketches folder.
        a.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        a.set_origin_shown(true);
        a.session.visibility.locked_bodies.push("Body1".into());
        a.session.visibility.hidden_folders.push("c0/sketches".into());
        new_design(&mut a);
        assert!(!a.origin_shown() && a.session.visibility.is_empty(), "a new design follows the preference");
        switch(&mut a, 0);
        assert!(a.origin_shown() && a.session.visibility.locked_bodies == ["Body1"] && a.session.folder_hidden("c0/sketches"));
        // Opened again, the design's own Origin setting wins over the preference.
        let text = a.session.design().to_json();
        let mut c = app();
        c.preferences.hide_origin = true;
        c.session = Session::new(solidcraft_engine::doc::Document::from_json(&text).unwrap());
        assert!(c.origin_shown() && c.session.visibility == a.session.visibility);
    }

    /// #1205: Units of new designs apply to the empty design a launch shows, to New while that
    /// design is still empty, to `file.new`, and to the blank design left when the last tab closes.
    /// A design that already has features keeps its units.
    #[test]
    fn new_designs_use_the_preferred_units_including_a_blank_one() {
        let mut a = app();
        a.preferences.default_units = "in".into();
        assert_eq!(a.session.doc.units, "mm");
        // What a launch with no file does after reading preferences.
        crate::prefs::apply_new_design(&mut a);
        assert_eq!(a.session.doc.units, "in");
        assert!(!a.session.is_dirty());
        assert!(active_is_blank(&a));
        let tabs = a.docs.count();
        new_design(&mut a);
        assert_eq!(a.docs.count(), tabs, "New on an empty design does not open another tab");
        assert_eq!(a.session.doc.units, "in");
        assert!(!a.session.is_dirty());
        // Geometry already there is left alone. New then opens a tab in the preferred units.
        a.run("solid.box", json!({"length": 25.4, "width": 25.4, "height": 25.4})).unwrap();
        assert_eq!(a.session.doc.units, "in");
        a.preferences.default_units = "ft".into();
        new_design(&mut a);
        assert_eq!(a.docs.count(), tabs + 1);
        assert_eq!(a.session.doc.units, "ft");
        assert!(a.session.doc.features.is_empty());
        assert!(!a.session.is_dirty());
        a.run("file.new", json!({"name": "From command"})).unwrap();
        assert_eq!(a.session.doc.units, "ft");
        assert_eq!(a.session.doc.name, "From command");
        assert!(!a.session.is_dirty());
        // Closing every tab leaves a blank design in the preferred units.
        assert_eq!(a.docs.count(), 2);
        let active = a.docs.active;
        close(&mut a, active, true);
        assert_eq!(a.docs.count(), 1);
        assert!(!a.session.doc.features.is_empty(), "the other tab still has its part");
        close(&mut a, 0, true);
        assert!(active_is_blank(&a));
        assert_eq!(a.session.doc.units, "ft");
        assert!(!a.session.is_dirty());
    }

    /// A design built elsewhere replaces a blank one and gets a tab otherwise.
    #[test]
    fn adopt_reuses_a_blank_design() {
        let mut a = app();
        let mut s = Session::default();
        s.run_script(&solidcraft_engine::sample::script()).unwrap();
        adopt(&mut a, s);
        assert_eq!(a.docs.count(), 1);
        assert!(!a.session.doc.features.is_empty());
        adopt(&mut a, Session::default());
        assert_eq!(a.docs.count(), 2);
        let v = control(&mut a, &json!({"action": "switch", "index": 0}));
        assert_eq!(v["active"], 0);
        assert_eq!(v["documents"].as_array().map(Vec::len), Some(2));
    }

    /// The items picked in the open dialog's first input.
    fn dialog_picks(a: &SolveApp) -> Vec<solidcraft_engine::Sel> {
        a.dialog.as_ref().and_then(|d| d.inputs.first()).map(|i| i.items.clone()).unwrap_or_default()
    }

    /// Tab 1: a box measured between two corners, on INSPECT. Tab 2: a sketch with the Line tool.
    fn measuring_and_drawing(keep: bool) -> (SolveApp, Vec<solidcraft_engine::Sel>) {
        let mut a = app();
        a.preferences.keep_work_on_tab_switch = keep;
        a.run("solid.box", json!({"length": 3, "width": 4, "height": 12, "body_name": "Box"})).unwrap();
        a.session.selection =
            [[0.0, 0.0, 0.0], [3.0, 4.0, 12.0]].into_iter().map(|p| solidcraft_engine::Sel::Vertex { body: "Box".into(), point: p.into() }).collect();
        a.ui.tab = "INSPECT".into();
        a.start("inspect.measure");
        let picks = dialog_picks(&a);
        assert_eq!(picks.len(), 2, "Measure opens on the two picked corners");
        new_design(&mut a);
        a.run("sketch.create", json!({"plane": "XY"})).unwrap();
        crate::workspace::sync_context(&mut a);
        a.start("sketch.line");
        assert!(a.tool.as_ref().is_some_and(|t| t.cmd == "sketch.line") && a.ui.tab == "SKETCH");
        (a, picks)
    }

    /// #9360: with "Keep dialogs and tools open when switching designs", each design tab has its
    /// open dialog (with its picks), its tool and its toolbar tab back when it is shown again, and
    /// none of them shows in the other design.
    #[test]
    fn tabs_keep_their_dialog_tool_and_toolbar_tab_with_the_preference() {
        let (mut a, picks) = measuring_and_drawing(true);
        switch(&mut a, 0);
        assert!(a.tool.is_none(), "the Line tool stays with the sketch's design");
        assert!(matches!(a.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::Measure { .. })), "Measure is open again");
        assert_eq!(dialog_picks(&a), picks, "with the same two corners");
        assert_eq!(a.ui.tab, "INSPECT");
        assert!(a.session.active_sketch.is_none());
        switch(&mut a, 1);
        assert!(a.dialog.is_none(), "Measure stays with the box's design");
        assert!(a.tool.as_ref().is_some_and(|t| t.cmd == "sketch.line"), "the Line tool is back");
        assert!(a.session.active_sketch.is_some());
        assert_eq!(a.ui.tab, "SKETCH");
        // Kept work comes back even once the preference is off; after that, a switch cancels.
        a.preferences.keep_work_on_tab_switch = false;
        switch(&mut a, 0);
        assert!(dialog_picks(&a) == picks && a.ui.tab == "INSPECT");
        switch(&mut a, 1);
        switch(&mut a, 0);
        assert!(a.dialog.is_none() && a.ui.tab == "MODEL");
    }

    /// Without the preference (the default), a switch cancels the dialog and the tool and shows
    /// MODEL, as before #9360.
    #[test]
    fn a_tab_switch_cancels_the_dialog_and_tool_by_default() {
        assert!(!crate::prefs::Prefs::default().keep_work_on_tab_switch);
        let (mut a, _) = measuring_and_drawing(false);
        switch(&mut a, 0);
        assert!(a.dialog.is_none() && a.tool.is_none());
        assert_eq!(a.ui.tab, "MODEL");
        switch(&mut a, 1);
        assert!(a.dialog.is_none() && a.tool.is_none());
        // The sketch itself stays open in its design, so its tab is SKETCH.
        assert_eq!(a.ui.tab, "SKETCH");
    }

    /// #9360: a feature being edited keeps its history rolled back to it in its own design only,
    /// and its Cancel, after the switch back, puts that design's history back.
    #[test]
    fn a_feature_edit_waits_in_its_tab_and_cancels_there() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.preferences.keep_work_on_tab_switch = true;
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        h.app.run("solid.box", json!({"length": 5, "width": 5, "height": 30})).unwrap();
        let features = h.app.session.doc.features.len();
        let first = h.app.session.doc.features.first().map(|f| f.id).unwrap();
        h.call("ui.editFeature", json!({"feature": first}));
        h.frames(3);
        let editing = |h: &crate::scenario::Harness| h.app.dialog.as_ref().and_then(|d| d.editing).map(|(id, _)| id);
        assert_eq!(editing(&h), Some(first), "the first box's dialog is open");
        let marker = h.app.session.doc.marker;
        new_design(&mut h.app);
        h.frames(2);
        assert!(h.app.dialog.is_none() && h.app.editing_state.is_none());
        assert!(h.app.session.doc.features.is_empty() && h.app.session.doc.marker.is_none(), "the new design is untouched");
        switch(&mut h.app, 0);
        h.frames(2);
        assert_eq!(editing(&h), Some(first), "the edit is open again");
        assert_eq!(h.app.session.doc.marker, marker, "with the history where the edit left it");
        crate::dialogs::cancel(&mut h.app);
        assert!(h.app.dialog.is_none());
        assert_eq!((h.app.session.doc.features.len(), h.app.session.doc.marker), (features, None), "Cancel puts the history back");
    }
}
