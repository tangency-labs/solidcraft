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
use crate::design_tab_prefs::DesignTabPrefs;
use crate::ui_kit;
use solidcraft_views::{DesignView, ViewKind, ViewSet};

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
    /// The views of the design (its sub-tabs), kept while another design is shown.
    views: ViewSet,
    /// What each of its views shows besides the camera ([`ViewInspect`]), kept while another design is shown.
    inspect: ViewInspect,
    navigation: ViewNavigation,
    /// Opened behind the shown design: framed when it is first shown.
    fit: bool,
    /// Restored behind the shown design: apply the remembered workspace and camera once.
    restore_views: bool,
}

/// What a design was doing when its tab was left, with the "Keep dialogs and tools open when
/// switching designs" preference (`keep_work_on_tab_switch`, #9360): the open dialog with its
/// picks and values, the tool, and the toolbar tab. They act on that design's session, which
/// waits in the same slot, so showing the tab again picks up exactly where it was left.
#[derive(Default)]
struct DocWork {
    /// Shared panel content belongs to the design, independently of its current placement.
    bom: Option<crate::bom_panel::BomPanel>,
    params: Option<crate::params_dialog::ParamsDialog>,
    design_properties: Option<crate::design_properties::DesignProperties>,
    dialog: Option<crate::dialogs::Dialog>,
    /// A New Sketch of that dialog (#6383): its checkpoint is in this design's session.
    inline_sketch: Option<crate::command_sketch::InlineSketch>,
    editing_state: Option<crate::dialogs::EditingState>,
    tool: Option<crate::tools::Tool>,
    /// The toolbar tab shown, its contextual tab and the tab a sketch returns to (`None`: the
    /// design was left with both preferences off, or is new, and comes back on MODEL).
    tab: Option<(String, Option<String>, Option<String>)>,
}

/// Take the active design's dialog, tool and toolbar tab, to keep with its tab. A feature still
/// computing after OK can't wait (its result belongs to the window thread): that dialog is
/// cancelled, as without the preference. Without the preference the toolbar tab is still kept
/// (`remember_toolbar_tab`), once the dialog and tool are gone, so it is a tab they leave behind.
fn take_work(app: &mut SolveApp, presenting: bool) -> DocWork {
    let bom = app.bom.take();
    if !presenting && (!app.preferences.keep_work_on_tab_switch || app.applying.is_some()) {
        crate::dialogs::cancel(app);
        // A sketch still open for a New Sketch stays, as any open sketch does.
        crate::command_sketch::forget(app);
        app.tool = None;
        crate::drawing_workspace::close_panel(app);
        let tab = app.preferences.remember_toolbar_tab.then(|| (app.ui.tab.clone(), app.ui.ribbon_context.clone(), app.ui.sketch_return_tab.clone()));
        return DocWork { bom, tab, ..DocWork::default() };
    }
    DocWork {
        bom,
        params: presenting.then(|| app.params.take()).flatten(),
        design_properties: presenting.then(|| app.design_properties.take()).flatten(),
        dialog: app.dialog.take(),
        inline_sketch: app.inline_sketch.take(),
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
    app.params = work.params;
    app.design_properties = work.design_properties;
    crate::bom_panel::set_focus_pending(app, true);
    // That refresh is not a change to the design: the edit's Cancel must still see the design
    // it left (`Session::restore_editor` compares revisions), as `PendingFeatureEdit` does.
    if let Some(state) = &mut work.editing_state {
        state.source.revision = state.source.revision.saturating_add(app.session.revision.saturating_sub(before));
    }
    app.dialog = work.dialog;
    app.inline_sketch = work.inline_sketch;
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
    crate::drawing_workspace::swap_selection(app);
    crate::workspace::sync_history(app);
}

/// The per-design part of the UI state the session doesn't keep (what is hidden, shown or locked
/// is in the session's `visibility`, saved with the design).
#[derive(Default)]
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
    /// Its Drawing workspace: the sheet page (sheet, zoom, camera) and the panels open on it.
    drawing: crate::drawing_workspace::State,
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
            reveals: Default::default(),
            agent_chat: Default::default(),
            link_stamps: Vec::new(),
            edit_in_place: None,
            work: DocWork::default(),
            jobs: Default::default(),
            tab: next_tab(),
            views: ViewSet::new(),
            inspect: ViewInspect::new(),
            navigation: ViewNavigation::new(),
            fit: false,
            restore_views: false,
        }
    }
}

/// A new tab's number ([`DocSlot::tab`], [`Documents::active_tab`]).
pub(crate) fn next_tab() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// What a place of a layout shows when the design has no view for it yet (the Layout menu's
/// "Empty cells show"). A choice for this session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LayoutFill {
    /// A new angle of the shown view: a copy of it.
    #[default]
    Angle,
    /// The standard views in turn: front, right, top.
    Standard,
    /// An empty cell that asks what to show.
    Picker,
}

impl LayoutFill {
    pub const ALL: [LayoutFill; 3] = [LayoutFill::Angle, LayoutFill::Standard, LayoutFill::Picker];

    /// The key the Layout menu's row has.
    pub fn key(self) -> &'static str {
        match self {
            LayoutFill::Angle => "fill.angle",
            LayoutFill::Standard => "fill.standard",
            LayoutFill::Picker => "fill.picker",
        }
    }
}

/// The open designs: one slot per tab; the active tab's slot is empty (its design is in the app).
#[derive(Default)]
pub struct Documents {
    /// What the places of a layout without a view show ([`LayoutFill`]).
    pub(crate) layout_fill: LayoutFill,
    slots: Vec<Option<DocSlot>>,
    pub active: usize,
    /// A tab waiting for the answer to "save changes?".
    pub closing: Option<usize>,
    /// File > Revert to Saved is asking whether to drop the active design's unsaved changes.
    pub reverting: bool,
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
    /// The design being shown was opened through the control channel or by the assistant: it
    /// never asks whether to load linked files from other folders (`link_folders_prompt`).
    pub(crate) quiet_open: bool,
    /// The shown tab's number ([`DocSlot::tab`]); 0 until it is first asked for ([`active_tab`]).
    active_tab: u64,
    /// The shown design's views ([`DocSlot::views`] for the others); see [`apply_view`].
    pub(crate) views: ViewSet,
    /// What each of the shown design's views shows besides the camera ([`DocSlot::inspect`] for the others).
    pub(crate) inspect: ViewInspect,
    navigation: ViewNavigation,
    /// Recalled framing waits for selection, across passive native-window swaps.
    restore_views: bool,
    /// A view's name being typed in its sub-tab ([`view_rename_start`]).
    pub(crate) view_rename: Option<ViewRename>,
    /// Click placement in progress (design tab id, view id).
    pub(crate) view_placement: Option<(u64, u64)>,
    /// The window's tab groups, by tab number ([`crate::tab_groups`]).
    pub(crate) groups: crate::tab_groups::TabGroups,
    /// A group's name being typed in its label.
    pub(crate) group_rename: Option<crate::tab_group_ui::GroupRename>,
    /// The active tab's own parked conversation while `agent_shared_session` is enabled, so
    /// switching tabs never hands one tab's parked chat to another tab's slot. Boxed so
    /// `Documents` / `SolveApp` does not double its inline stack footprint with a second `AgentChat`.
    parked_agent_chat: Option<Box<crate::agent_chat::AgentChat>>,
}

/// A view of the shown design whose name is being typed: the view, the text and whether the
/// field has taken the focus.
pub(crate) struct ViewRename {
    view: u64,
    text: String,
    focused: bool,
    /// The design it was begun in (see [`give_up_renames_of_other_designs`]).
    session: u64,
}

/// Give up a rename begun in another design than the one shown (another tab, a design closed,
/// opened, started or imported): a refused name stays in its field only while its design is
/// shown, and the same id in another design names something else.
pub(crate) fn give_up_renames_of_other_designs(app: &mut SolveApp) {
    let shown = app.session.uid;
    if app.menu.rename.as_ref().is_some_and(|r| r.session != shown) {
        app.menu.rename = None;
    }
    if app.docs.view_rename.as_ref().is_some_and(|r| r.session != shown) {
        app.docs.view_rename = None;
    }
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

    /// Every tab's number ([`DocSlot::tab`]), in strip order; the shown one is numbered now if it
    /// was not yet.
    pub(crate) fn tab_numbers(&mut self) -> Vec<u64> {
        let shown = self.active_tab();
        if self.slots.is_empty() {
            return vec![shown];
        }
        self.slots.iter().map(|slot| slot.as_ref().map_or(shown, |slot| slot.tab)).collect()
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
        drawing: std::mem::take(&mut app.drawing_workspace),
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
    app.drawing_workspace = v.drawing;
    crate::edit_clipboard::drop_stale(app);
}

/// Put `slot` into the app and return what was there.
fn swap_in(app: &mut SolveApp, mut slot: DocSlot, presenting: bool) -> DocSlot {
    keep_look(app);
    if !presenting {
        crate::select_other::close(app);
    }
    let shared_chat = app.preferences.agent_shared_session;
    let reveals = crate::browser::take_reveals(app);
    if !presenting {
        crate::select_other::close(app);
        crate::semantic_content::leave_document(app);
        if crate::render_workspace::active(app) {
            let _ = crate::render_workspace::enter(app, false);
        }
        crate::timeline::stop_playback(app);
        crate::sketch_text_move::cancel_drag(app);
        crate::dim_view::cancel();
        app.cancel_view_animation();
        app.viewport.orbit_free = None;
        app.preview.reset();
        app.viewport.hover = None;
        crate::design_properties::close_object(app);
        crate::sketch_attachment_ui::clear(app);
        if shared_chat && app.agent_chat.following_document {
            crate::agent_chat::merge_active_turn_for_switch(app);
        } else {
            crate::agent_chat::leave_document(app);
        }
        app.edit_anchor_hint = None;
    }
    if !presenting {
        // A joint drag's shown frame stands for this design: it must not stay with the
        // session, and no drag goes on into the design that comes in.
        crate::joint_drag::leave(app);
        crate::control_joint_drive::discard(app);
    }
    let work = take_work(app, presenting);
    let outgoing_view = app.docs.views.active().id;
    let mut outgoing_navigation = ViewLook::from_ui(&app.ui);
    outgoing_navigation.motion.swap(app);
    app.docs.navigation.insert(outgoing_view, outgoing_navigation);
    // Tile slots are local to the shown design. Re-upload after another design used them.
    for look in slot.navigation.values_mut() {
        look.scenes = Default::default();
    }
    let incoming_view = slot.views.active().id;
    if let Some(mut look) = slot.navigation.remove(&incoming_view) {
        look.swap(app);
    } else {
        crate::viewport::ViewScenes::default().swap(&mut app.viewport);
    }
    let outgoing_chat = if shared_chat {
        app.docs.parked_agent_chat.replace(Box::new(slot.agent_chat)).map_or_else(Default::default, |chat| *chat)
    } else {
        app.docs.parked_agent_chat = None;
        app.agent_chat.swap_document(&mut slot.agent_chat);
        slot.agent_chat
    };
    let out_rev = app.session.revision;
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
        reveals,
        agent_chat: outgoing_chat,
        link_stamps: std::mem::replace(&mut app.docs.link_stamps, slot.link_stamps),
        edit_in_place: std::mem::replace(&mut app.edit_in_place.active, slot.edit_in_place),
        work,
        jobs: app.swap_jobs(slot.jobs),
        tab: std::mem::replace(&mut app.docs.active_tab, slot.tab),
        views: std::mem::replace(&mut app.docs.views, slot.views),
        inspect: std::mem::replace(&mut app.docs.inspect, slot.inspect),
        navigation: std::mem::replace(&mut app.docs.navigation, slot.navigation),
        fit: false,
        restore_views: std::mem::replace(&mut app.docs.restore_views, slot.restore_views),
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
    // The incoming design's plugin panels: the Model tab reads the installed plugins once, not on
    // every frame, so it no longer syncs them when the design comes to the front (#21594).
    solidcraft_engine::plugin::sync_panels(&mut app.session);
    // Views cache by revision: the incoming design must not look unchanged.
    let before = app.session.revision;
    if !presenting {
        app.session.invalidate_view(out_rev);
        if let Some(pending) = &mut app.pending_feature_edit {
            pending.view_changed(before, app.session.revision);
        }
        app.jobs_view_changed(before, app.session.revision);
    }
    if !presenting && shared_chat {
        crate::agent_chat::continue_in_document(app);
    }
    crate::browser::forget_found(app);
    put_work(app, slot.work, before);
    if presenting && app.docs.restore_views {
        // Protect the root spec before a passive child paint parks its camera.
        // Applying a workspace here would change the passive receiver.
        let view = app.docs.views.active().clone();
        show_look(app, &view);
    }
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
    let out = swap_in(app, incoming, false);
    // Give up the old design's renames now, before an editor drawn later in this frame commits one.
    give_up_renames_of_other_designs(app);
    if fit && !app.docs.restore_views {
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
    if std::mem::take(&mut app.docs.restore_views) {
        let view = app.docs.views.active().clone();
        apply_recalled_view(app, &view);
    }
    // The design comes back as the app shows it: a Render view it was left in shows only if the
    // workspace came back too (`put_work`).
    sync_views(app);
    crate::plugin_events::observe(app);
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

/// Make the app show `view` of the shown design: the one place a view change reaches the app, so
/// a new kind plugs in here. Model leaves the render workspace; Render enters the
/// render workspace. Sheet and Animation have no workspace yet and change nothing: enter theirs
/// here when they exist.
pub fn apply_view(app: &mut SolveApp, view: &DesignView) {
    if app.preferences.workspace_follows_view {
        if let Err(error) = crate::workspace::apply_focused_view(app, view) {
            app.set_status(error, true);
        }
        return;
    }
    match view.kind {
        ViewKind::Model => {
            if crate::render_workspace::active(app) {
                enter_workspace(app, false);
            }
        }
        ViewKind::Render => enter_workspace(app, true),
        ViewKind::Sheet => {
            if let Err(error) = crate::sheet_views::focus(app, view) {
                app.set_status(error, true);
            }
        }
        ViewKind::Animation => {}
    }
}

/// Enter the render workspace (`render`) or leave it, saying in the status why it could not.
fn enter_workspace(app: &mut SolveApp, render: bool) {
    if let Err(e) = crate::render_workspace::enter(app, render) {
        app.set_status(e, true);
    }
}

/// Reflect the render workspace in the active view. Sketch editing stays in the current Model
/// view and never creates a sub-tab. With the strip and switcher hidden, no view is opened here.
pub(crate) fn sync_views(app: &mut SolveApp) {
    if app.preferences.workspace_follows_view {
        return;
    }
    let shown = if crate::render_workspace::active(app) { Some((ViewKind::Render, None)) } else { None };
    let prefs = DesignTabPrefs::from(&app.preferences);
    // The Sketch ribbon shows because a sketch is open, not because the person left the Render
    // view: another view editing a sketch keeps the Render view the ground.
    let editing_elsewhere = app.session.sketching_view.is_some_and(|owner| owner != app.docs.views.active().id);
    let views = &mut app.docs.views;
    let before = views.active().id;
    match shown {
        Some((kind, subject)) => {
            if prefs.opens_views_by_itself() && prefs.opens_sub_tab(kind) && (views.active().kind != kind || views.active().subject != subject) {
                views.open(kind, subject, prefs.reuse_views);
            }
        }
        None if views.active().kind == ViewKind::Render && !editing_elsewhere => {
            let model = views.views().iter().find(|view| view.kind == ViewKind::Model).map(|view| view.id);
            if !model.is_some_and(|model| views.activate(model)) {
                views.open(ViewKind::Model, None, true);
            }
        }
        None => {}
    }
    let after = views.active().id;
    if before != after {
        switch_navigation(app, before, after);
    }
}

/// Change the shown design's views with `change`; when that changes the active view, the app
/// shows the new one ([`apply_view`]).
pub(crate) fn change_views<T>(app: &mut SolveApp, change: impl FnOnce(&mut ViewSet) -> T) -> T {
    let before = app.docs.views.active().id;
    keep_look(app);
    let result = change(&mut app.docs.views);
    app.docs.navigation.retain(|id, _| app.docs.views.has_open_or_closed(*id) || app.view_windows.keeps_view(app.docs.active_tab, *id));
    let active = app.docs.views.active();
    if active.id != before {
        let active = active.clone();
        crate::workspace::remember_view(app, before);
        switch_navigation(app, before, active.id);
        let perspective = app.ui.perspective;
        apply_view(app, &active);
        if active.kind == ViewKind::Model {
            // Leaving Render restores the Model projection from which it was entered.
            // An explicitly selected Model view already supplied its own projection.
            app.ui.perspective = perspective;
        }
        show_look(app, &active);
    }
    if let Err(error) = crate::sketch_view::sync(app) {
        app.set_status(error, true);
    }
    crate::drawing_sheet_page::retire_closed(app);
    result
}

/// The section and analyses each view of a design shows, by view id: a view that was never left
/// has none and shows what the design shows now.
pub(crate) type ViewInspect = std::collections::HashMap<u64, solidcraft_engine::InspectState>;

/// Runtime navigation state belongs to the view, including a gesture or animation in progress.
type ViewNavigation = std::collections::HashMap<u64, ViewLook>;

#[derive(Clone, Copy)]
struct ViewOptions {
    show_grid: bool,
    ground_shadow: bool,
    show_navigation_bar: bool,
    tool_cursor: bool,
    perspective: bool,
    up_axis: solidcraft_viewport::UpAxis,
    auto_ortho_faces: bool,
    visual_style: u8,
    view_angles_lock_yaw: bool,
    view_angles_lock_pitch: bool,
    view_angles_lock_roll: bool,
}

struct ViewLook {
    options: ViewOptions,
    motion: ViewMotion,
    scenes: crate::viewport::ViewScenes,
}

impl ViewOptions {
    fn from_ui(ui: &crate::UiState) -> Self {
        Self {
            show_grid: ui.show_grid,
            ground_shadow: ui.ground_shadow,
            show_navigation_bar: ui.show_navigation_bar,
            tool_cursor: ui.tool_cursor,
            perspective: ui.perspective,
            up_axis: ui.up_axis,
            auto_ortho_faces: ui.auto_ortho_faces,
            visual_style: ui.visual_style,
            view_angles_lock_yaw: ui.view_angles_lock_yaw,
            view_angles_lock_pitch: ui.view_angles_lock_pitch,
            view_angles_lock_roll: ui.view_angles_lock_roll,
        }
    }

    fn swap(&mut self, ui: &mut crate::UiState) {
        std::mem::swap(&mut self.show_grid, &mut ui.show_grid);
        std::mem::swap(&mut self.ground_shadow, &mut ui.ground_shadow);
        std::mem::swap(&mut self.show_navigation_bar, &mut ui.show_navigation_bar);
        std::mem::swap(&mut self.tool_cursor, &mut ui.tool_cursor);
        std::mem::swap(&mut self.perspective, &mut ui.perspective);
        std::mem::swap(&mut self.up_axis, &mut ui.up_axis);
        std::mem::swap(&mut self.auto_ortho_faces, &mut ui.auto_ortho_faces);
        std::mem::swap(&mut self.visual_style, &mut ui.visual_style);
        std::mem::swap(&mut self.view_angles_lock_yaw, &mut ui.view_angles_lock_yaw);
        std::mem::swap(&mut self.view_angles_lock_pitch, &mut ui.view_angles_lock_pitch);
        std::mem::swap(&mut self.view_angles_lock_roll, &mut ui.view_angles_lock_roll);
    }
}

impl ViewLook {
    fn from_ui(ui: &crate::UiState) -> Self {
        Self { options: ViewOptions::from_ui(ui), motion: ViewMotion::default(), scenes: Default::default() }
    }

    fn swap(&mut self, app: &mut SolveApp) {
        self.options.swap(&mut app.ui);
        self.motion.swap(app);
        self.scenes.swap(&mut app.viewport);
    }
}

/// The gesture state used by the one navigation path while this view is in the app.
#[derive(Default)]
struct ViewMotion {
    sketch_camera_target: Option<Camera>,
    cam_anim: Option<solidcraft_viewport::CameraAnim>,
    pending_fit: Option<(Camera, solidcraft_engine::geom::Aabb3, bool, bool)>,
    fit_waits: u32,
    axis_drag: Option<egui::Pos2>,
    axis_warp: Option<egui::Pos2>,
    nav: Option<crate::viewport::NavMode>,
    primary_nav: Option<crate::viewport::NavMode>,
    zoom_shortcut_drag: bool,
    modifier_nav: Option<(crate::viewport::NavMode, egui::Pos2)>,
    nav_toggle: Option<crate::viewport::NavMode>,
    nav_toggle_at: Option<egui::Pos2>,
    button_nav: Option<(crate::prefs::NavButton, Option<crate::viewport::NavMode>)>,
    right_drag: bool,
    orbit_pivot: Option<solidcore_record::Vec3>,
    orbit_pivot_at: Option<egui::Pos2>,
    twist_pivot: Option<solidcore_record::Vec3>,
    twist_last: Option<f64>,
    orbit_from: Option<crate::navigation::OrbitFrom>,
    cursor_wrap: Option<egui::Vec2>,
    orbit_free: Option<Camera>,
    spin: bool,
    spin_pivot: Option<solidcore_record::Vec3>,
    spin_pivot_at: Option<egui::Pos2>,
    wheel_zoom_raw_until_idle: bool,
    trackpad_until_idle: bool,
    trackpad_gesture: bool,
    trackpad_gesture_bracketed: bool,
    trackpad_coast: crate::navigation::TrackpadCoast,
    device_pivot: Option<solidcore_record::Vec3>,
    key_hold: Option<crate::navigation::KeyHold>,
    mouse: Option<egui::Pos2>,
    hover: Option<crate::viewport::Hit>,
    select_other: crate::select_other::State,
}

impl ViewMotion {
    fn swap(&mut self, app: &mut SolveApp) {
        std::mem::swap(&mut self.sketch_camera_target, &mut app.sketch_camera_target);
        std::mem::swap(&mut self.cam_anim, &mut app.cam_anim);
        std::mem::swap(&mut self.pending_fit, &mut app.pending_fit);
        std::mem::swap(&mut self.fit_waits, &mut app.fit_waits);
        std::mem::swap(&mut self.axis_drag, &mut app.viewport.axis_drag);
        std::mem::swap(&mut self.axis_warp, &mut app.viewport.axis_warp);
        std::mem::swap(&mut self.nav, &mut app.viewport.nav);
        std::mem::swap(&mut self.primary_nav, &mut app.viewport.primary_nav);
        std::mem::swap(&mut self.zoom_shortcut_drag, &mut app.viewport.zoom_shortcut_drag);
        std::mem::swap(&mut self.modifier_nav, &mut app.viewport.modifier_nav);
        std::mem::swap(&mut self.nav_toggle, &mut app.viewport.nav_toggle);
        std::mem::swap(&mut self.nav_toggle_at, &mut app.viewport.nav_toggle_at);
        std::mem::swap(&mut self.button_nav, &mut app.viewport.button_nav);
        std::mem::swap(&mut self.right_drag, &mut app.viewport.right_drag);
        std::mem::swap(&mut self.orbit_pivot, &mut app.viewport.orbit_pivot);
        std::mem::swap(&mut self.orbit_pivot_at, &mut app.viewport.orbit_pivot_at);
        std::mem::swap(&mut self.twist_pivot, &mut app.viewport.twist_pivot);
        std::mem::swap(&mut self.twist_last, &mut app.viewport.twist_last);
        std::mem::swap(&mut self.orbit_from, &mut app.viewport.orbit_from);
        std::mem::swap(&mut self.cursor_wrap, &mut app.viewport.cursor_wrap);
        std::mem::swap(&mut self.orbit_free, &mut app.viewport.orbit_free);
        std::mem::swap(&mut self.spin, &mut app.viewport.spin);
        std::mem::swap(&mut self.spin_pivot, &mut app.viewport.spin_pivot);
        std::mem::swap(&mut self.spin_pivot_at, &mut app.viewport.spin_pivot_at);
        std::mem::swap(&mut self.wheel_zoom_raw_until_idle, &mut app.viewport.wheel_zoom_raw_until_idle);
        std::mem::swap(&mut self.trackpad_until_idle, &mut app.viewport.trackpad_until_idle);
        std::mem::swap(&mut self.trackpad_gesture, &mut app.viewport.trackpad_gesture);
        std::mem::swap(&mut self.trackpad_gesture_bracketed, &mut app.viewport.trackpad_gesture_bracketed);
        std::mem::swap(&mut self.trackpad_coast, &mut app.viewport.trackpad_coast);
        std::mem::swap(&mut self.device_pivot, &mut app.viewport.device_pivot);
        std::mem::swap(&mut self.key_hold, &mut app.viewport.key_hold);
        std::mem::swap(&mut self.mouse, &mut app.viewport.mouse);
        std::mem::swap(&mut self.hover, &mut app.viewport.hover);
        std::mem::swap(&mut self.select_other, &mut app.viewport.select_other);
    }
}

fn switch_navigation(app: &mut SolveApp, before: u64, id: u64) {
    crate::select_other::close(app);
    let mut look = app.docs.navigation.remove(&id).unwrap_or_else(|| ViewLook::from_ui(&app.ui));
    look.swap(app);
    // A single view always uploads to slot zero, regardless of which sub-tab is shown.
    if !app.docs.views.is_tiled() {
        app.viewport.invalidate_view_uploads();
    }
    if app.docs.views.has_open_or_closed(before) {
        app.docs.navigation.insert(before, look);
    }
}

/// Find and select a design by its stable tab id, even after tabs move.
pub(crate) fn select_tab(app: &mut SolveApp, tab: u64) -> bool {
    if app.docs.current_tab() == tab {
        return true;
    }
    let Some(index) = app.docs.slots.iter().position(|slot| slot.as_ref().is_some_and(|slot| slot.tab == tab)) else {
        return false;
    };
    let Some(incoming) = app.docs.slots.get_mut(index).and_then(Option::take) else {
        return false;
    };
    let out = swap_in(app, incoming, true);
    if let Some(slot) = app.docs.slots.get_mut(app.docs.active) {
        *slot = Some(out);
    }
    app.docs.active = index;
    true
}

pub(crate) fn view_window_ids(app: &SolveApp) -> Vec<(u64, solidcraft_views::WindowId)> {
    std::iter::once((app.docs.current_tab(), &app.docs.views))
        .chain(app.docs.slots.iter().flatten().map(|slot| (slot.tab, &slot.views)))
        .flat_map(|(tab, views)| views.windows().iter().map(move |window| (tab, window.id)))
        .collect()
}

pub(crate) fn view_workspace(kind: ViewKind) -> &'static str {
    match kind {
        ViewKind::Render => "RENDER",
        ViewKind::Sheet => "DRAWING LAYOUT",
        _ => "MODEL",
    }
}

pub(crate) fn move_view_to_window(
    app: &mut SolveApp,
    id: u64,
    target: solidcraft_views::WindowTarget,
) -> Result<solidcraft_views::Moved, solidcraft_views::MoveError> {
    keep_look(app);
    let before = app.docs.views.active().id;
    let moved = app.docs.views.move_to_window(id, target)?;
    if before != app.docs.views.active().id {
        let active = app.docs.views.active().clone();
        switch_navigation(app, before, active.id);
        apply_view(app, &active);
        show_look(app, &active);
    }
    Ok(moved)
}

#[cfg(test)]
pub(crate) fn detach_view(app: &mut SolveApp, id: u64) -> Option<ViewSet> {
    keep_look(app);
    let before = app.docs.views.active().id;
    let detached = app.docs.views.detach(id)?;
    if before == id {
        park_navigation(app, before);
        let active = app.docs.views.active().clone();
        apply_view(app, &active);
        show_active_look(app);
    }
    Some(detached)
}

fn park_navigation(app: &mut SolveApp, id: u64) {
    let mut look = ViewLook::from_ui(&app.ui);
    look.swap(app);
    app.docs.navigation.insert(id, look);
}

pub(crate) fn park_active_look(app: &mut SolveApp) {
    keep_look(app);
    let id = app.docs.views.active().id;
    park_navigation(app, id);
}

pub(crate) fn show_active_look(app: &mut SolveApp) {
    let view = app.docs.views.active().clone();
    if let Some(mut look) = app.docs.navigation.remove(&view.id) {
        look.swap(app);
    }
    if view.kind == ViewKind::Render {
        crate::render_workspace::focus_view_framing(app, view.id);
    }
    if view.kind == ViewKind::Render
        && let Some(camera) = app.render_workspace.view.render_camera
    {
        app.cam = camera;
    }
    show_look(app, &view);
}

/// Put a window's view strip back when the window closes or its drawing finishes.
pub(crate) fn exchange_window_views(app: &mut SolveApp, tab: u64, views: &mut ViewSet) {
    if app.docs.current_tab() == tab {
        std::mem::swap(&mut app.docs.views, views);
    } else if let Some(slot) = app.docs.slots.iter_mut().flatten().find(|slot| slot.tab == tab) {
        std::mem::swap(&mut slot.views, views);
        if let Some(spec) = slot.views.active().spec {
            slot.cam = spec.camera;
        }
    }
}

pub(crate) fn put_view_window(app: &mut SolveApp, tab: u64, window: solidcraft_views::PopOut) {
    if app.docs.current_tab() == tab {
        app.docs.views.put_window(window);
    } else if let Some(slot) = app.docs.slots.iter_mut().flatten().find(|slot| slot.tab == tab) {
        slot.views.put_window(window);
    }
}

#[cfg(test)]
pub(crate) fn reattach_window_views(app: &mut SolveApp, tab: u64, views: ViewSet) {
    if app.docs.current_tab() == tab {
        app.docs.views.reattach(views);
    } else if let Some(slot) = app.docs.slots.iter_mut().flatten().find(|slot| slot.tab == tab) {
        slot.views.reattach(views);
    }
}

/// Keep how the shown Model view looks at the design (its camera) with that view, so it comes back
/// as it was left: two Model views of one design are two angles, not one.
pub(crate) fn keep_look(app: &mut SolveApp) {
    let active = app.docs.views.active();
    if matches!(active.kind, ViewKind::Model | ViewKind::Render) {
        let id = active.id;
        let model = active.kind == ViewKind::Model;
        app.docs.views.set_spec(id, solidcraft_viewport::ViewSpec::new(app.cam));
        if model {
            app.docs.inspect.insert(id, app.session.inspect());
        } else {
            crate::render_workspace::park_view_framing(app, id);
        }
    }
}

/// Show `view` from the angle it was left at. A view that was never left (a new one) keeps the
/// angle the design looks from now.
fn show_look(app: &mut SolveApp, view: &DesignView) {
    if matches!(view.kind, ViewKind::Model | ViewKind::Render)
        && let Some(spec) = view.spec
    {
        // Live options can already contain a new projection while this saved
        // camera still has the previous frame's fov. Restoring the camera must
        // not overwrite those pending options.
        app.cam = spec.camera;
    }
    if view.kind == ViewKind::Model
        && let Some(state) = app.docs.inspect.get(&view.id)
    {
        app.session.set_inspect(state.clone());
    }
}

/// Apply a recalled workspace after its initialized runtime options were restored.
fn apply_recalled_view(app: &mut SolveApp, view: &DesignView) {
    let projection = (app.ui.perspective, app.ui.up_axis);
    apply_view(app, view);
    (app.ui.perspective, app.ui.up_axis) = projection;
    show_look(app, view);
}

/// Run `draw` with the Model view `id` standing in for the shown one: its camera and analyses are
/// the app's while `draw` runs, and what `draw` left them as (an orbit, a dragged cut) is kept with
/// that view. The shown view's own come back afterwards. This is how the views of a tiled window
/// are each drawn and handled by the one viewport code.
pub(crate) fn with_view_look<T>(app: &mut SolveApp, id: u64, draw: impl FnOnce(&mut SolveApp) -> T) -> T {
    if id == app.docs.views.active().id {
        return draw(app);
    }
    let Some(view) = app.docs.views.get(id).cloned() else { return draw(app) };
    if !matches!(view.kind, ViewKind::Model | ViewKind::Render) {
        // Sheet canvases own their Page camera and receiver scope. Preserve the
        // tile identity without publishing the Model camera into their specs.
        let previous_compass = app.viewport.compass_view.replace(id);
        let out = draw(app);
        app.viewport.compass_view = previous_compass;
        return out;
    }
    let shown = (app.cam, app.session.inspect());
    let mut look = app.docs.navigation.remove(&id).unwrap_or_else(|| {
        let mut look = ViewLook::from_ui(&app.ui);
        // A Render view never shown yet starts with the projection of its own saved lens, as a
        // restored one does, so painting it does not re-project that lens to the app options.
        if view.kind == ViewKind::Render
            && let Some(spec) = view.spec
        {
            look.options.perspective = spec.camera.fov > 0.0;
        }
        look
    });
    look.swap(app);
    show_look(app, &view);
    let previous_compass = app.viewport.compass_view.replace(id);
    let out = draw(app);
    app.viewport.compass_view = previous_compass;
    app.docs.views.set_spec(id, solidcraft_viewport::ViewSpec::new(app.cam));
    app.docs.inspect.insert(id, app.session.inspect());
    look.swap(app);
    app.docs.navigation.insert(id, look);
    app.cam = shown.0;
    app.session.set_inspect(shown.1);
    out
}

/// Run a sketch camera transition in its owner, even when another native window is drawing.
pub(crate) fn with_sketch_camera<T>(app: &mut SolveApp, id: u64, change: impl FnOnce(&mut SolveApp) -> T) -> Option<T> {
    let view = crate::view_windows::layout_views(app).into_iter().find(|(_, view)| view.id == id).map(|(_, view)| view.clone())?;
    let painted = app.viewport.compass_view.unwrap_or_else(|| app.docs.views.active().id);
    if painted == id {
        return Some(change(app));
    }
    let shown = app.cam;
    let mut look = app.docs.navigation.remove(&id).unwrap_or_else(|| ViewLook::from_ui(&app.ui));
    look.swap(app);
    app.cam = view.spec.map_or(shown, |spec| spec.camera);
    let compass = app.viewport.compass_view.replace(id);
    let result = change(app);
    app.viewport.compass_view = compass;
    crate::view_windows::set_sketch_camera(app, id, app.cam);
    look.swap(app);
    app.docs.navigation.insert(id, look);
    app.cam = shown;
    Some(result)
}

/// Make the view `id` the shown one, keeping the look of the one it replaces.
pub(crate) fn focus_view(app: &mut SolveApp, id: u64) -> bool {
    activate_local_view(app, id)
}

/// Open a view of `kind` showing `subject` in the shown design and show it (`reuse`: one that
/// already has that kind and subject is shown instead, unless Preferences turn reuse off). Returns
/// the id of the view now active. When Preferences keep that kind out of sub-tabs, no view is
/// added: the design enters the state in the view already shown, which stays active.
pub fn view_open(app: &mut SolveApp, kind: ViewKind, subject: Option<String>, reuse: bool) -> u64 {
    let prefs = DesignTabPrefs::from(&app.preferences);
    if !app.preferences.workspace_follows_view && !prefs.opens_sub_tab(kind) {
        apply_view(app, &DesignView { editing: None, id: 0, kind, name: String::new(), subject, spec: None, compass: true });
        return app.docs.views.active().id;
    }
    change_views(app, |views| views.open(kind, subject, reuse && prefs.reuse_views))
}

/// Whether a remembered view of `kind` showing `subject` can be shown in `session`'s design again:
/// Sheet views return only while their exact sheet still exists.
fn can_restore(session: &Session, kind: ViewKind, subject: Option<&str>) -> bool {
    match kind {
        ViewKind::Model | ViewKind::Render => true,
        ViewKind::Sheet => crate::sheet_views::resolve_for_session(session, subject).is_some(),
        ViewKind::Animation => false,
    }
}

/// Remember the views of design tab `i` in `dir`, when it is saved to a file and Preferences ask
/// for views to come back ([`restore_views_in`]).
fn remember_views_in(app: &SolveApp, i: usize, dir: &std::path::Path) {
    if !app.preferences.design_reopen_views {
        return;
    }
    let session = if i == app.docs.active { Some(&app.session) } else { app.docs.slots.get(i).and_then(Option::as_ref).map(|slot| &slot.session) };
    let (Some(path), Some(views)) = (session.and_then(|session| session.path.as_deref()), views_of(app, i)) else { return };
    // Every design keeps its live camera, including one parked in a background slot.
    let mut views = views.clone();
    let camera = if i == app.docs.active { Some(app.cam) } else { app.docs.slots.get(i).and_then(Option::as_ref).map(|slot| slot.cam) };
    if views.active().kind == ViewKind::Model
        && let Some(camera) = camera
    {
        let id = views.active().id;
        views.set_spec(id, solidcraft_viewport::ViewSpec::new(camera));
    }
    // Persist views now; native placement restoration is a later slice.
    let windows: Vec<_> = views.windows().iter().map(|window| window.id).collect();
    for window in windows {
        views.close_window(window);
    }
    let remembered = crate::view_memory::remember_async(dir.to_owned(), path.to_owned(), views.to_json());
    if let Err(e) = remembered {
        log::warn!("could not remember the views of {path}: {e}");
    }
}

/// [`remember_views_in`] in the user's data folder.
pub(crate) fn remember_views(app: &SolveApp, i: usize) {
    if let Some(dir) = solidcraft_engine::recovery::data_dir() {
        remember_views_in(app, i, &dir);
    }
}

/// Give the design just opened from `path` the views it was closed with, from `dir`: those that
/// can still be shown ([`can_restore`]). The design shown also shows its active one.
fn restore_views_in(app: &mut SolveApp, path: &str, dir: &std::path::Path) {
    if !app.preferences.design_reopen_views {
        return;
    }
    let Some(session) = session_at(app, path) else { return };
    let Some(mut set) = crate::view_memory::recall(dir, path)
        .and_then(|saved| crate::view_memory::restorable(&saved, |kind, subject| can_restore(session, kind, subject)))
    else {
        return;
    };
    let windows: Vec<_> = set.windows().iter().map(|window| window.id).collect();
    for window in windows {
        set.close_window(window);
    }
    // Saved cameras initialize runtime options once. Subsequent UI changes are
    // authoritative even until the next frame writes their projection into cam.
    let navigation: ViewNavigation = set
        .views()
        .iter()
        .map(|view| {
            let mut look = ViewLook::from_ui(&app.ui);
            if matches!(view.kind, ViewKind::Model | ViewKind::Render)
                && let Some(spec) = view.spec
            {
                look.options.perspective = spec.camera.fov > 0.0;
                look.options.up_axis = spec.camera.up;
            }
            (view.id, look)
        })
        .collect();
    if app.session.path.as_deref().is_some_and(|p| same_path(p, path)) {
        app.docs.views = set;
        app.docs.restore_views = false;
        app.docs.inspect.clear();
        app.docs.navigation = navigation;
        let active = app.docs.views.active().clone();
        if let Some(mut look) = app.docs.navigation.remove(&active.id) {
            look.swap(app);
        }
        apply_recalled_view(app, &active);
    } else if let Some(slot) = app.docs.slots.iter_mut().flatten().find(|slot| slot.session.path.as_deref().is_some_and(|p| same_path(p, path))) {
        if matches!(set.active().kind, ViewKind::Model | ViewKind::Render)
            && let Some(spec) = set.active().spec
        {
            slot.cam = spec.camera;
        }
        slot.views = set;
        slot.inspect.clear();
        slot.navigation = navigation;
        slot.restore_views = true;
    }
}

/// A design from the file `path` has just finished opening (shown or behind): it gets its
/// remembered views back, and the shown one takes the view the command line asked to start on
/// (`--view`, once).
pub(crate) fn design_opened(app: &mut SolveApp, path: &str) {
    if let Some(dir) = solidcraft_engine::recovery::data_dir() {
        restore_views_in(app, path, &dir);
    }
    if app.session.path.as_deref().is_some_and(|p| same_path(p, path))
        && let Some(kind) = crate::new_window::take_start_view().and_then(|key| ViewKind::from_key(&key))
    {
        view_open(app, kind, None, true);
    }
}

/// Close the view `id` of the shown design (never the last one; Reopen brings it back).
pub fn view_close(app: &mut SolveApp, id: u64) -> bool {
    if !app.view_windows.is_child() && app.docs.views.len() == 1 && app.docs.views.get(id).is_some() {
        return false;
    }
    if !crate::sketch_view::before_close(app, &[id]) {
        return false;
    }
    if let Some(closed) = crate::view_windows::close_view(app, id) {
        return closed;
    }
    change_views(app, |views| views.close(id))
}

/// Stop showing the view `id` beside the others; it stays open, reached from the bar.
pub fn view_unshow(app: &mut SolveApp, id: u64) -> bool {
    change_views(app, |views| views.unshow(id))
}

/// Close the other views of the shown design, keeping `id`.
pub fn view_close_others(app: &mut SolveApp, id: u64) {
    let closing: Vec<_> = app.docs.views.views().iter().map(|view| view.id).filter(|other| *other != id).collect();
    if !crate::sketch_view::before_close(app, &closing) {
        return;
    }
    change_views(app, |views| views.close_others(id));
}

/// Close the views after `id` in the shown design.
pub fn view_close_right_of(app: &mut SolveApp, id: u64) {
    let closing: Vec<_> = app.docs.views.views().iter().map(|view| view.id).skip_while(|other| *other != id).skip(1).collect();
    if !crate::sketch_view::before_close(app, &closing) {
        return;
    }
    change_views(app, |views| views.close_right_of(id));
}

/// Show the view `id` of the shown design. False when there is none.
pub fn view_activate(app: &mut SolveApp, id: u64) -> bool {
    crate::view_windows::focus_view(app, id, None).is_ok()
}

pub(crate) fn activate_local_view(app: &mut SolveApp, id: u64) -> bool {
    if app.docs.views.get(id).is_none() {
        return false;
    }
    crate::view_windows::record_local_focus(app, id);
    let follows = app.preferences.workspace_follows_view;
    change_views(app, |views| {
        let previous = views.active().id;
        if follows && views.is_tiled() && !views.tiles().is_some_and(|tiles| tiles.contains(id)) {
            views.place_tile(id, solidcraft_views::Placement::Replace(previous));
        }
        views.activate(id)
    })
}

/// Bring back the view the shown design closed last and show it.
pub fn view_reopen(app: &mut SolveApp) -> Option<u64> {
    change_views(app, ViewSet::reopen)
}

/// Rename the view `id` of the shown design (a name [`ViewSet::rename`] accepts).
pub fn view_rename(app: &mut SolveApp, id: u64, name: &str) -> bool {
    app.docs.views.rename(id, name)
}

/// Start typing a new name for the view `id` of the shown design in its sub-tab, with the name as
/// it is. False when there is no such view.
pub fn view_rename_start(app: &mut SolveApp, id: u64) -> bool {
    let Some(name) = app.docs.views.get(id).map(|view| view.name.clone()) else { return false };
    app.docs.view_rename = Some(ViewRename { view: id, text: name, focused: false, session: app.session.uid });
    true
}

/// Copy the view `id` of the shown design next to it and show the copy.
pub fn view_duplicate(app: &mut SolveApp, id: u64) -> Option<u64> {
    let state = if id == app.docs.views.active().id { Some(app.session.inspect()) } else { app.docs.inspect.get(&id).cloned() };
    let options = app.docs.navigation.get(&id).map(|look| look.options);
    let copy = change_views(app, |views| views.duplicate(id))?;
    if let Some(mut options) = options {
        options.swap(&mut app.ui);
    }
    if let Some(state) = state {
        app.session.set_inspect(state);
        app.docs.inspect.insert(copy, app.session.inspect());
    }
    Some(copy)
}

/// Arrange the available views, adding Model views or leaving picker cells as requested.
pub fn view_layout(app: &mut SolveApp, preset: solidcraft_views::Preset) -> usize {
    if preset == solidcraft_views::Preset::Single {
        app.docs.views.untile();
        return 1;
    }
    if !app.docs.views.active().kind.can_tile() {
        return 1;
    }
    let fill = app.docs.layout_fill;
    if fill == LayoutFill::Picker {
        return app.docs.views.set_layout_padded(preset);
    }
    let shown = app.docs.views.active().clone();
    for n in app.docs.views.layout_view_count()..preset.tiles() {
        let added = match fill {
            LayoutFill::Standard => view_copy_at(app, FILL_ANGLES[n.saturating_sub(1).min(FILL_ANGLES.len() - 1)]),
            _ if shown.kind == ViewKind::Model => view_duplicate(app, shown.id),
            _ => new_model_at(app, None),
        };
        if added.is_none() {
            break;
        }
        change_views(app, |views| views.activate(shown.id));
    }
    change_views(app, |views| views.set_layout(preset))
}

/// The standard views the empty places of a layout get, in turn ([`LayoutFill::Standard`]).
const FILL_ANGLES: [solidcraft_viewport::StandardView; 3] =
    [solidcraft_viewport::StandardView::Front, solidcraft_viewport::StandardView::Right, solidcraft_viewport::StandardView::Top];

/// A new Model view of the shown design looking from `angle`, next to the shown one, which stays
/// the shown one. None when the design has no room for another view.
fn view_copy_at(app: &mut SolveApp, angle: solidcraft_viewport::StandardView) -> Option<u64> {
    if app.docs.views.active().kind != ViewKind::Model {
        return new_model_at(app, Some(angle));
    }
    let shown = app.docs.views.active().id;
    let copy = view_duplicate(app, shown)?;
    // The copy is shown now, its look in the app: turn it, and going back to the first keeps that.
    app.cam.set_view(angle);
    change_views(app, |views| views.activate(shown));
    Some(copy)
}

/// Start a Model view at the saved Model camera when a different workspace is showing.
fn new_model_at(app: &mut SolveApp, angle: Option<solidcraft_viewport::StandardView>) -> Option<u64> {
    if app.docs.views.len() >= solidcraft_views::MAX_VIEWS {
        return None;
    }
    let active = app.docs.views.active().id;
    let mut camera =
        app.docs.views.views().iter().find(|view| view.kind == ViewKind::Model).and_then(|view| view.spec).map_or(app.cam, |spec| spec.camera);
    if let Some(angle) = angle {
        camera.set_view(angle);
    }
    let id = app.docs.views.open(ViewKind::Model, None, false);
    app.docs.views.set_spec(id, solidcraft_viewport::ViewSpec::new(camera));
    app.docs.views.activate(active);
    Some(id)
}

/// Show a new view looking from `angle` in the empty cell number `ordinal` of the layout.
pub fn view_fill_angle(app: &mut SolveApp, ordinal: usize, angle: solidcraft_viewport::StandardView) -> Option<u64> {
    let copy = view_copy_at(app, angle)?;
    change_views(app, |views| views.fill_empty(ordinal, copy)).then_some(copy)
}

/// Show the open view `id` in the empty cell number `ordinal` of the layout.
pub fn view_fill_open(app: &mut SolveApp, ordinal: usize, id: u64) -> bool {
    change_views(app, |views| views.fill_empty(ordinal, id))
}

/// Close the empty cell number `ordinal` of the layout.
pub fn view_close_empty(app: &mut SolveApp, ordinal: usize) -> bool {
    change_views(app, |views| views.remove_empty(ordinal))
}

/// Show an open view beside the active view. With no id, start another Model view.
pub fn view_beside(app: &mut SolveApp, id: Option<u64>, edge: solidcraft_views::Edge) -> Option<u64> {
    let shown = app.docs.views.active().clone();
    if !shown.kind.can_tile() || (app.docs.views.is_tiled() && app.docs.views.tiles().is_some_and(|tiles| tiles.len() >= solidcraft_views::MAX_TILES))
    {
        return None;
    }
    let id = match id {
        Some(id) => id,
        None if shown.kind == ViewKind::Render => new_model_at(app, None)?,
        None => {
            let copy = view_duplicate(app, shown.id)?;
            view_activate(app, shown.id);
            copy
        }
    };
    change_views(app, |views| views.show_beside(id, edge)).then_some(id)
}

/// Move the view at place `from` of the shown design to place `to`.
pub fn view_move(app: &mut SolveApp, from: usize, to: usize) {
    crate::view_windows::remember_strip_order(app);
    app.docs.views.move_view(from, to);
    let window = app.view_windows.current_window();
    crate::view_windows::remember_local_reorder(app, window);
}

/// The views of the design in tab `design` (the strip's order), each with whether it is shown.
/// Empty for a tab that does not exist.
pub fn views_info(app: &SolveApp, design: usize) -> Vec<(DesignView, bool)> {
    views_of(app, design).map(|set| set.views().iter().map(|view| (view.clone(), view.id == set.active().id)).collect()).unwrap_or_default()
}

pub(crate) fn visible_view_ids(app: &SolveApp) -> Vec<u64> {
    let views = &app.docs.views;
    views.tiles().filter(|_| views.is_tiled()).map_or_else(|| vec![views.active().id], solidcraft_views::Tiles::views)
}

/// Copy only the linked components to other visible Model cameras in this window.
pub(crate) fn link_visible_cameras(app: &mut SolveApp, source: u64, camera: Camera, before: Option<Camera>) {
    use solidcraft_views::CameraLinkMode::{Both, Orientation, Zoom};
    if crate::sketch_view::camera_transition(app, source, camera) {
        return;
    }
    let mode = app.docs.views.camera_link();
    let orientation = matches!(mode, Both | Orientation)
        && before.is_none_or(|before| (before.yaw, before.pitch, before.roll) != (camera.yaw, camera.pitch, camera.roll));
    let zoom = matches!(mode, Both | Zoom) && before.is_none_or(|before| (before.half_height() - camera.half_height()).abs() > 1e-9);
    if !(orientation || zoom)
        || !app.docs.views.is_tiled()
        || !camera.is_valid()
        || !app.docs.views.get(source).is_some_and(|view| view.kind == ViewKind::Model)
    {
        return;
    }
    let active = app.docs.views.active().id;
    for id in visible_view_ids(app).into_iter().filter(|id| *id != source) {
        let Some(view) = app.docs.views.get(id).filter(|view| view.kind == ViewKind::Model) else { continue };
        let mut to = if id == active { app.cam } else { view.spec.map_or(app.cam, |spec| spec.camera) };
        if orientation {
            (to.yaw, to.pitch, to.roll) = (camera.yaw, camera.pitch, camera.roll);
        }
        if zoom {
            let scale = to.half_height() / to.distance;
            if scale.is_finite() && scale > 0.0 {
                to.distance = (camera.half_height() / scale).clamp(1e-3, 1e7);
            }
        }
        if id == active {
            app.cam = to;
            app.cam_anim = None;
            app.pending_fit = None;
        } else if let Some(look) = app.docs.navigation.get_mut(&id) {
            look.motion.cam_anim = None;
            look.motion.pending_fit = None;
        }
        app.docs.views.set_spec(id, solidcraft_viewport::ViewSpec::new(to));
    }
}

pub(crate) fn align_linked_cameras(app: &mut SolveApp) {
    let view = app.docs.views.active();
    let source = if view.kind == ViewKind::Model {
        Some((view.id, app.cam))
    } else {
        visible_view_ids(app).into_iter().find_map(|id| {
            let view = app.docs.views.get(id)?;
            (view.kind == ViewKind::Model).then(|| (id, view.spec.map_or(app.cam, |spec| spec.camera)))
        })
    };
    if let Some((id, camera)) = source {
        link_visible_cameras(app, id, camera, None);
    }
}

pub(crate) fn sync_linked_active(app: &mut SolveApp) {
    let view = app.docs.views.active();
    if view.kind != ViewKind::Model || !app.docs.views.is_tiled() || app.docs.views.camera_link() == solidcraft_views::CameraLinkMode::Off {
        return;
    }
    let id = view.id;
    let before = view.spec.map(|spec| spec.camera);
    link_visible_cameras(app, id, app.cam, before);
    app.docs.views.set_spec(id, solidcraft_viewport::ViewSpec::new(app.cam));
}

/// How many views each design has, in tab order (the count badge of a tab with more than one).
pub fn view_counts(app: &SolveApp) -> Vec<usize> {
    (0..app.docs.count()).map(|design| views_of(app, design).map_or(1, ViewSet::len)).collect()
}

fn views_of(app: &SolveApp, design: usize) -> Option<&ViewSet> {
    if design == app.docs.active { Some(&app.docs.views) } else { app.docs.slots.get(design)?.as_ref().map(|slot| &slot.views) }
}

/// Live root-window slots across all designs. Closed views retain their camera/navigation,
/// but their consumed upload slots must be fresh before Reopen can draw them again.
pub(crate) fn live_view_picture_slots(app: &mut SolveApp) -> Vec<u64> {
    fn collect(views: &ViewSet, navigation: &mut ViewNavigation, slots: &mut Vec<u64>) {
        slots.extend(views.views().iter().map(|view| view.id));
        for (id, look) in navigation {
            if views.window_of(*id).is_none() {
                look.scenes.retire_uploads();
            }
        }
    }
    let mut slots = vec![0];
    collect(&app.docs.views, &mut app.docs.navigation, &mut slots);
    for slot in app.docs.slots.iter_mut().flatten() {
        collect(&slot.views, &mut slot.navigation, &mut slots);
    }
    slots.sort_unstable();
    slots.dedup();
    slots
}

/// Exact native ownership after every temporary child/document swap has ended.
pub(crate) fn view_picture_window_owners(app: &SolveApp) -> std::collections::HashMap<(u64, solidcraft_views::WindowId), Vec<u64>> {
    let mut owners = std::collections::HashMap::new();
    let mut add = |tab: u64, views: &ViewSet| {
        for window in views.windows() {
            owners.insert((tab, window.id), window.views.views().iter().map(|view| view.id).collect());
        }
    };
    add(app.docs.current_tab(), &app.docs.views);
    for slot in app.docs.slots.iter().flatten() {
        add(slot.tab, &slot.views);
    }
    owners
}

/// Carry out a checked `tab.view.*` command (`v`: the engine's echo of its parameters) on the
/// shown design, and return the reply with what was done filled in: the engine holds no views.
pub(crate) fn view_command(app: &mut SolveApp, command: &str, v: Value) -> Result<Value, String> {
    if matches!(
        command,
        "tab.view.layout"
            | "tab.view.beside"
            | "tab.view.open"
            | "tab.view.equal_sizes"
            | "tab.view.link"
            | "tab.view.reset_cameras"
            | "tab.view.close_others"
            | "tab.view.close_right"
    ) {
        let requested = v.get("window").and_then(Value::as_u64).and_then(|id| u32::try_from(id).ok()).or_else(|| {
            if matches!(command, "tab.view.close_others" | "tab.view.close_right") {
                v.get("id")
                    .and_then(Value::as_u64)
                    .and_then(|id| app.docs.views.window_of(id).map(|window| if window == 0 { app.view_windows.current_window() } else { window }))
            } else {
                None
            }
        });
        return crate::view_windows::with_strip(app, requested, |app| view_command_local(app, command, v));
    }
    view_command_local(app, command, v)
}

pub(crate) fn set_compass(app: &mut SolveApp, request: Value) -> Result<Value, String> {
    let options: solidcraft_engine::schema::tab_views::ViewCompassParams =
        serde_json::from_value(request.clone()).map_err(|error| error.to_string())?;
    if !app.docs.views.set_compass(options.view, options.on) {
        return Err("no_such_3d_view".into());
    }
    Ok(request)
}

fn view_command_local(app: &mut SolveApp, command: &str, mut v: Value) -> Result<Value, String> {
    let active = app.docs.views.active().id;
    let asked = v.get("id").and_then(Value::as_u64).unwrap_or(active);
    let place = |key: &str| v.get(key).and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok());
    let id = match command {
        "tab.view.open" => {
            let kind = v.get("kind").and_then(Value::as_str).and_then(ViewKind::from_key).unwrap_or(app.docs.views.active().kind);
            let mut subject = v.get("subject").and_then(Value::as_str).map(str::to_owned);
            if kind == ViewKind::Sheet {
                let (drawing, sheet) = crate::sheet_views::resolve(app, subject.as_deref())?;
                subject = Some(crate::sheet_views::subject(drawing, sheet));
                v["subject"] = json!(subject);
            }
            let reuse = v.get("reuse").and_then(Value::as_bool).unwrap_or(false);
            if kind == ViewKind::Sheet
                && (app.preferences.workspace_follows_view || app.preferences.design_sub_tab_sheets)
                && !(reuse
                    && app.preferences.design_reuse_views
                    && app.docs.views.views().iter().any(|view| view.kind == kind && view.subject == subject))
                && crate::view_windows::layout_views(app).len() >= solidcraft_views::MAX_VIEWS
            {
                return Err(app.tr("sheet_views.limit").to_owned());
            }
            Some(view_open(app, kind, subject, reuse))
        }
        "tab.view.close" => {
            if app.docs.views.window_of(asked).is_none() {
                return Err(format!("there is no view {asked}"));
            }
            if !view_close(app, asked) {
                return Err("the last view of a design stays open".into());
            }
            Some(asked)
        }
        "tab.view.close_others" | "tab.view.close_right" => {
            if app.docs.views.get(asked).is_none() {
                return Err(format!("there is no view {asked} in this window"));
            }
            if command == "tab.view.close_others" {
                view_close_others(app, asked);
            } else {
                view_close_right_of(app, asked);
            }
            v["window"] = json!(app.view_windows.current_window());
            Some(asked)
        }
        "tab.view.reopen" => Some(view_reopen(app).ok_or("there is no closed view to reopen")?),
        "tab.view.activate" => {
            let window = v.get("window").and_then(Value::as_u64).and_then(|id| u32::try_from(id).ok());
            let id = match place("index") {
                Some(index) => crate::view_windows::view_at(app, window, index)?,
                None => asked,
            };
            crate::view_windows::focus_view(app, id, window)?;
            Some(id)
        }
        "tab.view.rename" => {
            match v.get("name").and_then(Value::as_str) {
                // No name: the window types it in the view's own sub-tab.
                None if view_rename_start(app, asked) => {}
                Some(name) if view_rename(app, asked, name) => {}
                _ => return Err(format!("there is no view {asked}, or its name is not acceptable")),
            }
            Some(asked)
        }
        "tab.view.duplicate" => Some(view_duplicate(app, asked).ok_or("there is no such view, or the design has too many views")?),
        "tab.view.layout" => {
            let preset = v.get("layout").and_then(Value::as_str).and_then(solidcraft_views::Preset::from_key).ok_or("unknown layout")?;
            let shown = view_layout(app, preset);
            align_linked_cameras(app);
            v["shown"] = json!(shown);
            None
        }
        "tab.view.equal_sizes" => {
            app.docs.views.equal_sizes();
            v["shown"] = json!(visible_view_ids(app).len());
            None
        }
        "tab.view.link" => {
            let params = serde_json::from_value::<solidcraft_engine::schema::tab_views::TabViewLinkParams>(v).map_err(|error| error.to_string())?;
            app.docs.views.set_camera_link(params.mode.into());
            align_linked_cameras(app);
            return serde_json::to_value(params).map_err(|error| error.to_string());
        }
        "tab.view.reset_cameras" => {
            crate::view_tiles::reset_cameras(app);
            v["shown"] = json!(visible_view_ids(app).len());
            None
        }
        "tab.view.beside" => {
            let id = if let Some(placement) = v.get("placement") {
                let placement = serde_json::from_value::<solidcraft_engine::schema::tab_views::TabViewPlacement>(placement.clone())
                    .map_err(|error| error.to_string())?;
                if !change_views(app, |views| views.place_tile(asked, placement.into())) {
                    return Err("the requested view cannot be placed in that cell".into());
                }
                asked
            } else {
                let edge = match v.get("edge").and_then(Value::as_str) {
                    Some("left") => solidcraft_views::Edge::Left,
                    Some("top") => solidcraft_views::Edge::Top,
                    Some("bottom") => solidcraft_views::Edge::Bottom,
                    _ => solidcraft_views::Edge::Right,
                };
                let asked = v.get("id").and_then(Value::as_u64);
                view_beside(app, asked, edge).ok_or_else(|| app.tr("sheet_views.cannot_tile").to_owned())?
            };
            v["shown"] = json!(app.docs.views.tiles().map_or(1, solidcraft_views::Tiles::len));
            Some(id)
        }
        "tab.view.move_to_window" => {
            let view = v.get("view").and_then(Value::as_u64).ok_or("no_such_view")?;
            return crate::view_windows::detach(app, view, &v);
        }
        "tab.view.pop_out" => {
            crate::view_windows::detach(app, asked, &v)?;
            Some(asked)
        }
        "tab.view.move" => {
            let len = app.docs.views.len();
            let (from, to) = (place("from").filter(|n| *n < len), place("to").filter(|n| *n < len));
            let (Some(from), Some(to)) = (from, to) else { return Err(format!("places are 0 to {}", len - 1)) };
            view_move(app, from, to);
            None
        }
        _ => None,
    };
    sync_views(app);
    if let Some(reply) = v.as_object_mut() {
        if let Some(id) = id {
            reply.insert("id".into(), json!(id));
        }
        if command == "tab.view.list" {
            let list = views_info(app, app.docs.active)
                .into_iter()
                .map(|(view, active)| json!({"id": view.id, "kind": view.kind.key(), "name": view.name, "subject": view.subject, "active": active}));
            reply.insert("views".into(), Value::Array(list.collect()));
        }
    }
    Ok(v)
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
    if app.agent_chat.following_document && index != app.docs.active && !app.preferences.agent_shared_session {
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
        // Never lowercase: Windows can also host case-sensitive directories. A path on a network
        // location not allowed (other than the host the other path is on) or a device is never
        // resolved: on Windows that connects to the host before any "network location" question.
        use solidcraft_engine::io::vfs;
        let reachable = |p: &str, other: &str| vfs::design_path_refusal(p, Some(other)).is_none();
        if reachable(a, b)
            && reachable(b, a)
            && let (Ok(a), Ok(b)) = (vfs::canonicalize(a), vfs::canonicalize(b))
        {
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
pub(crate) fn open_tab_behind(app: &mut SolveApp, mut session: Session) -> Result<(), String> {
    if app.docs.slots.is_empty() {
        app.docs.slots.push(None);
        app.docs.active = 0;
    }
    if app.docs.slots.len() >= MAX_OPEN {
        return Err(format!("at most {MAX_OPEN} designs can be open"));
    }
    session.plugin_state.publish_user_settings().map_err(|e| e.to_string())?;
    let mut slot = DocSlot::new(session);
    slot.fit = true;
    app.docs.slots.insert(app.docs.active + 1, Some(slot));
    Ok(())
}

/// Open `session` in a new tab after the active one and switch to it (`fit`: frame its model).
pub fn open_tab(app: &mut SolveApp, mut session: Session) -> Result<(), String> {
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
    session.plugin_state.publish_user_settings().map_err(|e| e.to_string())?;
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
pub fn adopt(app: &mut SolveApp, mut session: Session) -> bool {
    if active_is_blank(app) {
        if let Err(error) = session.plugin_state.publish_user_settings() {
            app.set_status(error.to_string(), true);
            return false;
        }
        let rev = app.session.revision;
        crate::browser::discard_reveals(app);
        crate::semantic_content::leave_document(app);
        crate::dialogs::cancel(app);
        app.tool = None;
        app.bom = None;
        crate::design_properties::close_object(app);
        crate::sketch_attachment_ui::clear(app);
        app.session.replace_design(session);
        give_up_renames_of_other_designs(app);
        reset_chat(app);
        // Another design in the tab: what waits for the blank one's tab no longer does.
        app.docs.active_tab = next_tab();
        app.docs.views = ViewSet::new();
        app.docs.navigation.clear();
        // The blank design's record of its links goes with it; the new one's start now.
        app.docs.link_stamps.clear();
        crate::history_folds::load(app);
        crate::timeline_scope::reset(app);
        crate::render_workspace::reset_document(app);
        crate::drawing_workspace::reset_document(app);
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

/// Show `session` in a new tab after the active one, even when the active design is blank
/// ([`adopt`] replaces a blank one). Whether it is shown (no tab left for it: the status says so).
pub fn adopt_in_new_tab(app: &mut SolveApp, session: Session) -> bool {
    match open_tab(app, session) {
        Ok(()) => true,
        Err(e) => {
            app.set_status(e, true);
            false
        }
    }
}

/// Run `id` (`file.new` or `doc.open`, given `new_tab`) in a tab of its own: a blank tab opens
/// after the active one and is shown, then the command runs in it as it would in any tab. When the
/// command fails the tab closes again and the design that was shown comes back, as nothing had
/// happened. `new_tab` is taken off the parameters: the engine has no tabs.
pub(crate) fn run_in_new_tab(app: &mut SolveApp, id: &str, params: Value) -> Result<Value, String> {
    // A design already open in a tab is shown, as the control channel's loader shows it: opening
    // it again would make a second copy and could lose the unsaved edits of the first.
    if id == "doc.open"
        && let Some(path) = params.get("path").and_then(Value::as_str)
        && params.as_object().is_some_and(|p| p.iter().all(|(key, value)| key == "path" || value.is_null()))
        && reuse_path(app, path)?
    {
        let s = &app.session;
        return Ok(json!({"path": path, "features": s.doc.features.len(),
            "errors": s.model.results.iter().filter(|r| r.error.is_some()).count(), "name": s.doc.name}));
    }
    let before = app.docs.active;
    open_tab(app, Session::default())?;
    crate::prefs::apply_new_design(app);
    let at = app.docs.active;
    match app.run(id, params) {
        Ok(mut reply) => {
            if id == "file.new"
                && let Some(reply) = reply.as_object_mut()
            {
                reply.insert("document_id".into(), json!(app.session.document_id));
            }
            Ok(reply)
        }
        Err(e) => {
            // Back to the design that was shown, then the tab that was opened for this goes.
            switch(app, before);
            close(app, at, true);
            Err(e)
        }
    }
}

/// `new_tab` of a `file.new` or `doc.open`: whether it is true, taken off `params` (the engine
/// refuses it, as it has one design and no tabs). Left out, null or false it changes nothing. A
/// value that is no boolean stays in `params`, so the engine's own contract check sees it.
pub(crate) fn take_new_tab(params: &mut Value) -> bool {
    let Some(p) = params.as_object_mut() else { return false };
    match p.get("new_tab") {
        Some(Value::Bool(_) | Value::Null) => p.remove("new_tab") == Some(Value::Bool(true)),
        _ => false,
    }
}

/// `p` as the contract type `T`; no parameters at all (`null`) is the empty object.
fn read_params<T: serde::de::DeserializeOwned>(command: &str, p: &Value) -> Result<T, String> {
    let p = if p.is_null() { json!({}) } else { p.clone() };
    serde_json::from_value(p).map_err(|e| format!("{command}: {e}"))
}

/// `document.list` and `document.activate`: the window answers them for every design tab, as the
/// engine, with one design, cannot. Ids are the ones `document.revision` gives.
pub(crate) fn command(app: &mut SolveApp, id: &str, p: &Value) -> Result<Value, String> {
    use solidcraft_engine::schema::documents::{
        DocumentActivateParams, DocumentActivateResult, DocumentListParams, DocumentListResult, OpenDocument,
    };
    let reply = |value: Result<Value, serde_json::Error>| value.map_err(|e| format!("{id}: {e}"));
    if id == "document.list" {
        let DocumentListParams {} = read_params(id, p)?;
        let documents: Vec<OpenDocument> = open_sessions(app)
            .into_iter()
            .enumerate()
            .map(|(i, s)| OpenDocument {
                document_id: s.document_id.clone(),
                name: tab_name(app, s),
                // A caller kept to some folders is not told where a design outside them lives.
                path: s.path.clone().filter(|p| solidcraft_engine::io::vfs::check(p).is_ok()),
                active: i == app.docs.active,
                dirty: s.is_dirty(),
            })
            .collect();
        return reply(serde_json::to_value(DocumentListResult {
            active_document_id: app.session.document_id.clone(),
            documents,
            param_warnings: Vec::new(),
        }));
    }
    let request: DocumentActivateParams = read_params(id, p)?;
    let at = open_sessions(app)
        .iter()
        .position(|s| s.document_id == request.document_id)
        .ok_or("document.activate: no open design has that document_id (document.list names the open designs)")?;
    // The folder limits that opening the file would meet, as `reuse_path` applies them.
    if let Some(path) = open_sessions(app)[at].path.as_deref() {
        // A fixed message: `vfs::check`'s names the path, which `document.list` leaves out.
        if solidcraft_engine::io::vfs::check(path).is_err() {
            return Err("document.activate: that design is outside the folders this caller may use".into());
        }
    }
    if app.agent_chat.following_document && at != app.docs.active {
        return Err("the design is already open in another tab; select that tab to continue in its own conversation".into());
    }
    if at != app.docs.active {
        if app.docs.group.is_some() || app.docs.closing.is_some() || app.docs.quitting.is_some() || app.docs.batch.is_some() {
            return Err("finish the save prompt before showing another design".into());
        }
        switch(app, at);
        if app.docs.active != at {
            return Err("document.activate: the design could not be shown".into());
        }
    }
    let s = &app.session;
    reply(serde_json::to_value(DocumentActivateResult {
        document_id: s.document_id.clone(),
        name: tab_name(app, s),
        path: s.path.clone(),
        param_warnings: Vec::new(),
    }))
}

/// The active design as its file holds it (`document.export`): what a host that keeps the files
/// itself writes (#8517). `text` is the design's JSON, the same a `.json` design file holds, and
/// `revision` is the one `document.revision` names this state by.
pub fn export(app: &mut SolveApp) -> Result<Value, String> {
    let revision = app.session.design_revision();
    let design = app.session.design();
    let text = String::from_utf8(solidcraft_engine::io::write_design(&design).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
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

/// Close only the active clean design for the control API. The optional document_id
/// guards against a tab switch between inspection and closing (#18781).
pub(crate) fn close_clean(app: &mut SolveApp, p: &Value) -> Result<Value, String> {
    let expected = match p {
        Value::Null => None,
        Value::Object(fields) => {
            if let Some(key) = fields.keys().find(|key| key.as_str() != "document_id") {
                return Err(format!("unknown document.close field {key}"));
            }
            match fields.get("document_id") {
                None => None,
                Some(Value::String(id)) => Some(id.as_str()),
                _ => return Err("document_id must be a string".into()),
            }
        }
        _ => return Err("document.close params must be an object".into()),
    };
    if let Some(id) = expected
        && id != app.session.document_id
    {
        return Err("the active document changed; nothing was closed".into());
    }
    if app.file_picker_busy()
        || app.file_load.busy()
        || app.saving.is_some()
        || app.docs.closing.is_some()
        || app.docs.quitting.is_some()
        || app.docs.batch.is_some()
        || app.docs.group.is_some()
    {
        return Err("finish the file operation or save prompt before closing a document".into());
    }
    if app.docs.count() == 1 && active_is_blank(app) {
        return Err("no open design to close".into());
    }
    if dirty(app, app.docs.active) {
        return Err("the design has unsaved changes; save it before document.close".into());
    }
    let closed = app.session.document_id.clone();
    close(app, app.docs.active, false);
    if app.session.document_id == closed {
        return Err("the active design could not be closed".into());
    }
    Ok(json!({"closed_document_id": closed, "document_id": app.session.document_id, "documents": app.docs.count()}))
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
    remember_views(app, i);
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
        give_up_renames_of_other_designs(app);
        app.session.revision = rev + 1;
        crate::prefs::apply_new_design(app);
        app.cam = Camera::default();
        app.fit_view();
        put_view(app, DocView::default());
        app.docs = Documents::default();
        app.edit_in_place = Default::default();
        app.home.open = app.preferences.show_home_on_startup;
        crate::plugin_events::observe(app);
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
    if app.preferences.agent_shared_session {
        crate::agent_chat::leave_document(app);
        app.docs.parked_agent_chat = None;
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
    if let Some(chat) = app.docs.parked_agent_chat.as_mut() {
        chat.disconnect_document(&mut app.session, wait);
    }
    for slot in app.docs.background_mut() {
        slot.agent_chat.disconnect_document(&mut slot.session, wait);
    }
}

/// Stop only the hidden conversations whose endpoint permission was revoked.
pub(crate) fn disconnect_background_endpoint(app: &mut SolveApp, id: &str) {
    if let Some(chat) = app.docs.parked_agent_chat.as_mut() {
        chat.disconnect_endpoint(&mut app.session, id);
    }
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
            for i in 0..app.docs.count() {
                remember_views(app, i);
            }
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

/// Closing tab `i` asks to save first: it has changes, and is not a STEP, IGES or mesh file
/// opened and only looked at (#15111), whose file still holds all of it.
fn dirty(app: &SolveApp, i: usize) -> bool {
    let asks = |s: &Session| s.is_dirty() && !s.unchanged_import();
    if i == app.docs.active || app.docs.slots.is_empty() {
        return asks(&app.session);
    }
    app.docs.slots.get(i).and_then(Option::as_ref).is_some_and(|s| asks(&s.session))
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
    let child_title = app.view_windows.is_child().then(|| {
        let locale = solidcraft_ui_text::Locale::for_language(&app.preferences.language);
        let window = if app.preferences.clear_view_tabs {
            crate::view_windows::window_name(app, app.view_windows.current_window())
        } else {
            locale.given_name(&app.docs.views.active().name)
        };
        format!("{} · {window}", locale.given_name(&app.session.doc.name))
    });
    let name = child_title.as_deref().or_else(|| (!no_design).then_some(app.session.doc.name.as_str()));
    if let Some(title) = app.window_title.update(name, app.session.is_dirty(), &app.preferences.language) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
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

/// The drawing each tab has open, by id and name, or null.
fn tab_drawings(app: &SolveApp) -> Vec<Value> {
    open_sessions(app)
        .into_iter()
        .map(|session| match session.drawing("ui.documents") {
            Ok(d) => json!({"id": solidcraft_engine::doc::ids::encode(d.id), "name": d.name}),
            Err(_) => Value::Null,
        })
        .collect()
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

/// Refuse a noninteractive close before changing tabs or creating a save prompt.
pub(crate) fn check_close_without_prompt(app: &SolveApp, index: Option<&Value>, force: bool) -> Result<(), String> {
    check_noninteractive_idle(app)?;
    let index = match index {
        None | Some(Value::Null) => app.docs.active,
        Some(value) => value.as_u64().and_then(|i| usize::try_from(i).ok()).ok_or("index must be a nonnegative tab index")?,
    };
    let tabs = tabs_info(app);
    let (name, dirty) = tabs.get(index).ok_or_else(|| format!("no design tab at index {index}"))?;
    if *dirty && !force {
        return Err(format!("design `{name}` has unsaved changes; save it first or use force: true to discard them"));
    }
    Ok(())
}

/// Refuse a noninteractive quit before changing the active tab or creating a save prompt.
pub(crate) fn check_quit_without_prompt(app: &SolveApp, force: bool) -> Result<(), String> {
    check_noninteractive_idle(app)?;
    if !force {
        let names: Vec<String> = tabs_info(app).into_iter().filter(|(_, dirty)| *dirty).map(|(name, _)| format!("`{name}`")).collect();
        if !names.is_empty() {
            return Err(format!("designs with unsaved changes: {}; save them first or use force: true to discard them", names.join(", ")));
        }
    }
    Ok(())
}

/// A programmatic close must not interrupt an in-flight file operation or a person's save prompt.
fn check_noninteractive_idle(app: &SolveApp) -> Result<(), String> {
    if app.file_picker_busy() || app.file_load.busy() || app.docs.closing.is_some() || app.docs.quitting.is_some() || app.docs.batch.is_some() {
        return Err("a file is being opened or saved, or designs are being closed: try again".into());
    }
    Ok(())
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
        Some("duplicate") => duplicate(app, i.unwrap_or(app.docs.active)),
        Some("close_all") => close_many(app, (0..app.docs.count()).collect()),
        Some("move") => {
            let at = |k: &str| p.get(k).and_then(Value::as_u64).map(|x| x as usize);
            if let (Some(from), Some(to)) = (at("from"), at("to")) {
                move_tab(app, from, to);
                crate::tab_group_ui::dropped(app, to);
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
    let tabs: Vec<Value> = tabs_info(app)
        .into_iter()
        .zip(tab_drawings(app))
        .map(|((name, dirty), drawing)| json!({"name": name, "dirty": dirty, "drawing": drawing}))
        .collect();
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
    view: "tab:view:",
    view_close: "tab:view_close:",
    view_new: "tab:view_new",
    view_more: "tab:view_more",
    switcher: "tab:switcher",
    group: "tab:group:",
};

/// How far below the bar (points) a design tab is released to tear it off: the distance
/// [`crate::new_window::drag_out_is_tear_off`] uses.
const TEAR_OFF_BELOW: f32 = 40.0;

/// The footer of a design's view switcher (option B), under the list of its views.
const SWITCHER_MENU: [crate::app_bar::TabMenuRow<'static>; 4] = [
    crate::app_bar::TabMenuRow::new(crate::design_view_ui::SWITCHER_OPEN, "Open another view", "Ctrl+T"),
    crate::design_view_ui::MOVE_TO_WINDOW_MENU,
    crate::app_bar::TabMenuRow::new(crate::design_view_ui::SWITCHER_REOPEN, "Reopen closed view", "Ctrl+Shift+T"),
    crate::app_bar::TabMenuRow::new(crate::design_view_ui::SWITCHER_AS_TABS, "Show views as tabs", ""),
];

/// A view's name for [`crate::workspace::view_icon_colour`] and [`crate::workspace::view_accent_tab`].
pub(crate) fn kind_word(kind: ViewKind) -> &'static str {
    match kind {
        ViewKind::Model => "Model",
        ViewKind::Sheet => "Sheet",
        ViewKind::Render => "Render",
        ViewKind::Animation => "Animation",
    }
}

/// A view's hover text: "Model 2 · Design workspace · Alt+2" (the key for the first nine views).
fn view_tip(view: &DesignView, place: usize) -> String {
    let workspace = solidcraft_ui_text::tr(match view.kind {
        ViewKind::Model => "tab_strip.workspace_design",
        ViewKind::Sheet => "tab_strip.workspace_drawing",
        ViewKind::Render => "tab_strip.workspace_render",
        ViewKind::Animation => "tab_strip.workspace_animation",
    });
    if place < 9 { format!("{} · {workspace} · Alt+{}", view.name, place + 1) } else { format!("{} · {workspace}", view.name) }
}

/// A view as the strip draws it: its icon in the view's colour (the bar is dark in every theme)
/// and the same colour under it while it is shown.
fn view_tab<'a>(
    view: &'a DesignView,
    sketch_mark: bool,
    active: bool,
    on_screen: bool,
    tip: &'a str,
    to_window: Option<u32>,
    window: Option<&'a crate::view_windows::WindowChoice>,
) -> crate::app_bar::ViewTab<'a> {
    let sketch_mode = sketch_mark && view.editing.is_some();
    let colour =
        if sketch_mode { solidcraft_theme::Tokens::get().sketch_accent } else { crate::workspace::view_icon_colour(kind_word(view.kind), true) };
    crate::app_bar::ViewTab {
        sketch_mode,
        compass: view.kind.can_tile().then_some(view.compass),
        name: &view.name,
        icon: view.kind.key(),
        accent: colour,
        underline: colour,
        state: crate::app_bar::ViewState::Normal,
        active,
        on_screen,
        tip,
        to_window,
        window_name: window.and_then(|window| window.custom_name.as_deref()),
        window_number: window.map(|window| window.number),
    }
}

/// The translated labels of a tab's Rename, Duplicate and Save rows, and the Save As shortcut
/// shown beside it.
struct TabMenuText {
    rename: String,
    duplicate: String,
    save: String,
    save_as: String,
    save_as_key: String,
}

/// A tab's menu: rename its design, open a copy of it, save it, save it under another name, close
/// it, the others, or all of them. Save and Save As show their shortcuts on the shown design's tab,
/// where they apply.
fn tab_menu(text: &TabMenuText) -> [crate::app_bar::TabMenuRow<'_>; 7] {
    [
        crate::app_bar::TabMenuRow::new("ui.renameTab", &text.rename, ""),
        crate::app_bar::TabMenuRow::new("ui.duplicateTab", &text.duplicate, ""),
        crate::app_bar::TabMenuRow::new("ui.saveTab", &text.save, "Ctrl+S"),
        crate::app_bar::TabMenuRow::new("ui.saveTabAs", &text.save_as, &text.save_as_key),
        crate::app_bar::TabMenuRow::new("ui.closeTab", "Close", "Ctrl+W"),
        crate::app_bar::TabMenuRow::new("ui.closeOtherTabs", "Close Other Tabs", "").needing_others(),
        crate::app_bar::TabMenuRow::new("ui.closeAllTabs", "Close All Tabs", ""),
    ]
}

pub(crate) fn strip_view_id(app: &SolveApp, index: usize) -> Option<u64> {
    crate::view_windows::strip_views(app).get(index).map(|(_, view)| view.id)
}

/// The document tabs in the application bar, from `x0` to `x1` ([`crate::app_bar::tabs`]): click
/// to switch, × to close, drag to reorder, + for the start page.
pub fn tabs(app: &mut SolveApp, ui: &mut egui::Ui, r: Rect, x0: f32, x1: f32) {
    crate::sketch_view::project(app);
    let owner_key = tab_strip_id(ui.ctx().viewport_id()).with("view_popup_owner");
    let changed = ui.ctx().data_mut(|data| {
        let previous = data.get_temp::<u64>(owner_key);
        data.insert_temp(owner_key, app.session.uid);
        previous.is_some_and(|owner| owner != app.session.uid)
    });
    if changed {
        crate::app_bar::clear_view_popup(ui.ctx(), tab_strip_id(ui.ctx().viewport_id()));
    }
    let child = app.view_windows.is_child();
    let info = if child { vec![(app.session.doc.name.clone(), app.session.is_dirty())] } else { tabs_info(app) };
    let paths = if child { vec![app.session.path.clone()] } else { tab_paths(app) };
    let n = info.len();
    let locale = solidcraft_ui_text::Locale::for_language(&app.preferences.language);
    let names: Vec<String> = info.iter().map(|(name, _)| locale.given_name(name)).collect();
    let security = if child { vec![None] } else { tab_security(app) };
    let badges = if child { vec![None] } else { tab_badges(app) };
    let places: Vec<String> = info
        .iter()
        .zip(&paths)
        .zip(&security)
        .map(|(((_, dirty), path), security)| tab_place(path.as_deref(), *dirty, security.as_deref()))
        .collect();
    let selected = if child || app.docs.slots.is_empty() { 0 } else { app.docs.active };
    let mut rename = app.menu.rename.take();
    let renaming = renaming_tab_state(rename.as_ref());
    crate::view_windows::remember_strip_order(app);
    let new_tip = app.tr("ui.start_page").to_string();
    let menu_text = TabMenuText {
        rename: app.tr("ui.rename_design_tab").to_string(),
        duplicate: app.tr("ui.duplicate_design_tab").to_string(),
        save: app.tr("ui.save").to_string(),
        save_as: format!("{}…", app.tr("ui.save_as")),
        save_as_key: crate::keymap::effective(app, "file.save_as").unwrap_or_default(),
    };
    let menu = tab_menu(&menu_text);
    let views_as_tabs = child || DesignTabPrefs::from(&app.preferences).views_as_tabs || !app.docs.views.windows().is_empty();
    let current_window = app.view_windows.current_window();
    let design_views: Vec<Vec<_>> = (0..n)
        .map(|design| {
            if design == selected {
                let set = &app.docs.views;
                let tiles = set.tiles().filter(|_| set.is_tiled());
                let strip = crate::view_windows::strip_views(app);
                // The sketch mark on a view tab only tells views apart: with one view, none.
                let sketch_mark = crate::ui_kit::disambiguates(strip.len());
                strip
                    .into_iter()
                    .map(|(window, view)| {
                        let elsewhere = (window != current_window).then_some(window);
                        let active = elsewhere.is_none() && view.id == set.active().id;
                        let on_screen = elsewhere.is_none() && tiles.is_some_and(|tiles| tiles.contains(view.id));
                        (
                            {
                                let mut view = view.clone();
                                if sketch_mark
                                    && view.editing.is_some()
                                    && let Some(name) = crate::sketch_view::name(app, view.id)
                                {
                                    view.name = name;
                                }
                                view
                            },
                            active,
                            on_screen,
                            elsewhere,
                        )
                    })
                    .collect()
            } else {
                views_info(app, design).into_iter().map(|(view, active)| (view, active, false, None)).collect()
            }
        })
        .collect();
    // The tab groups: their labels and menus, each tab's group rows, and a name being typed.
    let group_parts = crate::tab_group_ui::StripGroups::build(app);
    let group_menus = group_parts.group_menus();
    let group_labels = group_parts.labels(&group_menus);
    let tab_menus = group_parts.tab_menus();
    let mut group_edit = crate::tab_group_ui::take_rename(app);
    // Local views expose their editor in both the switcher and sub-tab presentations.
    let mut view_edit = app.docs.view_rename.take().filter(|r| !app.home.open && app.docs.views.get(r.view).is_some());
    let view_place = view_edit.as_ref().and_then(|r| design_views.get(selected)?.iter().position(|(view, _, _, _)| view.id == r.view));
    let window_choices = crate::view_windows::window_choices(app);
    let view_tips: Vec<Vec<String>> = design_views
        .iter()
        .map(|views| {
            let mut local_place = 0;
            views
                .iter()
                .map(|(view, _, _, window)| {
                    let other = window.and_then(|id| window_choices.iter().find(|choice| choice.id == id));
                    if let Some(other) = other {
                        locale.format("view_tab.elsewhere", &[("view", &view.name), ("window", &other.name)])
                    } else if window.is_some() {
                        format!("{} · {}", view.name, app.tr("multi_window.window_mark"))
                    } else {
                        let tip = view_tip(view, local_place);
                        local_place += 1;
                        tip
                    }
                })
                .collect()
        })
        .collect();
    let view_tabs: Vec<Vec<crate::app_bar::ViewTab>> = design_views
        .iter()
        .zip(&view_tips)
        .map(|(views, tips)| {
            let sketch_mark = crate::ui_kit::disambiguates(views.len());
            views
                .iter()
                .zip(tips)
                .map(|((view, active, on_screen, window), tip)| {
                    let choice = window.and_then(|id| window_choices.iter().find(|choice| choice.id == id));
                    view_tab(view, sketch_mark, *active, *on_screen, tip, *window, choice)
                })
                .collect()
        })
        .collect();
    let more_text = solidcraft_ui_text::tr("tab_strip.more");
    let sheet_entries = crate::sheet_views::menu_entries(app);
    let mut sheet_rows: Vec<_> = sheet_entries.iter().map(|(id, label)| crate::app_bar::TabMenuRow::new(id, label, "")).collect();
    sheet_rows.push(crate::app_bar::TabMenuRow::new("sheet.new", app.tr("sheet_views.new"), "").with_icon("plus", None).separated());
    let plus_menu: Vec<_> = crate::design_view_ui::plus_menu(solidcraft_theme::is_dark())
        .into_iter()
        .map(|row| if row.id == "plus.sheet" { row.with_children(&sheet_rows) } else { row })
        .collect();
    let window_entries = crate::design_view_ui::window_menu_entries(app);
    let window_rows = crate::design_view_ui::window_menu_rows(app, &window_entries);
    let view_menu = crate::design_view_ui::view_menu_rows(app, &window_rows);
    let switcher_menu: Vec<_> = SWITCHER_MENU
        .iter()
        .copied()
        .filter(|row| row.id != crate::design_view_ui::MOVE_TO_WINDOW_MENU.id || crate::view_windows::supports_pop_out(app))
        .map(|row| if row.id == crate::design_view_ui::MOVE_TO_WINDOW_MENU.id { row.with_children(&window_rows) } else { row })
        .collect();
    let layout_menu = crate::design_view_ui::layout_menu();
    let view_layout = app.preferences.clear_view_tabs.then(|| crate::design_view_ui::window_layout(app));
    let layout_current = if app.docs.views.is_tiled() {
        match app.docs.views.tiles().and_then(solidcraft_views::Tiles::preset_kind) {
            Some(solidcraft_views::Preset::SideBySide) => "layout.side_by_side",
            Some(solidcraft_views::Preset::Stacked) => "layout.stacked",
            Some(solidcraft_views::Preset::LargeAndTwo) => "layout.large_and_two",
            Some(solidcraft_views::Preset::Grid) => "layout.grid",
            _ => "",
        }
    } else {
        "layout.single"
    };
    let (actions, drop_zones) = {
        let mut strip = crate::app_bar::TabStrip {
            tabs: info
                .iter()
                .zip(&names)
                .zip(&places)
                .zip(&badges)
                .zip(&view_tabs)
                .zip(group_parts.of_tab.iter().chain(std::iter::repeat(&None)))
                .zip(tab_menus.iter().map(Vec::as_slice).chain(std::iter::repeat(&[][..])))
                .map(|(((((((list_name, dirty), name), place), security), views), group), extra_menu)| crate::app_bar::DocTab {
                    name,
                    list_name,
                    dirty: *dirty,
                    place,
                    security: *security,
                    views,
                    group: *group,
                    extra_menu,
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
            menu: &menu,
            close_name: solidcraft_ui_text::tr("ui.close_tab"),
            close_tip: "Close",
            more_tip: solidcraft_ui_text::tr_label("All open designs"),
            new_tip: &new_tip,
            id: tab_strip_id(ui.ctx().viewport_id()),
            handles: TAB_HANDLES,
            views_as_tabs,
            switcher_always: DesignTabPrefs::from(&app.preferences).switcher_always(),
            clear_view_tabs: app.preferences.clear_view_tabs,
            pop_out: child,
            view_text: crate::app_bar::ViewText {
                close_view: solidcraft_ui_text::tr_label("Close view"),
                close_design: solidcraft_ui_text::tr_label("Close design"),
                new_view: solidcraft_ui_text::tr_label("Open another view"),
                more: more_text,
                switch_view: solidcraft_ui_text::tr("tab_strip.switch_view"),
            },
            view_menu: &view_menu,
            view_right_row: crate::design_view_ui::VIEW_CLOSE_RIGHT,
            view_rename_row: crate::design_view_ui::VIEW_RENAME,
            view_rename: view_edit
                .as_mut()
                .zip(view_place)
                .map(|(r, place)| (place, crate::app_bar::TabRename { text: &mut r.text, focused: &mut r.focused, max: solidcraft_views::MAX_NAME })),
            plus_menu: &plus_menu,
            layout_menu: &layout_menu,
            view_layout: view_layout.as_ref(),
            layout_current,
            layout_tip: solidcraft_ui_text::tr_label("Show views together"),
            layout_fill: app.docs.layout_fill.key(),
            layout_fill_heading: solidcraft_ui_text::tr_label("Empty cells show"),
            switcher_menu: &switcher_menu,
            tear_off_below: TEAR_OFF_BELOW,
            groups: &group_labels,
            group_rename: crate::tab_group_ui::rename_field(&group_parts, group_edit.as_mut()),
        };
        if child {
            let width = crate::app_bar::preferred_pill_width(ui, &strip);
            let key = strip.id.with("preferred_width");
            let changed = ui.ctx().data_mut(|data| {
                let previous = data.get_temp::<f32>(key);
                data.insert_temp(key, width);
                previous != Some(width)
            });
            if changed {
                ui.ctx().request_repaint();
            }
        }
        let linked = crate::view_window_drag::hovered_view(app)
            .and_then(|id| design_views.get(selected)?.iter().position(|(view, _, _, window)| view.id == id && window.is_none()));
        let detached = design_views.get(selected).and_then(|views| {
            views.iter().position(|(view, _, _, window)| window.is_none() && crate::view_window_drag::unanchored(app, ui.ctx(), view.id))
        });
        ui.ctx().data_mut(|data| {
            let detached_key = strip.id.with("unanchored_view");
            if let Some(detached) = detached {
                data.insert_temp(detached_key, detached);
            } else {
                data.remove::<usize>(detached_key);
            }
            let key = strip.id.with("linked_view_hover");
            if let Some(linked) = linked {
                data.insert_temp(key, linked);
            } else {
                data.remove::<usize>(key);
            }
        });
        crate::app_bar::tabs_with_view_drop_zones(ui, &mut strip, r, x0, x1)
    };
    let drop_zones = drop_zones
        .into_iter()
        .filter_map(|zone| {
            let tab = if child || zone.document == selected { app.docs.current_tab() } else { app.docs.slots.get(zone.document)?.as_ref()?.tab };
            let count = design_views.get(zone.document)?.iter().filter(|(_, _, _, window)| window.is_none()).count();
            Some((tab, count, zone))
        })
        .collect();
    // The id outside `data`: a context lock taken inside another can freeze the window (#21091).
    let hover_id = tab_strip_id(ui.ctx().viewport_id()).with("view_hover");
    let hovered = ui.ctx().data(|data| data.get_temp::<usize>(hover_id)).and_then(|view| strip_view_id(app, view));
    crate::view_window_drag::hover(app, ui.ctx(), hovered);
    let drag_continues = !app.file_load_holds() && actions.iter().any(|action| matches!(action, crate::app_bar::TabAction::DockView { .. }));
    crate::view_window_drag::record(app, ui.ctx(), drop_zones, drag_continues);
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
        (Some(true), Some(mut done)) => {
            // A refused name keeps the field open, focused with the name selected; the status
            // line says why (a name another component has takes a number, which can make it too
            // long).
            if names.get(selected).is_some_and(|shown| done.text.trim() != shown.trim()) && crate::context_menu::commit_rename(app, &done).is_err() {
                done.focused = false;
                app.menu.rename = Some(done);
            }
        }
        (Some(false), _) | (_, None) => {}
        (None, Some(editing)) => app.menu.rename = Some(editing),
    }
    // The same for the name typed into a view: kept on Enter, given up on Escape, else still typed.
    let view_kept = actions.iter().find_map(|a| match a {
        crate::app_bar::TabAction::ViewRenameDone { .. } => Some(true),
        crate::app_bar::TabAction::ViewRenameCancelled => Some(false),
        _ => None,
    });
    match (view_kept, view_edit) {
        // A refused name (control characters pasted in) keeps the field open with the name
        // selected, as the tab does; a blank one gives up, as it does for the tab.
        (Some(true), Some(mut done)) => {
            if !done.text.trim().is_empty() && !view_rename(app, done.view, &done.text) {
                let why = app.tr("tab_strip.view_name_refused").to_string();
                app.set_status(why, true);
                done.focused = false;
                app.docs.view_rename = Some(done);
            }
        }
        (None, Some(editing)) => app.docs.view_rename = Some(editing),
        _ => {}
    }
    // A file loading into the window keeps the tabs as they are (dragging still moves it).
    let idle = !app.file_load_holds();
    let (mut shut, mut pick, mut many, mut dropped, mut renamed, mut copied, mut saved) = (None, None, None, None, None, None, None);
    crate::tab_group_ui::finish_rename(app, group_edit, &actions);
    let mut window_drop = false;
    for action in actions {
        if idle && crate::tab_group_ui::handle(app, &group_parts, &action) {
            continue;
        }
        match action {
            crate::app_bar::TabAction::Select(i) => pick = Some(i),
            crate::app_bar::TabAction::Close(i) => shut = Some(i),
            crate::app_bar::TabAction::Menu { tab, id } => match id.as_str() {
                "ui.renameTab" => renamed = Some(tab),
                "ui.duplicateTab" => copied = Some(tab),
                "ui.saveTab" => saved = Some((tab, false)),
                "ui.saveTabAs" => saved = Some((tab, true)),
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
            crate::app_bar::TabAction::RenameDone
            | crate::app_bar::TabAction::RenameCancelled
            | crate::app_bar::TabAction::ViewRenameDone { .. }
            | crate::app_bar::TabAction::ViewRenameCancelled => {}
            crate::app_bar::TabAction::DockView { view, origin, pointer, released } if idle => {
                if let Some(id) = strip_view_id(app, view) {
                    crate::view_window_drag::route(app, ui.ctx(), id, origin, pointer, released);
                    window_drop = true;
                }
            }
            crate::app_bar::TabAction::LiftView { view, origin } if idle => {
                if let Some(id) = strip_view_id(app, view) {
                    crate::view_window_drag::lift(app, ui.ctx(), id, origin);
                }
            }
            crate::app_bar::TabAction::ReorderView { .. } if window_drop => {}
            view_action => {
                if idle {
                    crate::design_view_ui::handle(app, ui.ctx(), view_action);
                }
            }
        }
    }
    crate::view_setups_ui::show(app, ui.ctx());
    crate::view_window_drag::keyboard(app, ui.ctx());
    match (shut, pick) {
        _ if !idle => {}
        _ if many.is_some() => close_many(app, many.unwrap_or_default()),
        (Some(i), _) => close(app, i, false),
        (None, Some(i)) => switch(app, i),
        (None, None) => {
            if let Some((from, to)) = dropped {
                move_tab(app, from, to);
                crate::tab_group_ui::dropped(app, to);
            }
        }
    }
    if let Some(tab) = copied.filter(|_| idle) {
        duplicate(app, tab);
    }
    if let Some((tab, as_new)) = saved.filter(|_| idle) {
        save_tab(app, tab, as_new);
    }
    // Rename from the menu: the design's tab is shown first (over the start page too), so its
    // name is edited in place.
    if let Some(tab) = renamed.filter(|_| idle) {
        if tab != selected || app.home.open {
            switch(app, tab);
        }
        let shown = if app.docs.slots.is_empty() { 0 } else { app.docs.active };
        if let Some((name, _)) = info.get(tab).filter(|_| shown == tab && !app.home.open) {
            start_tab_rename(app, name, Rect::ZERO);
        }
    }
}

/// Save design `i` from its tab's menu, as File › Save does (`as_new`: as File › Save As does).
/// Another tab's design is shown first, as the "save changes?" prompt does before it saves one,
/// so a question Save asks (where to save a new design, a read-only or changed file) is about the
/// design in view.
fn save_tab(app: &mut SolveApp, i: usize, as_new: bool) {
    if app.file_picker_busy() || i >= app.docs.count() || app.docs.group.is_some() {
        return;
    }
    let shown = if app.docs.slots.is_empty() { 0 } else { app.docs.active };
    if i != shown || app.home.open {
        switch(app, i);
    }
    if as_new {
        crate::toolbar::save_as(app);
    } else {
        crate::toolbar::save(app);
    }
}

/// Open a copy of design `i` in a new tab right after it, and show the copy: its design as it
/// is now (unsaved changes, parameters, components and what is hidden included), named
/// "<name> copy", with the same view. The copy has no file yet, so it counts as unsaved and
/// closing it asks first. Its evaluated model goes with it when it is current, so a large design
/// is not evaluated again; otherwise the copy is evaluated as an opened file is.
pub fn duplicate(app: &mut SolveApp, i: usize) {
    // A pending close, quit, batch or group save holds tab numbers that a new tab would shift.
    if app.file_picker_busy()
        || i >= app.docs.count()
        || app.docs.closing.is_some()
        || app.docs.quitting.is_some()
        || app.docs.batch.is_some()
        || app.docs.group.is_some()
    {
        return;
    }
    let shown = if app.docs.slots.is_empty() { 0 } else { app.docs.active };
    if i != shown || app.home.open {
        switch(app, i);
    }
    let source = &app.session;
    let mut doc = (*source.doc).clone();
    let saved = source.model.saved(&source.doc);
    doc.visibility = source.visibility.clone();
    doc.name = solidcraft_ui_text::format("document.copy_name", &[("name", &doc.name)]);
    let mut copy = Session::opened_validating(doc, saved.as_ref(), source.validation());
    copy.mark_unsaved();
    let cam = app.cam;
    if let Err(e) = open_tab(app, copy) {
        app.set_status(e, true);
        return;
    }
    app.cancel_view_animation();
    app.cam = cam;
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
        session: app.session.uid,
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

/// File > Revert to Saved: read the active design again from the file it was opened or saved as.
/// With unsaved changes it asks first ([`revert_prompt`]), as they cannot be undone afterwards.
pub fn revert(app: &mut SolveApp) {
    if app.session.path.is_none() {
        return;
    }
    if app.session.is_dirty() {
        app.docs.reverting = true;
    } else {
        revert_now(app);
    }
}

/// Close the active tab without asking and open its file again in its place. The file is checked
/// first, so a missing or unreadable one leaves the tab and its changes where they were.
fn revert_now(app: &mut SolveApp) {
    app.docs.reverting = false;
    let Some(path) = app.session.path.clone() else { return };
    if app.docs.group.is_some() || app.docs.closing.is_some() || app.docs.quitting.is_some() || app.docs.batch.is_some() {
        return;
    }
    if let Err(e) = solidcraft_engine::io::vfs::check(&path) {
        app.set_status(e.to_string(), true);
        return;
    }
    if !solidcraft_engine::io::vfs::is_file(&path) {
        app.set_status(format!("{path}: the file is not there to revert to"), true);
        return;
    }
    let active = app.docs.active;
    close(app, active, true);
    app.open_path_async(&path);
}

/// The question File > Revert to Saved asks when the design has unsaved changes.
fn revert_prompt(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.docs.reverting {
        return;
    }
    let name = tabs_info(app).get(app.docs.active).map(|x| x.0.clone()).unwrap_or_default();
    let question = solidcraft_ui_text::format("document.revert_changes", &[("name", &crate::i18n::tr_name(&name))]);
    let (closed, action) = ui_kit::Confirm::new(egui::Id::unique("sc_revert_prompt"), "status_warning", solidcraft_ui_text::tr("ui.revert_question"))
        .tone(ui_kit::Tone::Warning)
        .actions(solidcraft_ui_text::tr("ui.revert"), solidcraft_ui_text::tr("ui.cancel"), None)
        .show(ctx, true, |ui| {
            ui_kit::text(ui, &question, ui_kit::Text::Report("question", false));
        });
    match action {
        ui_kit::Action::Ok => revert_now(app),
        ui_kit::Action::Cancel => app.docs.reverting = false,
        _ if closed => app.docs.reverting = false,
        _ => {}
    }
}

/// The "save changes?" prompt for a tab being closed, or for each unsaved design when quitting.
pub fn prompt(app: &mut SolveApp, ctx: &egui::Context) {
    let _language = solidcraft_ui_text::scope(&app.preferences.language);
    revert_prompt(app, ctx);
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
    let detail = if grouped { String::new() } else { crate::design_view_ui::closing_detail(view_counts(app).get(i).copied().unwrap_or(1)) };
    let save_label = solidcraft_ui_text::tr(if grouped { "document.save_selected" } else { "ui.save" });
    let mut show_tab = None;
    let mut row_action = None;
    let (closed, action) = ui_kit::Confirm::new(egui::Id::unique("sc_close_prompt"), "status_warning", &title)
        .tone(ui_kit::Tone::Warning)
        .actions(save_label, solidcraft_ui_text::tr("ui.cancel"), Some(solidcraft_ui_text::tr("ui.don_t_save")))
        .secondary_left()
        .detail(&detail)
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

/// Whether a tab is waiting for its save before closing.
pub(crate) fn saving_to_close(app: &SolveApp) -> bool {
    app.docs.saving
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

/// Files in any open tab, including linked parts, must not disappear underneath it.
pub(crate) fn uses_file(app: &SolveApp, path: &str) -> bool {
    solidcraft_engine::session_uses_file(&app.session, path)
        || app.docs.slots.iter().flatten().any(|slot| solidcraft_engine::session_uses_file(&slot.session, path))
}

/// The last measured width of a pop-out's complete view strip, before labels collapse.
/// Until the first layout, reserve all available space for the views.
pub(crate) fn preferred_tab_width(ctx: &egui::Context) -> f32 {
    let id = tab_strip_id(ctx.viewport_id()).with("preferred_width");
    ctx.data(|data| data.get_temp::<f32>(id)).unwrap_or(f32::INFINITY)
}

/// Where SolidCraft's document tab strip keeps its state.
pub(crate) fn tab_strip_id(viewport: egui::ViewportId) -> egui::Id {
    egui::Id::unique(("sc_doc_tabs", viewport))
}

/// Whether the document tab strip's list of every open design is open.
pub fn tab_list_open(ctx: &egui::Context) -> bool {
    crate::app_bar::tab_list_open(ctx, tab_strip_id(ctx.viewport_id()))
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
    fn unsaved_view_can_move_windows_without_replacing_its_session() {
        let mut app = SolveApp::new(Session::default(), Default::default());
        let uid = app.session.uid;
        let revision = app.session.revision;
        let model = app.docs.views.active().id;
        let other = view_duplicate(&mut app, model).unwrap();
        let tab = app.docs.active_tab();
        app.ui.visual_style = 2;
        app.ui.perspective = true;
        let mut child = detach_view(&mut app, other).unwrap();
        assert!(app.session.path.is_none());
        assert_eq!(app.docs.views.active().id, model);
        park_active_look(&mut app);
        exchange_window_views(&mut app, tab, &mut child);
        show_active_look(&mut app);
        assert_eq!(app.docs.views.active().id, other);
        assert_eq!(app.ui.visual_style, 2);
        assert!(app.ui.perspective);
        assert_eq!((app.session.uid, app.session.revision), (uid, revision));
        park_active_look(&mut app);
        exchange_window_views(&mut app, tab, &mut child);
        show_active_look(&mut app);
        assert_eq!(app.ui.visual_style, 0);
        assert!(!app.ui.perspective);
        reattach_window_views(&mut app, tab, child);
        assert!(app.docs.views.get(other).is_some());
        assert_eq!(app.docs.views.active().id, model);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn named_layout_reuses_live_views_and_keeps_extra_views_in_main() {
        let mut app = SolveApp::new(Session::default(), Default::default());
        app.preferences.clear_view_tabs = true;
        let first = app.docs.views.active().id;
        let second = view_duplicate(&mut app, first).unwrap();
        let third = view_duplicate(&mut app, second).unwrap();
        assert!(view_rename(&mut app, third, "Original angle"));
        let window = app.run("tab.view.move_to_window", json!({"view":third})).unwrap()["window"].clone();
        let migrated = crate::prefs::lenient(&json!({"pop_out_window_defaults":{"card_headers":"hover","keep_on_top":true}}));
        assert!(migrated.pop_out_window_defaults.ribbon && migrated.pop_out_window_defaults.ribbon_auto_hide);
        assert_eq!(migrated.pop_out_window_defaults.card_headers, solidcraft_views::CardHeaders::Hover);
        let options = app.run("tab.window.options", json!({"window":window})).unwrap()["options"].clone();
        assert_eq!(
            options,
            json!({"card_headers":"always","interface_size":100,"keep_on_top":false,"panels":false,"ribbon":true,"ribbon_auto_hide":true,"top_bar":"always"})
        );
        let edited = app
            .run(
                "tab.window.set_options",
                json!({"window":window,"card_headers":"hidden","interface_size":150,"keep_on_top":true,"panels":true,"ribbon":false,"ribbon_auto_hide":false,"top_bar":"edge"}),
            )
            .unwrap();
        app.run("window.options.use_as_default", json!({"window":window})).unwrap();
        assert_eq!(serde_json::to_value(app.preferences.pop_out_window_defaults).unwrap(), edited["options"]);
        app.run("tab.view.layout", json!({"window":0,"layout":"side_by_side"})).unwrap();
        assert_eq!(app.run("tab.layout.save", json!({"name":"Side monitors"})).unwrap()["name"], "Side monitors");
        assert_eq!(app.run("tab.layout.list", json!({})).unwrap()["layouts"], json!(["Side monitors"]));
        assert_eq!(crate::view_setups::matching_name(&app).as_deref(), Some("Side monitors"));
        let stored = serde_json::to_value(&app.preferences).unwrap();
        assert!(!stored["view_window_setups"].to_string().contains("Original angle"));
        let prefs = crate::prefs::lenient(&stored);

        let mut target = SolveApp::new(Session::default(), Default::default());
        target.preferences = prefs;
        let uid = target.session.uid;
        let main = target.docs.views.active().id;
        let a = view_duplicate(&mut target, main).unwrap();
        let b = view_duplicate(&mut target, a).unwrap();
        let extra = view_duplicate(&mut target, b).unwrap();
        assert!(view_rename(&mut target, b, "Keep this name"));
        target.run("tab.layout.apply", json!({"name":"Side monitors"})).unwrap();
        assert_eq!(target.session.uid, uid);
        assert_eq!(target.docs.views.views().iter().map(|view| view.id).collect::<Vec<_>>(), [main, a, extra]);
        assert_eq!(target.docs.views.tiles().unwrap().views(), [main, a]);
        let child = target.docs.views.windows().first().unwrap();
        assert_eq!(child.views.ids().collect::<Vec<_>>(), [b]);
        assert_eq!(child.views.active().name, "Keep this name");
        let id = child.id;
        assert_eq!(serde_json::to_value(child.options).unwrap(), edited["options"]);
        target.run("tab.window.set_options", json!({"window":id,"keep_on_top":false})).unwrap();
        assert_eq!(crate::view_setups::matching_name(&target), None);
        target.run("tab.layout.apply", json!({"name":"Side monitors"})).unwrap();
        assert_eq!(target.docs.views.windows().first().unwrap().id, id);
        assert_eq!(target.run("tab.window.options", json!({"window":id})).unwrap()["options"], edited["options"]);
        assert!(target.docs.views.check());
        assert_eq!(target.run("tab.layout.rename", json!({"name":"Side monitors","new_name":"Review"})).unwrap()["new_name"], "Review");
        assert_eq!(target.run("tab.layout.list", json!({})).unwrap()["layouts"], json!(["Review"]));
        assert_eq!(target.run("tab.layout.remove", json!({"name":"Review"})).unwrap()["name"], "Review");
        assert_eq!(target.run("tab.layout.list", json!({})).unwrap()["layouts"], json!([]));
        target.run("tab.window.close", json!({"window":id})).unwrap();
        assert_eq!(target.run("tab.window.options", json!({"window":id})).unwrap_err(), "window_not_found");
        let reopened = target.run("tab.view.move_to_window", json!({"view":b})).unwrap()["window"].clone();
        assert_eq!(target.run("tab.window.options", json!({"window":reopened})).unwrap()["options"], edited["options"]);
    }

    #[test]
    fn malformed_saved_layout_is_hidden_and_cannot_change_the_design() {
        let mut app = SolveApp::new(Session::default(), Default::default());
        app.preferences.view_window_setups.insert(
            "Broken".to_owned(),
            json!({"windows":[{
                "views":1, "level":"full", "placement":null, "tiles":{"view":3}
            }]}),
        );
        let before = app.docs.views.clone();
        assert_eq!(app.run("tab.layout.list", json!({})).unwrap()["layouts"], json!([]));
        assert_eq!(app.run("tab.layout.apply", json!({"name":"Broken"})).unwrap_err(), "no_such_layout");
        assert_eq!(app.docs.views, before);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn pop_out_commands_share_the_design_and_work_in_a_virtual_window() {
        let mut app = SolveApp::new(Session::default(), Default::default());
        let uid = app.session.uid;
        let first = app.docs.views.active().id;
        assert_eq!(app.run("tab.view.move_to_window", json!({"view":first})).unwrap_err(), "last_view_of_main_window");
        let second = view_duplicate(&mut app, first).unwrap();
        let moved = app.run("tab.view.move_to_window", json!({"view":second})).unwrap();
        let window = moved["window"].as_u64().unwrap();
        assert_eq!(moved["placement"], Value::Null);
        let listed = app.run("tab.window.list", json!({})).unwrap();
        assert_eq!(listed["windows"][0]["views"], json!([first]));
        assert_eq!(listed["windows"][1]["views"], json!([second]));
        assert_eq!(listed["windows"][1]["display"], Value::Null);
        app.run("tab.view.activate", json!({"id":second,"window":window})).unwrap();
        assert_eq!(app.run("tab.view.activate", json!({"index":0,"window":window})).unwrap()["id"], second);
        assert_eq!(app.run("tab.view.activate", json!({"index":0})).unwrap()["id"], second);
        assert_eq!(app.docs.views.active().id, first);
        assert_eq!(app.run("tab.window.close", json!({"window":window})).unwrap()["returned"], json!([second]));
        assert_eq!(app.session.uid, uid);
        assert!(app.session.path.is_none());
        assert_eq!(app.docs.views.views().iter().map(|view| view.id).collect::<Vec<_>>(), [first, second]);
        assert!(app.docs.views.check());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn detached_view_keeps_its_tab_position_until_an_explicit_reorder() {
        let mut app = SolveApp::new(Session::default(), Default::default());
        app.preferences.clear_view_tabs = true;
        let first = app.docs.views.active().id;
        let middle = view_duplicate(&mut app, first).unwrap();
        let last = view_duplicate(&mut app, middle).unwrap();
        let ids = |app: &SolveApp| crate::view_windows::strip_views(app).into_iter().map(|(_, view)| view.id).collect::<Vec<_>>();

        let moved = app.run("tab.view.move_to_window", json!({"view":middle})).unwrap();
        assert_eq!(ids(&app), [first, middle, last]);
        assert_eq!(moved["window"], 1);
        view_move(&mut app, 0, 1);
        assert_eq!(ids(&app), [last, middle, first]);
        app.run("tab.view.move_to_window", json!({"view":middle,"window":0})).unwrap();
        assert_eq!(ids(&app), [last, middle, first]);
        assert_eq!(app.docs.views.active().id, middle);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn layouts_and_commands_target_the_requested_design_window() {
        let mut app = SolveApp::new(Session::default(), Default::default());
        let main = app.docs.views.active().id;
        let first = view_duplicate(&mut app, main).unwrap();
        let second = view_duplicate(&mut app, main).unwrap();
        let first_window = app.run("tab.view.move_to_window", json!({"view":first})).unwrap()["window"].clone();
        let second_window = app.run("tab.view.move_to_window", json!({"view":second})).unwrap()["window"].clone();
        assert_ne!(first_window, second_window);

        let opened = app.run("tab.view.open", json!({"window":first_window,"kind":"model"})).unwrap()["id"].clone();
        assert_eq!(app.run("tab.view.layout", json!({"window":first_window,"layout":"stacked"})).unwrap()["shown"], 2);
        assert_eq!(app.run("tab.view.beside", json!({})).unwrap()["shown"], 2);
        assert_eq!(app.run("tab.view.layout", json!({"window":900,"layout":"single"})).unwrap_err(), "no_such_window");

        let listed = app.run("tab.window.list", json!({})).unwrap();
        assert_eq!(listed["windows"][0]["views"], json!([main]));
        assert_eq!(listed["windows"][0]["layout"], "single");
        assert_ne!(listed["windows"][1]["layout"], "single");
        assert_ne!(listed["windows"][2]["layout"], "single");
        assert!(listed["windows"][1]["views"].as_array().unwrap().contains(&opened));

        let moved = app.run("tab.view.move_to_window", json!({"view":second,"window":first_window})).unwrap();
        assert_eq!(moved["from"], second_window);
        assert_eq!(moved["window"], first_window);
        assert_eq!(moved["closed_source"], false);
        let moved = app.run("tab.view.move_to_window", json!({"view":second,"window":0})).unwrap();
        assert_eq!(moved["window"], 0);
        assert_eq!(app.docs.views.active().id, second);
        assert!(app.docs.views.check());
    }

    #[test]
    fn layout_menu_commands_link_visible_cameras_without_changing_their_projection() {
        let mut app = SolveApp::new(Session::default(), Default::default());
        app.run("tab.view.layout", json!({"layout":"side_by_side","window":0})).unwrap();
        let active = app.docs.views.active().id;
        let other = visible_view_ids(&app).into_iter().find(|id| *id != active).unwrap();
        let mut perspective = app.docs.views.get(other).unwrap().spec.unwrap().camera;
        perspective.fov = 0.5;
        app.docs.views.set_spec(other, solidcraft_viewport::ViewSpec::new(perspective));
        let initial_angle = perspective.yaw;
        let hidden = view_duplicate(&mut app, active).unwrap();
        assert!(focus_view(&mut app, active));
        let hidden_spec = app.docs.views.get(hidden).unwrap().spec;

        app.cam.set_view(solidcraft_viewport::StandardView::Left);
        app.run("tab.view.link", json!({"window":0,"mode":"orientation"})).unwrap();
        let changed = app.docs.views.get(other).unwrap().spec.unwrap().camera;
        assert_ne!(changed.yaw, initial_angle);
        assert_eq!((changed.yaw, changed.pitch), (app.cam.yaw, app.cam.pitch));
        assert_eq!(changed.distance, perspective.distance);
        assert_eq!(changed.fov, perspective.fov);
        assert_eq!(app.docs.views.get(hidden).unwrap().spec, hidden_spec);

        app.run("tab.view.link", json!({"window":0,"mode":"zoom"})).unwrap();
        app.cam.set_view(solidcraft_viewport::StandardView::Top);
        app.cam.zoom_at(0.5, None);
        sync_linked_active(&mut app);
        let zoomed = app.docs.views.get(other).unwrap().spec.unwrap().camera;
        assert_eq!(zoomed.yaw, changed.yaw);
        assert_eq!(zoomed.fov, perspective.fov);
        assert!((zoomed.half_height() - app.cam.half_height()).abs() < 1e-9);
        assert_eq!(app.run("tab.view.equal_sizes", json!({"window":0})).unwrap()["shown"], 2);
        assert_eq!(app.run("tab.view.reset_cameras", json!({"window":0})).unwrap()["shown"], 2);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn clean_window_only_changes_the_requested_window() {
        let mut app = SolveApp::new(Session::default(), Default::default());
        let main = app.docs.views.active().id;
        let other = view_duplicate(&mut app, main).unwrap();
        let child = app.run("tab.view.move_to_window", json!({"view":other})).unwrap()["window"].clone();
        assert_eq!(app.run("tab.window.clean", json!({"window":child,"on":true})).unwrap()["on"], true);
        let levels = app.run("tab.window.list", json!({})).unwrap();
        assert_eq!(levels["windows"][0]["level"], "full");
        assert_eq!(levels["windows"][1]["level"], "clean");
        assert_eq!(app.run("tab.window.clean", json!({"window":child})).unwrap()["on"], false);
        assert_eq!(app.run("tab.window.list", json!({})).unwrap()["windows"][1]["level"], "views_only");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn drag_placement_uses_the_release_display_and_menu_placement_cascades() {
        let mut app = SolveApp::new(Session::default(), Default::default());
        let ctx = egui::Context::default();
        ctx.set_embed_viewports(false);
        crate::view_windows::prepare(&mut app, &ctx);
        crate::view_windows::refresh_displays(&mut app, || {
            vec![
                crate::panel_runtime::NativeMonitor {
                    name: Some("primary".into()),
                    bounds: egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1920.0, 1080.0)),
                    scale: 1.0,
                    primary: true,
                },
                crate::panel_runtime::NativeMonitor {
                    name: Some("second".into()),
                    bounds: egui::Rect::from_min_max(egui::pos2(-1200.0, 0.0), egui::pos2(0.0, 1000.0)),
                    scale: 1.25,
                    primary: false,
                },
            ]
        });
        let main = app.docs.views.active().id;
        let first = view_duplicate(&mut app, main).unwrap();
        crate::view_windows::drag_drop(&mut app, first, None, Some(egui::pos2(-600.0, 650.0))).unwrap();
        let second = view_duplicate(&mut app, main).unwrap();
        app.run("tab.view.move_to_window", json!({"view":second})).unwrap();
        let windows = app.run("tab.window.list", json!({})).unwrap();
        assert_eq!(windows["windows"][1]["display"], "second");
        assert_eq!(windows["windows"][1]["placement"], json!({"x":80.0,"y":240.0,"w":800.0,"h":560.0,"maximised":false,"fullscreen":false}));
        assert_eq!(windows["windows"][2]["display"], "primary");
        assert_eq!(windows["windows"][2]["placement"]["x"], 592.0);
        assert_eq!(windows["windows"][2]["placement"]["y"], 292.0);
    }

    #[test]
    fn preparing_view_windows_reads_native_scale_in_a_frame() {
        let mut app = SolveApp::new(Session::default(), Default::default());
        let context = egui::Context::default();
        let mut input = egui::RawInput::default();
        input.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().native_pixels_per_point = Some(1.5);
        let mut prepared = false;
        let _ = context.run_ui(input, |ui| {
            crate::view_windows::prepare(&mut app, ui.ctx());
            prepared = true;
        });
        assert!(prepared);
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

    /// A design's link to a network location not allowed is never resolved to see whether it is
    /// an open design (Edit in Place asks that before the "network location" question).
    #[test]
    fn saved_path_identity_never_resolves_a_refused_network_path() {
        let dir = std::env::temp_dir().join(format!("solidcraft-tab-paths-net-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let open = dir.join("assembly.solidcraft").to_string_lossy().into_owned();
        let calls = solidcraft_engine::io::vfs::file_calls();
        for link in [r"\\server\share\part.solidcraft", "//server/share/part.solidcraft", r"\\.\pipe\part.solidcraft"] {
            assert!(!same_path(&open, link), "{link}");
            assert!(!same_path(link, &open), "{link}");
        }
        assert_eq!(solidcraft_engine::io::vfs::file_calls(), calls, "a refused path was resolved");
        // Two paths on the same server (a design opened from it) are still compared.
        assert!(same_path(r"\\files\share\a.solidcraft", r"\\files\share\a.solidcraft"));
        std::fs::remove_dir_all(dir).unwrap();
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

    /// The field and the command both count characters: a hundred `ł` (200 bytes) is taken.
    #[test]
    fn a_long_name_with_accented_letters_is_taken() {
        let mut h = design_open();
        double_click_tab(&mut h, 0);
        h.step(&json!({"text": "ł".repeat(100)})).unwrap();
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.session.doc.name, "ł".repeat(100));
        assert!(!renaming_tab(&h.app), "the field closed");
    }

    /// A refused name keeps the tab's field open with the name selected: the design takes a
    /// number when a component already has the typed name, and a 128-character name with a
    /// number is too long.
    #[test]
    fn a_refused_tab_name_keeps_the_field_open() {
        let mut h = design_open();
        let taken = "x".repeat(128);
        h.app.run("component.create", json!({"name": taken, "activate": false})).unwrap();
        let before = h.app.session.doc.name.clone();
        double_click_tab(&mut h, 0);
        h.step(&json!({"text": taken})).unwrap();
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert!(renaming_tab(&h.app), "the field stays open after a refused name");
        assert_eq!(h.app.session.doc.name, before);
        let (text, _, error) = h.app.status.clone().expect("the refusal is shown");
        assert!(error && text.contains("1…128 characters"), "{text}");
        h.step(&json!({"text": "Short"})).unwrap();
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.session.doc.name, "Short", "the next keystrokes replace the name");
        assert!(!renaming_tab(&h.app));
    }

    /// A refused tab name is given up when another design is shown: it never lands on that
    /// design's tab, and Enter there renames nothing.
    #[test]
    fn a_refused_tab_name_is_given_up_when_another_tab_is_shown() {
        let mut h = design_open();
        open_tab(&mut h.app, Session::default()).unwrap();
        h.frames(2);
        let taken = "x".repeat(128);
        h.app.run("component.create", json!({"name": taken, "activate": false})).unwrap();
        let names = |h: &crate::scenario::Harness| tabs_info(&h.app).into_iter().map(|(name, _)| name).collect::<Vec<_>>();
        let before = names(&h);
        double_click_tab(&mut h, 1);
        h.step(&json!({"text": taken})).unwrap();
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert!(renaming_tab(&h.app), "refused, so the field stays");
        h.step(&json!({"click": {"handle": "tab:design:0"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.docs.active, 0);
        assert!(!renaming_tab(&h.app), "the refused name is given up with its design");
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert_eq!(names(&h), before, "no design was renamed");
    }

    /// A refused tab name is given up when the last design is closed: it never lands on the new
    /// design that takes its place.
    #[test]
    fn a_refused_tab_name_is_given_up_when_the_last_design_closes() {
        let mut h = design_open();
        let taken = "x".repeat(128);
        h.app.run("component.create", json!({"name": taken, "activate": false})).unwrap();
        double_click_tab(&mut h, 0);
        h.step(&json!({"text": taken})).unwrap();
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert!(renaming_tab(&h.app), "refused, so the field stays");
        close(&mut h.app, 0, true);
        h.frames(2);
        assert!(!renaming_tab(&h.app), "the refused name is given up with its design");
        let fresh = h.app.session.doc.name.clone();
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.session.doc.name, fresh, "the new design is not renamed");
    }

    /// Clicking away from a refused tab name commits it again, is refused again, and keeps the
    /// field, as the other rename editors do.
    #[test]
    fn clicking_away_from_a_refused_tab_name_keeps_the_field() {
        let mut h = design_open();
        let taken = "x".repeat(128);
        h.app.run("component.create", json!({"name": taken, "activate": false})).unwrap();
        let before = h.app.session.doc.name.clone();
        double_click_tab(&mut h, 0);
        h.step(&json!({"text": taken})).unwrap();
        h.step(&json!({"click": [700, 500]})).unwrap();
        h.frames(2);
        assert!(renaming_tab(&h.app), "the field stays after a refused click-away");
        assert_eq!(h.app.session.doc.name, before);
    }

    /// A view's refused name (control characters pasted in) keeps its sub-tab's field open with
    /// the name selected, as the design tab does; the next keystrokes replace it.
    #[test]
    fn a_refused_view_name_keeps_the_field_open() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.design_views_as_tabs = true;
        h.app.home.open = false;
        let call =
            |h: &mut crate::scenario::Harness, command: &str, params: Value| h.call("engine.execute", json!({"command": command, "params": params}));
        let id = call(&mut h, "tab.view.open", json!({"kind": "animation"}))["result"]["id"].as_u64().unwrap();
        assert_eq!(call(&mut h, "tab.view.rename", json!({}))["ok"], true);
        h.frames(2);
        if let Some(r) = h.app.docs.view_rename.as_mut() {
            r.text = "Bad\u{7}name".into();
        }
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert!(h.app.docs.view_rename.is_some(), "the field stays open after a refused name");
        let (text, _, error) = h.app.status.clone().expect("the refusal is shown");
        assert!(error && text.contains("control characters"), "{text}");
        assert_eq!(h.app.docs.views.get(id).map(|v| v.name.as_str()), Some("Animation 1"));
        h.step(&json!({"text": "Good"})).unwrap();
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.docs.views.get(id).map(|v| v.name.as_str()), Some("Good"), "the next keystrokes replace the name");
        assert!(h.app.docs.view_rename.is_none());
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

    #[test]
    fn closed_picture_uploads_are_fresh_on_reopen_while_live_and_background_uploads_survive() {
        use std::sync::Arc;
        let mut app = app();
        app.home.open = false;
        let closed = app.docs.views.open(ViewKind::Model, None, false);
        let live = app.docs.views.open(ViewKind::Model, None, false);
        let mut closed_look = ViewLook::from_ui(&app.ui);
        closed_look.options.show_grid = false;
        closed_look.motion.spin = true;
        let old = closed_look.scenes.mark_uploaded_for_test(71);
        app.docs.navigation.insert(closed, closed_look);
        let mut live_look = ViewLook::from_ui(&app.ui);
        let kept = live_look.scenes.mark_uploaded_for_test(72);
        app.docs.navigation.insert(live, live_look);
        assert!(app.docs.views.close(closed));
        let mut background = DocSlot::new(Session::default());
        let background_id = (1u64 << 40) + 7;
        background.views =
            ViewSet::from_json(&json!({"views":[{"id":background_id,"kind":"model","name":"Background"}],"active":background_id})).unwrap();
        let mut background_look = ViewLook::from_ui(&app.ui);
        let hidden = background_look.scenes.mark_uploaded_for_test(73);
        background.navigation.insert(background_id, background_look);
        app.docs.slots = vec![None, Some(background)];
        app.docs.active = 0;
        let root_slots = app.viewport.upload_slots_for_test();
        let slots = live_view_picture_slots(&mut app);
        assert!(slots.contains(&0) && slots.contains(&live) && slots.contains(&background_id));
        assert!(!slots.contains(&closed));
        let cleared = &app.docs.navigation[&closed];
        let fresh = cleared.scenes.upload_slots_for_test();
        assert!(!cleared.options.show_grid && cleared.motion.spin, "closing keeps preferences and navigation state");
        for (old, fresh) in old.iter().zip(&fresh) {
            assert!(!Arc::ptr_eq(old, fresh), "Reopen must republish after the renderer released its consumed scene");
        }
        for (before, after) in kept.iter().zip(app.docs.navigation[&live].scenes.upload_slots_for_test()) {
            assert!(Arc::ptr_eq(before, &after), "a live view's consumed upload is still valid");
        }
        let background = app.docs.slots[1].as_ref().unwrap();
        for (before, after) in hidden.iter().zip(background.navigation[&background_id].scenes.upload_slots_for_test()) {
            assert!(Arc::ptr_eq(before, &after), "a background design's picture stays live");
        }
        for (before, after) in root_slots.iter().zip(app.viewport.upload_slots_for_test()) {
            assert!(Arc::ptr_eq(before, &after), "root's active CPU state is untouched");
        }
        let _ = live_view_picture_slots(&mut app);
        for (before, after) in fresh.iter().zip(app.docs.navigation[&closed].scenes.upload_slots_for_test()) {
            assert!(Arc::ptr_eq(before, &after), "a closed cache is retired once rather than reallocated each frame");
        }
        assert_eq!(view_reopen(&mut app), Some(closed));
        for (expected, actual) in fresh.iter().zip(app.viewport.upload_slots_for_test()) {
            assert!(Arc::ptr_eq(expected, &actual));
        }
        assert_eq!(app.viewport.upload_keys_for_test(), [0; 4], "reopening regenerates every consumed upload");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_window_ownership_retains_live_uploads_in_background_designs() {
        use std::sync::Arc;
        let mut app = app();
        let tab = app.docs.active_tab();
        let root = app.docs.views.active().id;
        let native = view_duplicate(&mut app, root).unwrap();
        let window = app.run("tab.view.move_to_window", json!({"view":native})).unwrap()["window"].as_u64().unwrap() as u32;
        let mut look = ViewLook::from_ui(&app.ui);
        let uploads = look.scenes.mark_uploaded_for_test(83);
        app.docs.navigation.insert(native, look);
        let live = live_view_picture_slots(&mut app);
        assert!(!live.contains(&native), "native views use isolated GPU slots");
        for (before, after) in uploads.iter().zip(app.docs.navigation[&native].scenes.upload_slots_for_test()) {
            assert!(Arc::ptr_eq(before, &after), "native views are still open");
        }
        assert_eq!(view_picture_window_owners(&app)[&(tab, window)], vec![native]);
        open_tab(&mut app, Session::default()).unwrap();
        assert_eq!(view_picture_window_owners(&app)[&(tab, window)], vec![native], "background designs keep their hidden hosts");
        let _ = live_view_picture_slots(&mut app);
        let background = app.docs.slots.iter().flatten().find(|slot| slot.tab == tab).unwrap();
        assert!(background.navigation.contains_key(&native));
        // A document switch intentionally invalidates its upload payloads, but the
        // liveness pass must preserve the resulting slots while its host is hidden.
        let before = background.navigation[&native].scenes.upload_slots_for_test();
        let _ = live_view_picture_slots(&mut app);
        let background = app.docs.slots.iter().flatten().find(|slot| slot.tab == tab).unwrap();
        for (before, after) in before.iter().zip(background.navigation[&native].scenes.upload_slots_for_test()) {
            assert!(Arc::ptr_eq(before, &after));
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

    /// #15111: a STEP file opened in a tab and only looked at closes without asking to save,
    /// although it was never saved as a design. Changed, it asks again.
    #[test]
    fn an_opened_step_file_closes_without_asking_until_it_is_changed() {
        let dir = std::env::temp_dir().join(format!("solidcraft-close-step-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let step = dir.join("Block.step");
        let mut maker = Session::default();
        maker.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        maker.execute("file.export", &json!({"path": step.to_string_lossy()})).unwrap();
        let opened = || {
            let mut s = Session::default();
            s.execute("doc.open", &json!({"path": step.to_string_lossy()})).unwrap();
            s
        };
        let mut a = app();
        open_tab(&mut a, opened()).unwrap();
        assert!(a.session.is_dirty() && a.session.path.is_none(), "still a design never saved");
        control(&mut a, &json!({"action": "close", "index": 1}));
        assert_eq!((a.docs.count(), a.docs.closing), (1, None), "closed without asking");

        open_tab(&mut a, opened()).unwrap();
        a.run("solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [30, 0, 0]})).unwrap();
        control(&mut a, &json!({"action": "close", "index": 1}));
        assert_eq!((a.docs.count(), a.docs.closing), (2, Some(1)), "a changed import asks first");
        std::fs::remove_dir_all(dir).unwrap();
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

    /// A design saved to a file, then changed in the app and not saved again.
    fn saved_then_changed(h: &mut crate::scenario::Harness) -> std::path::PathBuf {
        let file = h.dir.join("revert.solidcraft");
        h.app.run("solid.sphere", json!({"diameter":4})).unwrap();
        h.app.run("file.save_as", json!({"path":file.to_string_lossy()})).unwrap();
        h.app.run("solid.sphere", json!({"diameter":6})).unwrap();
        assert!(h.app.session.is_dirty());
        file
    }

    #[test]
    fn revert_to_saved_asks_first_and_then_brings_back_the_saved_design() {
        let mut h = crate::scenario::Harness::new();
        let file = saved_then_changed(&mut h);
        let saved = std::fs::read(&file).unwrap();
        assert_eq!(h.app.session.doc.features.len(), 2);
        crate::toolbar::FileAction::Revert.run(&mut h.app);
        assert!(h.app.docs.reverting, "unsaved changes are asked about, not dropped");
        assert_eq!(h.app.session.doc.features.len(), 2, "nothing is reloaded before the answer");
        revert_now(&mut h.app);
        h.settle();
        assert_eq!(h.app.session.doc.features.len(), 1, "the sphere added after the save is gone");
        assert!(!h.app.session.is_dirty() && !h.app.docs.reverting);
        assert_eq!(h.app.docs.count(), 1, "the tab was replaced, not joined by a second one");
        assert_eq!(h.app.session.path.as_deref().map(std::path::Path::new), Some(file.as_path()));
        assert_eq!(std::fs::read(&file).unwrap(), saved, "reverting never writes the file");
    }

    #[test]
    fn revert_to_saved_without_changes_reloads_what_another_program_wrote() {
        let mut h = crate::scenario::Harness::new();
        let file = h.dir.join("outside.solidcraft");
        h.app.run("solid.sphere", json!({"diameter":4})).unwrap();
        h.app.run("file.save_as", json!({"path":file.to_string_lossy()})).unwrap();
        // Another copy of the app adds a second feature to the same file.
        let mut other = Session::default();
        other.execute("doc.open", &json!({"path":file.to_string_lossy()})).unwrap();
        other.execute("solid.sphere", &json!({"diameter":9})).unwrap();
        other.execute("file.save_as", &json!({"path":file.to_string_lossy()})).unwrap();
        crate::toolbar::FileAction::Revert.run(&mut h.app);
        assert!(!h.app.docs.reverting, "nothing to lose, so nothing to ask");
        h.settle();
        assert_eq!(h.app.session.doc.features.len(), 2);
        assert_eq!(h.app.docs.count(), 1);
    }

    #[test]
    fn revert_to_saved_is_refused_without_a_file_and_keeps_changes_when_the_file_is_gone() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.sphere", json!({"diameter":4})).unwrap();
        assert!(!crate::toolbar::FileAction::Revert.enabled(&h.app), "a design with no file has nothing to revert to");
        crate::toolbar::FileAction::Revert.run(&mut h.app);
        assert!(!h.app.docs.reverting);
        let file = saved_then_changed(&mut h);
        std::fs::remove_file(&file).unwrap();
        revert_now(&mut h.app);
        h.settle();
        let (text, _, error) = h.app.status.clone().expect("a message");
        assert!(error && text.contains("not there"), "{text}");
        assert_eq!(h.app.session.doc.features.len(), 3, "the changes are still in the tab");
        assert_eq!(h.app.docs.count(), 1);
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

    /// Save in a tab's menu saves that design to its file, from another tab too (it is shown
    /// first); a design never saved asks where, as File › Save does. Save As always asks, and
    /// the design then lives in the new file.
    #[test]
    fn save_and_save_as_in_a_tabs_menu_save_that_design() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.custom_titlebar = true;
        h.call("ui.resize", json!({"width": 1400, "height": 720}));
        h.app.run("solid.box", json!({"length": 1, "width": 1, "height": 1})).unwrap();
        let first = h.dir.join("first.sc");
        h.app.run("file.save_as", json!({"path": first})).unwrap();
        h.app.run("solid.box", json!({"length": 2, "width": 2, "height": 2})).unwrap();
        assert!(h.app.session.is_dirty());
        open_tab(&mut h.app, Session::default()).unwrap();
        h.app.run("solid.box", json!({"length": 3, "width": 3, "height": 3})).unwrap();
        h.frames(3);
        let menu = |h: &mut crate::scenario::Harness, tab: usize, row: &str| {
            h.step(&json!({"click": {"handle": format!("tab:design:{tab}")}, "button": "right"})).unwrap();
            h.frames(2);
            h.step(&json!({"click": {"handle": format!("tab_menu:{row}")}})).unwrap();
            h.frames(2);
            h.settle();
        };
        // Save on the other tab's menu: that design is shown and written to its own file.
        menu(&mut h, 0, "ui.saveTab");
        assert_eq!(h.app.docs.active, 0);
        assert!(!h.app.session.is_dirty(), "saved");
        assert_eq!(h.app.session.path.as_deref(), first.to_str());
        let on_disk = std::fs::read(&first).unwrap();
        // Save on a design never saved asks where (the scenario's picker answers in its folder).
        menu(&mut h, 1, "ui.saveTab");
        assert_eq!(h.app.docs.active, 1);
        assert!(h.app.session.path.is_some() && !h.app.session.is_dirty(), "saved where the picker said");
        // Save As writes a new file and leaves the old one as it was.
        let before = h.app.session.path.clone();
        switch(&mut h.app, 0);
        h.app.run("component.rename", json!({"component": 0, "name": "Plate Mk2"})).unwrap();
        switch(&mut h.app, 1);
        h.frames(2);
        menu(&mut h, 0, "ui.saveTabAs");
        assert_eq!(h.app.docs.active, 0);
        assert_eq!(h.app.session.path.as_deref(), h.dir.join("Plate Mk2.sc").to_str(), "Save As offers the design's name");
        assert_eq!(std::fs::read(&first).unwrap(), on_disk, "the first file is kept as it was");
        switch(&mut h.app, 1);
        assert_eq!(h.app.session.path, before, "the other design keeps its own file");
        // Over the start page, Save shows the design before it saves it.
        h.app.run("solid.box", json!({"length": 4, "width": 4, "height": 4})).unwrap();
        h.app.home.open = true;
        h.frames(2);
        menu(&mut h, 1, "ui.saveTab");
        assert!(!h.app.home.open, "the start page gives way to the design");
        assert!(!h.app.session.is_dirty(), "and it is saved");
    }

    /// Duplicate in a tab's menu opens a copy of that design right after it: the same features
    /// and hidden bodies, named "<name> copy", unsaved, with the same view; the original is
    /// untouched, and an edit in the copy stays in the copy.
    #[test]
    fn duplicate_in_a_tabs_menu_opens_a_copy_after_it() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.custom_titlebar = true;
        h.call("ui.resize", json!({"width": 1400, "height": 720}));
        h.app.run("solid.box", json!({"length": 3, "width": 4, "height": 5, "body_name": "Plate"})).unwrap();
        h.app.run("component.rename", json!({"component": 0, "name": "Bracket"})).unwrap();
        h.app.run("solid.box", json!({"length": 1, "width": 1, "height": 1, "body_name": "Spare"})).unwrap();
        h.app.run("browser.visibility", json!({"items": ["Spare"], "visible": false})).unwrap();
        assert!(!h.app.session.body_visible("Spare"), "a body hidden in the original");
        open_tab(&mut h.app, Session::default()).unwrap();
        h.frames(3);
        switch(&mut h.app, 0);
        h.frames(2);
        let (features, hidden, cam) = (h.app.session.doc.features.clone(), h.app.session.visibility.clone(), h.app.cam);
        // From the other tab's menu: the original is shown first, and the copy lands after it.
        switch(&mut h.app, 1);
        h.frames(2);
        h.step(&json!({"click": {"handle": "tab:design:0"}, "button": "right"})).unwrap();
        h.frames(2);
        h.step(&json!({"click": {"handle": "tab_menu:ui.duplicateTab"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.docs.count(), 3);
        assert_eq!(h.app.docs.active, 1, "the copy is shown, right after its original");
        assert_eq!(h.app.session.doc.name, "Bracket copy");
        assert_eq!(h.app.session.doc.features, features, "the same design");
        assert_eq!(h.app.session.visibility, hidden, "with the same bodies hidden");
        assert!(h.app.session.path.is_none() && h.app.session.is_dirty(), "unsaved: closing it asks first");
        assert_eq!(h.app.cam, cam, "seen from the same view");
        assert_eq!(h.app.session.world_state().unwrap().bodies.len(), 2, "its bodies are there");
        // An edit in the copy stays in the copy.
        h.app.run("solid.box", json!({"length": 2, "width": 2, "height": 2, "body_name": "Extra"})).unwrap();
        switch(&mut h.app, 0);
        assert_eq!(h.app.session.doc.name, "Bracket");
        assert_eq!(h.app.session.doc.features, features, "the original is untouched");
    }

    /// `ui.documents {"action": "duplicate"}` does what the menu does, for scripts.
    #[test]
    fn the_control_channel_duplicates_a_design() {
        let mut a = app();
        a.run("solid.box", json!({"length": 1, "width": 1, "height": 1})).unwrap();
        let v = control(&mut a, &json!({"action": "duplicate", "index": 0}));
        assert_eq!(v["documents"].as_array().map(Vec::len), Some(2));
        assert_eq!(a.docs.active, 1);
        assert_eq!(a.session.doc.name, "Untitled copy");
        // While a close waits on its "save changes?" answer, no copy shifts the tabs it holds.
        a.docs.closing = Some(0);
        control(&mut a, &json!({"action": "duplicate", "index": 0}));
        assert_eq!(a.docs.count(), 2, "no copy while a close is pending");
        a.docs.closing = None;
    }

    /// The menu's Rename edits the design's name in its tab, as a double-click does; on a tab
    /// that isn't shown, that design is shown first. Enter keeps the name.
    #[test]
    fn rename_in_a_tabs_menu_edits_its_name_in_place() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.custom_titlebar = true;
        h.call("ui.resize", json!({"width": 1400, "height": 720}));
        h.app.run("solid.box", json!({"length": 1, "width": 1, "height": 1})).unwrap();
        open_tab(&mut h.app, Session::default()).unwrap();
        h.app.run("solid.box", json!({"length": 2, "width": 2, "height": 2})).unwrap();
        h.frames(3);
        assert_eq!(h.app.docs.active, 1);
        let other = h.app.docs.slots.first().and_then(Option::as_ref).map(|d| d.session.doc.name.clone()).unwrap();
        for (tab, name) in [(1, "Bracket"), (0, "Hinge")] {
            h.step(&json!({"click": {"handle": format!("tab:design:{tab}")}, "button": "right"})).unwrap();
            h.frames(2);
            h.step(&json!({"click": {"handle": "tab_menu:ui.renameTab"}})).unwrap();
            h.frames(2);
            assert_eq!(h.app.docs.active, tab, "the renamed design is the one shown");
            assert!(renaming_tab(&h.app), "its name is being edited in the tab");
            h.step(&json!({"text": name})).unwrap();
            h.step(&json!({"key": "Enter"})).unwrap();
            h.frames(2);
            assert_eq!(h.app.session.doc.name, name);
            assert!(!renaming_tab(&h.app));
        }
        // Each rename reached its own design only.
        let names: Vec<String> = tabs_info(&h.app).into_iter().map(|(name, _)| name).collect();
        assert_eq!(names, vec!["Hinge".to_string(), "Bracket".to_string()]);
        assert_ne!(other, "Hinge");
    }

    /// The menu's Rename is offered whatever the double-click preference says, and Escape keeps
    /// the old name.
    #[test]
    fn rename_in_the_menu_works_with_the_double_click_preference_off() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.preferences.tab_double_click_renames = false;
        h.app.run("solid.box", json!({"length": 1, "width": 1, "height": 1})).unwrap();
        let before = h.app.session.doc.name.clone();
        h.frames(3);
        h.step(&json!({"click": {"handle": "tab:design:0"}, "button": "right"})).unwrap();
        h.frames(2);
        h.step(&json!({"click": {"handle": "tab_menu:ui.renameTab"}})).unwrap();
        h.frames(2);
        assert!(renaming_tab(&h.app));
        h.step(&json!({"text": "Discarded"})).unwrap();
        h.step(&json!({"key": "Escape"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.session.doc.name, before);
        assert!(!renaming_tab(&h.app));
        // Over the start page, Rename shows the design and edits its name.
        h.app.home.open = true;
        h.frames(2);
        h.step(&json!({"click": {"handle": "tab:design:0"}, "button": "right"})).unwrap();
        h.frames(2);
        h.step(&json!({"click": {"handle": "tab_menu:ui.renameTab"}})).unwrap();
        h.frames(2);
        assert!(!h.app.home.open, "the start page gives way to the design");
        assert!(renaming_tab(&h.app), "its name is being edited in the tab");
    }

    /// Closing a batch through the tab menu must use the same final screen as closing one tab.
    #[test]
    fn close_all_tabs_respects_startup_choice_after_discard_and_on_repeat() {
        for (home, one_at_a_time) in [(true, true), (true, false), (false, true), (false, false)] {
            let mut a = four_tabs();
            a.preferences.show_home_on_startup = home;
            a.preferences.save_prompt_one_at_a_time = one_at_a_time;
            control(&mut a, &json!({"action": "close_all"}));
            assert_eq!(a.docs.closing, Some(if one_at_a_time { 2 } else { 0 }));
            if one_at_a_time {
                answer_prompt(&mut a, Answer::DontSave);
                assert_eq!(a.docs.closing, Some(0));
            }
            answer_prompt(&mut a, Answer::DontSave);
            assert_eq!(a.docs.count(), 1);
            assert!(active_is_blank(&a));
            assert_eq!(a.home.open, home);
            assert!(a.docs.batch.is_none() && a.docs.closing.is_none());
            control(&mut a, &json!({"action": "close_all"}));
            assert_eq!(a.docs.count(), 1);
            assert!(active_is_blank(&a));
            assert_eq!(a.home.open, home);
        }
    }

    #[test]
    fn last_document_and_repeated_untitled_closes_follow_home_preference() {
        for home in [true, false] {
            let mut a = app();
            a.preferences.show_home_on_startup = home;
            for _ in 0..3 {
                close(&mut a, 0, false);
                assert!(active_is_blank(&a));
                assert_eq!(a.home.open, home);
                assert_eq!(a.docs.count(), 1);
                assert!(!a.quit_requested);
            }
            // The explicit Home tab is still available, and quitting is still quitting.
            a.home.open = true;
            assert!(active_is_blank(&a));
            request_quit(&mut a);
            assert!(a.quit_requested);
        }
    }

    #[test]
    fn saving_or_discarding_last_document_returns_to_preferred_screen() {
        for home in [true, false] {
            for answer in [Answer::Save, Answer::DontSave] {
                let mut h = crate::scenario::Harness::new();
                let a = &mut h.app;
                a.preferences.show_home_on_startup = home;
                a.home.open = false;
                let path = h.dir.join("Close preference.solidcraft");
                a.run("solid.box", json!({"length":4,"width":5,"height":6})).unwrap();
                a.run("file.save_as", json!({"path":path.to_string_lossy()})).unwrap();
                a.run("solid.sphere", json!({"diameter":2})).unwrap();
                close(a, 0, false);
                assert_eq!(a.docs.closing, Some(0));
                assert!(!a.home.open);
                answer_prompt(a, answer);
                crate::save_job::tick(a, true);
                assert!(active_is_blank(a));
                assert_eq!(a.home.open, home);
                assert_eq!(a.docs.count(), 1);
                let saved = solidcraft_engine::io::read_design(&std::fs::read(path).unwrap()).unwrap();
                assert_eq!(saved.features.len(), if matches!(answer, Answer::Save) { 2 } else { 1 });
            }
        }
    }

    #[test]
    fn close_cancellation_and_cancelled_save_as_preserve_last_document() {
        for home in [true, false] {
            let mut a = app();
            a.preferences.show_home_on_startup = home;
            a.home.open = false;
            a.run("solid.box", json!({"length":4,"width":5,"height":6})).unwrap();
            let original = a.session.doc.to_json();
            close(&mut a, 0, false);
            answer_prompt(&mut a, Answer::Cancel);
            assert_eq!(a.session.doc.to_json(), original);
            assert!(a.session.is_dirty() && !a.home.open && a.docs.closing.is_none());
            a.services.pick_save = Some(Box::new(|_: &str, _: &[&str]| Box::pin(std::future::ready(None))));
            close(&mut a, 0, false);
            answer_prompt(&mut a, Answer::Save);
            crate::file_picker::tick(&mut a, &egui::Context::default());
            assert!(!a.file_picker_busy());
            assert_eq!(a.session.doc.to_json(), original);
            assert!(a.session.is_dirty() && !a.home.open && a.docs.closing.is_none());
        }
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

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn close_view_groups_stay_in_the_requested_window_and_keep_the_anchor() {
        let mut app = app();
        let root = app.docs.views.active().id;
        let second = app.run("tab.view.open", json!({"kind": "model"})).unwrap()["id"].as_u64().unwrap();
        let third = app.run("tab.view.open", json!({"kind": "model"})).unwrap()["id"].as_u64().unwrap();
        let remote = app.run("tab.view.open", json!({"kind": "model"})).unwrap()["id"].as_u64().unwrap();
        let window = app.run("tab.view.move_to_window", json!({"view": remote, "window": "new"})).unwrap()["window"].as_u64().unwrap();

        assert!(app.run("tab.view.close_right", json!({"id": second, "window": 0})).is_ok());
        assert_eq!(app.docs.views.views().iter().map(|view| view.id).collect::<Vec<_>>(), [root, second]);
        assert!(app.docs.views.get(third).is_none());
        assert!(app.run("tab.view.close_others", json!({"id": second, "window": window})).is_err());
        assert!(app.run("tab.view.close_others", json!({"id": second})).is_ok());
        assert_eq!(app.docs.views.views().iter().map(|view| view.id).collect::<Vec<_>>(), [second]);
        assert_eq!(app.docs.views.window_of(remote), u32::try_from(window).ok());
    }

    #[test]
    fn sheet_views_resolve_real_owners_reuse_locally_and_open_another_view() {
        let mut app = app();
        app.preferences.design_sub_tab_sheets = true;
        app.run("solid.box", json!({"length":10,"width":8,"height":2})).unwrap();
        app.run("drawing.create", json!({})).unwrap();
        let drawing = app.session.drawing("drawing.inspect").unwrap();
        let (owner, sheet) = (drawing.id, drawing.sheets[0].id);
        let subject = crate::sheet_views::subject(owner, sheet);
        assert_eq!(crate::sheet_views::resolve(&app, Some(&subject)).unwrap(), (owner, sheet));
        assert_eq!(crate::sheet_views::resolve(&app, Some(&solidcraft_engine::doc::ids::encode(sheet))).unwrap(), (owner, sheet));
        let original = crate::sheet_views::open_in(&mut app, owner, sheet, false).unwrap();
        assert_eq!(crate::sheet_views::open_in(&mut app, owner, sheet, false).unwrap(), original);
        let another = crate::sheet_views::open_in(&mut app, owner, sheet, true).unwrap();
        assert_ne!(original, another);
        assert_eq!(app.docs.views.get(another).unwrap().subject.as_deref(), Some(subject.as_str()));
        assert!(can_restore(&app.session, ViewKind::Sheet, Some(&subject)));
        assert!(crate::sheet_views::resolve(&app, Some("not-an-id")).is_err());
        let count = app.docs.views.len();
        assert!(app.run("tab.view.open", json!({"kind":"sheet","subject":"missing/sheet"})).is_err());
        assert_eq!(app.docs.views.len(), count);
        assert!(view_close(&mut app, another));
        assert!(app.session.doc.drawing(owner).unwrap().sheets.iter().any(|record| record.id == sheet));
    }

    #[test]
    fn opening_a_sheet_in_a_full_layout_parks_a_card_and_preserves_the_ground() {
        for ground_kind in [ViewKind::Model, ViewKind::Render] {
            let mut app = app();
            app.preferences.design_sub_tab_sheets = false;
            app.run("solid.box", json!({"length":10,"width":8,"height":2})).unwrap();
            app.run("drawing.create", json!({})).unwrap();
            let drawing = app.session.drawing("drawing.inspect").unwrap();
            let (owner, sheet) = (drawing.id, drawing.sheets[0].id);
            assert_eq!(app.docs.views.views().len(), 1, "the disabled preference does not open a Sheet view");
            assert_eq!(app.docs.views.active().kind, ViewKind::Model);
            app.preferences.design_sub_tab_sheets = true;
            let initial = app.docs.views.active().id;
            let ground = if ground_kind == ViewKind::Model { initial } else { app.docs.views.open(ground_kind, None, false) };
            if ground != initial {
                assert!(app.docs.views.close(initial));
            }
            for _ in 1..solidcraft_views::MAX_TILES {
                app.docs.views.open(ViewKind::Model, None, false);
            }
            app.docs.views.activate(ground);
            app.docs.views.set_layout(solidcraft_views::Preset::Grid);
            let before = app.docs.views.tiles().unwrap().views();
            let records = app.docs.views.views().to_vec();
            let opened = crate::sheet_views::open_in(&mut app, owner, sheet, true).unwrap();
            let after = app.docs.views.tiles().unwrap().views();
            assert_eq!(after.len(), solidcraft_views::MAX_TILES);
            assert!(after.contains(&ground) && after.contains(&opened));
            let parked: Vec<_> = before.iter().copied().filter(|id| !after.contains(id)).collect();
            assert_eq!(parked.len(), 1);
            assert_ne!(parked[0], ground);
            for record in records {
                assert_eq!(app.docs.views.get(record.id).unwrap().kind, record.kind);
                assert_eq!(after.contains(&record.id), record.id != parked[0]);
            }
            assert_eq!(app.docs.views.len(), solidcraft_views::MAX_TILES + 1);
            assert_eq!(app.docs.views.active().id, opened);
        }
    }

    /// A design's views: `tab.view.*` open, show, rename, move, close and reopen them (never the
    /// last one), each design keeps its own set, the keys are the commands' defaults, and a sketch
    /// entered anywhere in the app opens its view.
    #[test]
    fn the_views_of_a_design_follow_commands_keys_and_the_app() {
        let mut h = crate::scenario::Harness::new();
        // The classic switcher, which this test's keys cover: since #19655 a focused Sheet view
        // brings the Drawing workspace, where model shortcuts such as Ctrl+Shift+W wait, as on main.
        h.app.preferences.workspace_follows_view = false;
        h.app.preferences.design_views_as_tabs = true;
        h.app.home.open = false;
        let call =
            |h: &mut crate::scenario::Harness, command: &str, params: Value| h.call("engine.execute", json!({"command": command, "params": params}));
        let list = |h: &mut crate::scenario::Harness| -> Vec<(String, String, bool)> {
            let reply = call(h, "tab.view.list", json!({}));
            let views = reply["result"]["views"].as_array().cloned().unwrap_or_default();
            views.iter().map(|v| (v["kind"].as_str().unwrap().into(), v["name"].as_str().unwrap().into(), v["active"] == true)).collect()
        };
        let model = ("model".to_owned(), "Model".to_owned(), true);
        assert_eq!(list(&mut h), std::slice::from_ref(&model), "a new design has its Model view only");
        // Setup only: drawing.create refuses an empty design ("nothing to draw yet"), so the drawing needs a body to show.
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        h.app.run("drawing.create", json!({})).unwrap();
        let drawing = h.app.session.drawing("drawing.inspect").unwrap();
        let subject = crate::sheet_views::subject(drawing.id, drawing.sheets[0].id);
        h.app.run("drawing.close", json!({})).unwrap();
        // Setup only: the box and drawing made the design unsaved, and closing an unsaved design asks first; Ctrl+Shift+W below closes it outright.
        h.app.session.mark_saved();
        let opened = call(&mut h, "tab.view.open", json!({"kind": "sheet", "subject": subject}));
        assert_eq!(opened["ok"], true, "{opened}");
        let sheet = opened["result"]["id"].as_u64().unwrap();
        assert_eq!(call(&mut h, "tab.view.open", json!({"kind": "sheet", "subject": subject}))["result"]["id"], sheet, "the same sheet is reused");
        assert_eq!(call(&mut h, "tab.view.open", json!({"kind": "animation"}))["ok"], true);
        assert_eq!(h.app.docs.views.len(), 3);
        assert_eq!(list(&mut h)[2], ("animation".into(), "Animation 1".into(), true));
        // Show one by place, rename it, move it.
        assert_eq!(call(&mut h, "tab.view.activate", json!({"index": 1}))["result"]["id"], sheet);
        assert_eq!(call(&mut h, "tab.view.activate", json!({"index": 9}))["ok"], false);
        assert_eq!(call(&mut h, "tab.view.rename", json!({"name": " Drawing "}))["ok"], true);
        assert_eq!(call(&mut h, "tab.view.rename", json!({"name": "  "}))["ok"], false);
        // Without a name the window starts typing it in the view's own sub-tab (F2 and the menu row do).
        assert_eq!(call(&mut h, "tab.view.rename", json!({}))["ok"], true);
        assert_eq!(h.app.docs.view_rename.as_ref().map(|r| r.text.as_str()), Some("Drawing"));
        assert_eq!(call(&mut h, "tab.view.rename", json!({"id": 999}))["ok"], false);
        h.app.docs.view_rename = None;
        assert_eq!(call(&mut h, "tab.view.move", json!({"from": 1, "to": 0}))["ok"], true);
        assert_eq!(call(&mut h, "tab.view.move", json!({"from": 1, "to": 3}))["ok"], false);
        assert_eq!(list(&mut h)[0], ("sheet".into(), "Drawing".into(), true));
        // Each design has its own set; the badge counts come from them.
        open_tab(&mut h.app, Session::default()).unwrap();
        assert_eq!(view_counts(&h.app), [3, 1]);
        assert_eq!(views_info(&h.app, 0).len(), 3);
        switch(&mut h.app, 0);
        assert_eq!(view_counts(&h.app), [3, 1]);
        assert_eq!(views_info(&h.app, 1).len(), 1);
        // Close, never the last, and bring back.
        let ids: Vec<u64> = views_info(&h.app, 0).iter().map(|(view, _)| view.id).collect();
        assert_eq!(call(&mut h, "tab.view.close", json!({"id": ids[0]}))["ok"], true);
        assert_eq!(call(&mut h, "tab.view.close", json!({"id": 999}))["ok"], false);
        assert_eq!(call(&mut h, "tab.view.reopen", json!({}))["result"]["id"], ids[0]);
        assert_eq!(view_counts(&h.app), [3, 1]);
        for id in &ids[1..] {
            assert_eq!(call(&mut h, "tab.view.close", json!({"id": id}))["ok"], true);
        }
        assert_eq!(call(&mut h, "tab.view.close", json!({}))["ok"], false, "the last view stays");
        assert_eq!(call(&mut h, "tab.view.reopen", json!({}))["ok"], true);
        // Keys: the defaults, Ctrl+W on a view, Alt+1 to show the first one.
        assert_eq!(crate::keymap::effective(&h.app, "tab.view.open").as_deref(), Some("Ctrl+T"));
        assert_eq!(crate::keymap::effective(&h.app, "tab.view.reopen").as_deref(), Some("Ctrl+Shift+T"));
        h.call("ui.key", json!({"key": "T", "cmd": true}));
        assert_eq!(h.app.docs.views.len(), 3, "Ctrl+T opens another view of the active kind");
        h.call("ui.key", json!({"key": "W", "cmd": true}));
        assert_eq!((h.app.docs.views.len(), h.app.docs.count()), (2, 2), "Ctrl+W closes the view, not the design");
        h.call("ui.key", json!({"key": "T", "cmd": true, "shift": true}));
        assert_eq!(h.app.docs.views.len(), 3);
        h.call("ui.key", json!({"key": "1", "alt": true}));
        assert_eq!(h.app.docs.views.active().id, h.app.docs.views.views()[0].id);
        h.call("ui.key", json!({"key": "W", "cmd": true, "shift": true}));
        assert_eq!(h.app.docs.count(), 1, "Ctrl+Shift+W closes the design");
        // Sketch commands edit the current Model without creating a view.
        let before = h.app.docs.views.len();
        h.app.run("sketch.create", json!({"plane": "XY", "name": "Sketch1"})).unwrap();
        h.frames(2);
        assert!(h.app.session.active_sketch.is_some());
        let active = h.app.docs.views.active().clone();
        assert_eq!(active.kind, solidcraft_views::ViewKind::Model);
        assert_eq!(h.app.docs.views.len(), before);
        h.app.run("sketch.line", json!({"points": [[0, 0], [20, 10]]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.frames(2);
        assert_eq!(h.app.docs.views.active().kind, solidcraft_views::ViewKind::Model);
        assert_eq!(h.app.docs.views.len(), before, "finishing also leaves the view set alone");
        view_activate(&mut h.app, active.id);
        assert!(h.app.session.active_sketch.is_none(), "activating a Model never reopens a sketch");
    }

    /// Two Model views of one design are two angles: each keeps the camera it was left with, and a
    /// new one starts from where the design looks now.
    #[test]
    fn model_views_keep_their_own_angle() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let first = h.app.docs.views.active().id;
        h.app.cam.yaw = 0.25;
        let second = view_open(&mut h.app, ViewKind::Model, None, false);
        assert_ne!(first, second, "a second Model view is opened, not the first reused");
        assert_eq!(h.app.cam.yaw, 0.25, "a new view starts from the angle the design looks from");
        h.app.cam.yaw = 1.5;
        assert!(view_activate(&mut h.app, first));
        assert_eq!(h.app.cam.yaw, 0.25, "the first view comes back at its own angle");
        assert!(view_activate(&mut h.app, second));
        assert_eq!(h.app.cam.yaw, 1.5, "and the second at its own");
        let copy = view_duplicate(&mut h.app, second).unwrap();
        assert_eq!(h.app.cam.yaw, 1.5, "a copy starts at the angle of the view it copies");
        h.app.cam.yaw = -0.5;
        assert!(view_activate(&mut h.app, second));
        assert_eq!(h.app.cam.yaw, 1.5);
        assert!(view_activate(&mut h.app, copy));
        assert_eq!(h.app.cam.yaw, -0.5);
    }

    /// A layout opens the views it needs, and a view standing in for the shown one is drawn from its
    /// own angle and leaves the shown view's as it was.
    #[test]
    fn a_layout_opens_views_and_each_is_drawn_from_its_own_angle() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let first = h.app.docs.views.active().id;
        h.app.cam.yaw = 0.25;
        assert_eq!(view_layout(&mut h.app, solidcraft_views::Preset::SideBySide), 2);
        assert!(h.app.docs.views.is_tiled());
        assert_eq!(h.app.docs.views.active().id, first, "the view that was shown stays the focused one");
        let other = h.app.docs.views.tiles().unwrap().views().into_iter().find(|id| *id != first).unwrap();
        let yaw = with_view_look(&mut h.app, other, |app| {
            app.cam.yaw = 2.0;
            app.cam.yaw
        });
        assert_eq!(yaw, 2.0);
        assert_eq!(h.app.cam.yaw, 0.25, "the focused view's angle is back");
        assert!(focus_view(&mut h.app, other));
        assert_eq!(h.app.cam.yaw, 2.0, "the other view kept the angle it was orbited to");
        assert_eq!(view_layout(&mut h.app, solidcraft_views::Preset::Single), 1);
        assert!(!h.app.docs.views.is_tiled());
    }

    #[test]
    fn a_render_tile_keeps_its_camera_when_focused_beside_model() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let model = h.app.docs.views.active().id;
        h.app.cam.yaw = 0.25;
        let render = view_open(&mut h.app, ViewKind::Render, None, false);
        assert!(crate::render_workspace::active(&h.app));
        h.app.cam.yaw = 1.25;
        assert_eq!(view_beside(&mut h.app, Some(model), solidcraft_views::Edge::Left), Some(model));
        assert!(view_activate(&mut h.app, model));
        assert_eq!(h.app.cam.yaw, 0.25);

        crate::render_workspace::with_tile_camera(&mut h.app, |app| {
            assert!(crate::render_workspace::active(app));
            assert_eq!(app.cam.yaw, 1.25);
            app.cam.yaw = 1.75;
        });
        assert!(!crate::render_workspace::active(&h.app));
        assert_eq!(h.app.cam.yaw, 0.25);
        assert!(view_activate(&mut h.app, render));
        assert_eq!(h.app.cam.yaw, 1.75);
        assert!(crate::view_tiles::shown(&h.app));
        assert_eq!(view_layout(&mut h.app, solidcraft_views::Preset::Single), 1);
        assert!(!crate::view_tiles::shown(&h.app));
        assert!(crate::render_workspace::active(&h.app), "maximising keeps the Render workspace");
    }

    /// Each Model view keeps its own section and analysis too, so one design can be inspected two
    /// ways at once; the design itself is the same.
    #[test]
    fn model_views_keep_their_own_analysis() {
        use solidcraft_engine::SurfaceAnalysis;
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let first = h.app.docs.views.active().id;
        let second = view_open(&mut h.app, ViewKind::Model, None, false);
        h.app.session.analysis = Some(SurfaceAnalysis::Zebra { stripes: 8.0 });
        h.app.session.colored_components = Some(());
        assert!(view_activate(&mut h.app, first));
        assert!(h.app.session.colored_components.is_none());
        assert_eq!(h.app.session.analysis, None, "the first view never had the analysis");
        h.app.session.analysis = Some(SurfaceAnalysis::Environment);
        assert!(view_activate(&mut h.app, second));
        assert_eq!(h.app.session.analysis, Some(SurfaceAnalysis::Zebra { stripes: 8.0 }));
        assert!(h.app.session.colored_components.is_some());
        assert!(view_activate(&mut h.app, first));
        assert_eq!(h.app.session.analysis, Some(SurfaceAnalysis::Environment));
    }

    #[test]
    fn duplicating_an_inactive_view_copies_its_own_inspection_state() {
        use solidcraft_engine::SurfaceAnalysis;
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let source = h.app.docs.views.active().id;
        h.app.session.analysis = Some(SurfaceAnalysis::Zebra { stripes: 7.0 });
        h.app.session.colored_components = Some(());
        let other = view_open(&mut h.app, ViewKind::Model, None, false);
        h.app.session.analysis = Some(SurfaceAnalysis::Environment);
        h.app.session.colored_components = None;
        let copy = view_duplicate(&mut h.app, source).unwrap();
        assert_ne!(copy, source);
        assert_eq!(h.app.session.analysis, Some(SurfaceAnalysis::Zebra { stripes: 7.0 }));
        assert!(h.app.session.colored_components.is_some());
        assert!(view_activate(&mut h.app, other));
        assert_eq!(h.app.session.analysis, Some(SurfaceAnalysis::Environment));
        assert!(h.app.session.colored_components.is_none());
        assert!(view_activate(&mut h.app, copy));
        assert_eq!(h.app.session.analysis, Some(SurfaceAnalysis::Zebra { stripes: 7.0 }));
    }

    #[test]
    fn inspection_snapshots_drop_stale_sampled_geometry_but_keep_requested_settings() {
        use solidcore_record::Vec3;
        use solidcraft_engine::{CombPick, CombView, IsoPick, SurfaceAnalysis};
        let mut session = Session::default();
        session.analysis = Some(SurfaceAnalysis::Zebra { stripes: 7.0 });
        session.colored_components = Some(());
        session.isocurves = vec![IsoPick { body: "Body1".into(), at: Vec3::ZERO, count: 4 }];
        session.comb = Some(CombView {
            picks: vec![CombPick::Edge { at: Vec3::ZERO }],
            density: 4,
            length: 1.0,
            fixed_scale: None,
            scale: 1.0,
            teeth: vec![vec![[Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0)]]],
        });
        let saved = session.inspect();
        session.revision += 1;
        session.set_inspect(saved.clone());
        assert!(session.comb.is_some(), "a view-only revision preserves sampled geometry");
        session.doc_mut().name.push_str(" changed");
        session.set_inspect(saved);
        let comb = session.comb.as_ref().unwrap();
        assert!(comb.teeth.is_empty(), "a changed document cannot restore old comb teeth");
        assert_eq!(comb.density, 4, "the requested comb settings remain available");
        assert_eq!(comb.picks, vec![CombPick::Edge { at: Vec3::ZERO }]);
        assert!(session.continuity.is_empty());
        assert_eq!(session.analysis, Some(SurfaceAnalysis::Zebra { stripes: 7.0 }));
        assert!(session.colored_components.is_some());
        assert_eq!(session.isocurves.len(), 1, "face picks are resolved against the current model");
    }

    #[test]
    fn a_background_design_remembers_its_live_model_camera() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.preferences.design_reopen_views = true;
        let path = h.dir.join("background.solidcraft").to_string_lossy().into_owned();
        h.app.session.path = Some(path.clone());
        h.app.cam.yaw = 1.7;
        open_tab(&mut h.app, Session::default()).unwrap();
        let slot = h.app.docs.slots[0].as_mut().unwrap();
        let active = slot.views.active().id;
        slot.views.set_spec(active, solidcraft_viewport::ViewSpec::new(Camera::default()));
        let dir = h.dir.join("view-memory");
        remember_views_in(&h.app, 0, &dir);
        crate::view_memory::wait_for_pending().unwrap();
        let saved = crate::view_memory::recall(&dir, &path).unwrap();
        let views = ViewSet::from_json(&saved).unwrap();
        assert_eq!(views.active().spec.unwrap().camera.yaw, 1.7);
    }

    #[test]
    fn remembered_background_views_apply_their_camera_when_first_shown() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.preferences.design_reopen_views = true;
        let path = h.dir.join("restored.solidcraft").to_string_lossy().into_owned();
        let mut incoming = Session::default();
        incoming.path = Some(path.clone());
        open_tab_behind(&mut h.app, incoming).unwrap();
        let mut saved = ViewSet::new();
        let id = saved.open(ViewKind::Model, None, false);
        let camera = Camera { yaw: 2.3, pitch: 0.4, ..Camera::default() };
        saved.set_spec(id, solidcraft_viewport::ViewSpec::new(camera));
        let dir = h.dir.join("view-memory");
        crate::view_memory::remember(&dir, &path, saved.to_json()).unwrap();
        let before = h.app.cam;
        restore_views_in(&mut h.app, &path, &dir);
        assert_eq!(h.app.cam, before, "a background restore leaves the shown design alone");
        switch(&mut h.app, 1);
        assert_eq!(h.app.docs.views.active().id, id);
        assert_eq!(h.app.cam, camera, "the first selection uses the remembered camera, not Fit");
    }

    #[test]
    fn view_rename_is_available_with_one_view_and_with_the_default_switcher() {
        for many in [false, true] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.app.preferences.design_views_as_tabs = false;
            if many {
                view_open(&mut h.app, ViewKind::Model, None, false);
            }
            h.frames(2);
            h.app.run("tab.view.rename", json!({})).unwrap();
            h.frames(3);
            assert!(crate::scenario::handle_at("tab:rename").is_some(), "the switcher exposes the rename field");
            h.step(&json!({"text": "Inspection"})).unwrap();
            h.step(&json!({"key": "Enter"})).unwrap();
            h.frames(2);
            assert_eq!(h.app.docs.views.active().name, "Inspection");
        }
    }

    #[test]
    fn right_click_on_a_view_tab_opens_only_the_tab_menu() {
        // Both strips: the classic design pill and the clear view tabs.
        for clear in [false, true] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.app.preferences.design_views_as_tabs = true;
            h.app.preferences.clear_view_tabs = clear;
            view_open(&mut h.app, ViewKind::Model, None, false);
            h.call("ui.resize", json!({"width": 1920, "height": 720}));
            h.frames(3);
            h.step(&json!({"click": {"handle": "tab:view:1"}, "button": "right"})).unwrap();
            h.frames(3);
            assert!(crate::scenario::handle_at("menu:view.close").is_some(), "clear {clear}: the tab menu opens");
            assert!(
                !matches!(h.app.menu.target, crate::context_menu::Target::AppBar { .. }),
                "clear {clear}: the app bar's Customize menu must not open over the tab menu"
            );
            // Empty app bar space still opens Customize App Bar.
            h.step(&json!({"key": "Escape"})).unwrap();
            h.frames(2);
            h.step(&json!({"click": [1000, 17], "button": "right"})).unwrap();
            h.frames(3);
            assert_eq!(h.app.menu.target, crate::context_menu::Target::AppBar { id: None }, "clear {clear}");
            assert!(crate::scenario::handle_at("menu:view.close").is_none(), "clear {clear}");
        }
    }

    #[test]
    fn view_popups_never_follow_an_index_into_another_design() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.preferences.design_views_as_tabs = true;
        view_open(&mut h.app, ViewKind::Model, None, false);
        h.frames(3);
        let [x, y] = h.at(&json!({"handle": "tab:view:0"})).unwrap();
        let pos = egui::pos2(x as f32, y as f32);
        h.app.synthetic.extend([
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton { pos, button: egui::PointerButton::Secondary, pressed: true, modifiers: egui::Modifiers::NONE },
            egui::Event::PointerButton { pos, button: egui::PointerButton::Secondary, pressed: false, modifiers: egui::Modifiers::NONE },
        ]);
        h.frames(3);
        assert!(crate::scenario::handle_at("menu:view.close").is_some());
        open_tab(&mut h.app, Session::default()).unwrap();
        h.frames(3);
        assert!(crate::scenario::handle_at("menu:view.close").is_none(), "the old design's popup is discarded");
        assert_eq!(h.app.docs.views.len(), 1);
    }

    /// The Layout page's "Design tabs" choices govern the views: a kind kept out of sub-tabs is
    /// entered in the view shown, and reuse off opens a second view of the same subject.
    #[test]
    fn design_tab_preferences_decide_which_views_open() {
        let mut h = crate::scenario::Harness::new();
        // The classic switcher, which this test covers: since #19655 the default lets the focused
        // view choose the workspace (`workspace_follows_view`).
        h.app.preferences.workspace_follows_view = false;
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane": "XY", "name": "Sketch1"})).unwrap();
        h.app.run("sketch.line", json!({"points": [[0, 0], [10, 0]]})).unwrap();
        h.frames(2);
        assert!(h.app.session.active_sketch.is_some(), "the sketch is entered");
        assert_eq!(h.app.docs.views.len(), 1, "but without a sub-tab");
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.preferences.design_sub_tab_sheets = false;
        let model = h.app.docs.views.active().id;
        assert_eq!(view_open(&mut h.app, ViewKind::Sheet, Some("d1".into()), true), model, "the view shown stays");
        assert_eq!(h.app.docs.views.len(), 1);
        h.app.preferences.design_sub_tab_sheets = true;
        let first = view_open(&mut h.app, ViewKind::Sheet, Some("d1".into()), true);
        assert_eq!(view_open(&mut h.app, ViewKind::Sheet, Some("d1".into()), true), first, "reuse on");
        h.app.preferences.design_reuse_views = false;
        assert_ne!(view_open(&mut h.app, ViewKind::Sheet, Some("d1".into()), true), first, "reuse off");
        assert_eq!(h.app.docs.views.len(), 3);
    }

    /// The owner's ask: by default a design tab shows no view switcher while the design has its one
    /// view, and entering a sketch or Render stays in that view, so the tab looks as it did before
    /// design views. The switcher shows once the user opens a second view, with "Show the design
    /// view switcher" on (even for one view), and views
    /// as tabs keeps its sub-tabs and the tab's + for a new view.
    #[test]
    fn a_design_tab_shows_no_view_switcher_until_it_has_a_second_view() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.custom_titlebar = true;
        h.call("ui.resize", json!({"width": 1280, "height": 720}));
        let switcher = |h: &mut crate::scenario::Harness| {
            h.frames(3);
            crate::scenario::rect_of("tab:switcher").is_some()
        };
        assert!(!h.app.preferences.show_design_view_switcher && !h.app.preferences.design_views_as_tabs, "both off by default");
        assert!(!switcher(&mut h), "one view: no switcher");
        assert!(crate::scenario::handle_at("tab:design:0").is_some(), "the strip is drawn");
        // #19659 (owner-approved): a single-view design tab shows its + for a new view.
        assert!(crate::scenario::handle_at("tab:view_new").is_some(), "but the tab's + for a new view is there");
        h.app.run("sketch.create", json!({"plane": "XY", "name": "Sketch1"})).unwrap();
        h.frames(2);
        assert!(h.app.session.active_sketch.is_some(), "the sketch is entered");
        assert_eq!(h.app.docs.views.len(), 1, "in the view shown");
        assert!(!switcher(&mut h), "still no switcher while sketching");
        h.app.run("sketch.line", json!({"points": [[0, 0], [20, 10]]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        assert!(!switcher(&mut h), "nor after Finish Sketch");
        // The preference brings the switcher back for one view; sketching stays in it.
        h.app.preferences.show_design_view_switcher = true;
        assert!(switcher(&mut h), "the preference shows it for one view");
        h.app.run("sketch.create", json!({"plane": "XZ", "name": "Sketch2"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.docs.views.len(), 1, "the switcher does not turn sketch mode into a view");
        h.app.run("sketch.finish", json!({})).unwrap();
        h.frames(2);
        h.app.preferences.show_design_view_switcher = false;
        assert!(!switcher(&mut h), "one Model still needs no switcher");
        view_open(&mut h.app, ViewKind::Model, None, false);
        assert!(switcher(&mut h), "a second view shows it with the preference off");
        // Views as tabs: sub-tabs, no switcher, whatever the preference (on a bar with room for them;
        // a narrow one falls back to the switcher).
        h.call("ui.resize", json!({"width": 1920, "height": 1080}));
        h.app.preferences.design_views_as_tabs = true;
        assert!(!switcher(&mut h), "views as tabs draws sub-tabs instead");
        assert!(crate::scenario::rect_of("tab:view:0").is_some(), "the sub-tabs are drawn");
        h.app.preferences.show_design_view_switcher = true;
        assert!(!switcher(&mut h));
        // Back to one view with both off: no switcher again; a view opened with Ctrl+T shows it.
        h.app.preferences.design_views_as_tabs = false;
        h.app.preferences.show_design_view_switcher = false;
        let first = h.app.docs.views.views()[0].id;
        view_close_others(&mut h.app, first);
        assert!(!switcher(&mut h), "one view again");
        h.app.preferences.design_views_as_tabs = true;
        assert!(!switcher(&mut h));
        assert!(crate::scenario::handle_at("tab:view_new").is_some(), "views as tabs keeps the tab's + for a new view");
        h.app.preferences.design_views_as_tabs = false;
        h.call("ui.key", json!({"key": "T", "cmd": true}));
        assert_eq!(h.app.docs.views.len(), 2, "Ctrl+T still opens another view");
        assert!(switcher(&mut h), "and the switcher shows");
    }

    /// Sketch lifecycle commands change features, never the Model views or their reopen history.
    #[test]
    fn sketch_lifecycle_keeps_the_current_model_view() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.preferences.design_views_as_tabs = true;
        let model = h.app.docs.views.active().id;
        h.app.run("sketch.create", json!({"plane": "XY", "name": "Sketch1"})).unwrap();
        h.frames(2);
        let sketch = h.app.session.active_sketch.unwrap();
        assert_eq!(h.app.docs.views.active().id, model);
        h.app.run("sketch.line", json!({"points": [[0, 0], [20, 10]]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.frames(2);
        assert_eq!(h.app.docs.views.len(), 1);
        h.app.run("sketch.edit", json!({"sketch": sketch})).unwrap();
        h.frames(2);
        assert_eq!(h.app.docs.views.active().id, model);
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("sketch.create", json!({"plane": "XY", "name": "Sketch2"})).unwrap();
        h.frames(2);
        h.app.run("sketch.cancel", json!({})).unwrap();
        h.frames(2);
        assert_eq!(h.app.docs.views.len(), 1);
        assert_eq!(h.app.docs.views.active().id, model);
        assert!(!h.app.docs.views.can_reopen(), "no sketch tab enters the closed stack");
        for _ in 0..12 {
            if h.app.session.doc.features.iter().all(|feature| feature.name != "Sketch1") {
                break;
            }
            h.app.run("edit.undo", json!({})).unwrap();
            h.frames(2);
        }
        assert!(h.app.session.doc.features.iter().all(|feature| feature.name != "Sketch1"));
        assert_eq!(h.app.docs.views.len(), 1);
        assert_eq!(h.app.docs.views.active().id, model);
    }

    /// With "Reopen a design's views" on, a saved design's views come back when it opens again:
    /// those that can still be shown, nothing from a stale memory, and nothing at all when the
    /// preference is off.
    #[test]
    fn a_design_gets_its_remembered_views_back_when_the_preference_is_on() {
        let dir = std::env::temp_dir().join(format!("solidcraft-documents-views-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.preferences.show_design_view_switcher = true;
        let path = std::env::temp_dir().join("solidcraft-remembered.scd").to_string_lossy().into_owned();
        h.app.session.path = Some(path.clone());
        h.app.run("sketch.create", json!({"plane": "XY", "name": "Sketch1"})).unwrap();
        h.frames(2);
        h.app.run("sketch.finish", json!({})).unwrap();
        h.frames(2);
        view_open(&mut h.app, ViewKind::Model, None, false);
        view_open(&mut h.app, ViewKind::Sheet, Some("d1".into()), true);
        let model = h.app.docs.views.views()[0].id;
        view_activate(&mut h.app, model);
        assert_eq!(h.app.docs.views.len(), 3);
        remember_views_in(&h.app, 0, &dir);
        crate::view_memory::wait_for_pending().unwrap();
        assert!(!dir.exists(), "off: nothing is kept");
        h.app.preferences.design_reopen_views = true;
        remember_views_in(&h.app, 0, &dir);
        crate::view_memory::wait_for_pending().unwrap();
        h.app.docs.views = ViewSet::new();
        h.app.preferences.design_reopen_views = false;
        restore_views_in(&mut h.app, &path, &dir);
        assert_eq!(h.app.docs.views.len(), 1, "off: nothing comes back");
        h.app.preferences.design_reopen_views = true;
        restore_views_in(&mut h.app, &path, &dir);
        let kinds: Vec<ViewKind> = h.app.docs.views.views().iter().map(|view| view.kind).collect();
        assert_eq!(kinds, [ViewKind::Model, ViewKind::Model], "the sheet has no workspace yet");
        assert_eq!(h.app.docs.views.active().kind, ViewKind::Model);
        // Legacy remembered Sketch tabs are removed without changing design features.
        let stale = json!({"views": [{"id": 7, "kind": "sketch", "name": "Sketch 1", "subject": "987654"}], "active": 7});
        crate::view_memory::remember(&dir, &path, stale).unwrap();
        restore_views_in(&mut h.app, &path, &dir);
        assert_eq!(h.app.docs.views.len(), 1);
        assert_eq!(h.app.docs.views.active().kind, ViewKind::Model);
        assert!(h.app.session.doc.features.iter().any(|feature| feature.name == "Sketch1"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn passive_native_presentation_preserves_pending_recalled_root_framing() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.design_reopen_views = true;
        let root = h.app.docs.active_tab();
        let path = h.dir.join("background.solidcraft").to_string_lossy().into_owned();
        let mut session = Session::default();
        session.path = Some(path.clone());
        open_tab_behind(&mut h.app, session).unwrap();
        let background = h.app.docs.slots[1].as_ref().unwrap().tab;
        let mut recalled = ViewSet::new();
        let view = recalled.active().id;
        let camera = Camera { yaw: 0.71, pitch: 0.23, fov: 0.52, up: solidcraft_viewport::UpAxis::Y, ..Camera::default() };
        recalled.set_spec(view, solidcraft_viewport::ViewSpec::new(camera));
        crate::view_memory::remember(&h.dir, &path, recalled.to_json()).unwrap();
        restore_views_in(&mut h.app, &path, &h.dir);
        assert!(h.app.docs.slots[1].as_ref().unwrap().restore_views);

        // Native child painting passively presents its design before swapping in
        // the child host. Parking the root must preserve its recalled camera.
        assert!(select_tab(&mut h.app, background));
        assert!(h.app.docs.restore_views);
        assert_eq!(h.app.cam.yaw, camera.yaw);
        assert_eq!(h.app.cam.fov, camera.fov);
        assert!(h.app.ui.perspective);
        assert_eq!(h.app.ui.up_axis, camera.up);
        park_active_look(&mut h.app);
        show_active_look(&mut h.app);
        assert!(select_tab(&mut h.app, root));
        assert!(h.app.docs.slots[1].as_ref().unwrap().restore_views);
        let saved = h.app.docs.slots[1].as_ref().unwrap().views.active().spec.unwrap().camera;
        assert_eq!((saved.yaw, saved.pitch, saved.fov), (camera.yaw, camera.pitch, camera.fov));

        switch(&mut h.app, 1);
        assert!(!h.app.docs.restore_views);
        assert_eq!((h.app.cam.yaw, h.app.cam.pitch, h.app.cam.fov), (camera.yaw, camera.pitch, camera.fov));
        assert!(h.app.ui.perspective);
        assert_eq!(h.app.ui.up_axis, camera.up);
    }

    #[test]
    fn model_and_render_recall_initializes_projection_and_up_axis_once() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.design_reopen_views = true;
        h.app.preferences.workspace_follows_view = true;
        let path = h.dir.join("projection.solidcraft").to_string_lossy().into_owned();
        h.app.session.path = Some(path.clone());
        for kind in [ViewKind::Model, ViewKind::Render] {
            for fov in [0.0, 0.52] {
                let mut recalled = ViewSet::new();
                let id = recalled.open(kind, None, true);
                let camera = Camera { fov, up: solidcraft_viewport::UpAxis::Y, ..Camera::default() };
                recalled.set_spec(id, solidcraft_viewport::ViewSpec::new(camera));
                crate::view_memory::remember(&h.dir, &path, recalled.to_json()).unwrap();
                h.app.ui.perspective = fov == 0.0;
                h.app.ui.up_axis = solidcraft_viewport::UpAxis::Z;
                restore_views_in(&mut h.app, &path, &h.dir);
                assert_eq!(h.app.docs.views.active().kind, kind);
                assert_eq!(h.app.ui.perspective, fov > 0.0);
                assert_eq!(h.app.ui.up_axis, camera.up);
                assert_eq!((h.app.cam.fov, h.app.cam.up), (camera.fov, camera.up));
                // The next navigation click precedes the frame that updates cam.
                h.app.ui.perspective = fov == 0.0;
                h.app.ui.up_axis = solidcraft_viewport::UpAxis::Z;
                let view = h.app.docs.views.active().clone();
                show_look(&mut h.app, &view);
                assert_eq!(h.app.ui.perspective, fov == 0.0);
                assert_eq!(h.app.ui.up_axis, solidcraft_viewport::UpAxis::Z);
            }
        }
    }

    #[test]
    fn pending_tile_projection_survives_reentry_before_its_camera_updates() {
        let mut app = app();
        app.ui.perspective = false;
        app.ui.auto_ortho_faces = false;
        let ground = app.docs.views.active().id;
        for kind in [ViewKind::Model, ViewKind::Render] {
            let id = app.docs.views.open(kind, None, false);
            app.docs.views.set_spec(id, solidcraft_viewport::ViewSpec::new(Camera::default()));
            app.docs.views.activate(ground);
            let mut look = ViewLook::from_ui(&app.ui);
            look.options.auto_ortho_faces = false;
            look.options.perspective = false;
            app.docs.navigation.insert(id, look);
            with_view_look(&mut app, id, |app| app.ui.perspective = true);
            assert_eq!(app.docs.views.get(id).unwrap().spec.unwrap().camera.fov, 0.0);
            with_view_look(&mut app, id, |app| assert!(app.perspective_view()));
            assert!(!app.ui.perspective, "the ground keeps its own projection");
        }
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

    /// The volume of the shown design's bodies, in mm³.
    fn shown_volume(a: &mut SolveApp) -> f64 {
        a.run("inspect.measure", json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap()
    }

    /// Three sheets in three tabs: a cube of side 10, 20 and 30 mm, each named, as a nesting
    /// program makes them. Tab order and ids come back with the app.
    fn three_sheets() -> (SolveApp, Vec<String>) {
        let mut a = app();
        let mut ids = Vec::new();
        for (n, side) in [(1, 10), (2, 20), (3, 30)] {
            if n == 1 {
                a.run("file.new", json!({"name": "Sheet 1"})).unwrap();
            } else {
                let reply = a.run("file.new", json!({"name": format!("Sheet {n}"), "new_tab": true})).unwrap();
                assert_eq!(reply["document_id"], json!(a.session.document_id), "the reply names the new design");
            }
            a.run("solid.box", json!({"length": side, "width": side, "height": side})).unwrap();
            ids.push(a.session.document_id.clone());
        }
        (a, ids)
    }

    /// `file.new` with `new_tab` opens a design in a tab of its own, after the active one, and
    /// shows it: three calls make three tabs with three ids, and the first two keep their contents.
    #[test]
    fn file_new_with_new_tab_opens_a_tab_each_time_and_keeps_the_others() {
        let (mut a, ids) = three_sheets();
        assert_eq!(a.docs.count(), 3);
        assert_eq!(a.docs.active, 2, "the new design is the one shown");
        let distinct: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(distinct.len(), 3, "{ids:?}");
        for (i, side) in [10.0, 20.0, 30.0].into_iter().enumerate() {
            switch(&mut a, i);
            assert_eq!(a.session.document_id, ids[i]);
            assert_eq!(a.session.doc.name, format!("Sheet {}", i + 1));
            assert!((shown_volume(&mut a) - side * side * side).abs() < 1e-6, "tab {i}");
        }
        assert_eq!(a.docs.count(), 3, "looking at the tabs opens none");
    }

    /// The new tab is always opened, even where the active design is blank (a launch's untouched
    /// design stays beside it), and without `new_tab` or with it false `file.new` replaces the
    /// active design as it always did.
    #[test]
    fn file_new_replaces_the_design_unless_it_is_asked_for_a_tab() {
        let mut a = app();
        a.run("file.new", json!({"new_tab": true})).unwrap();
        assert_eq!(a.docs.count(), 2, "the blank design is kept");
        for params in [json!({}), json!({"name": "Plain"}), json!({"new_tab": false}), json!({"new_tab": null, "name": "Plain"})] {
            let reply = a.run("file.new", params.clone()).unwrap();
            assert_eq!(a.docs.count(), 2, "{params}");
            assert!(reply.get("document_id").is_none(), "no id without a new tab: {reply}");
        }
    }

    /// `document.list` says which design is active, in tab order; `document.activate` shows one by
    /// its id, and the commands after it act on that design.
    #[test]
    fn document_list_and_activate_follow_the_tabs() {
        let (mut a, ids) = three_sheets();
        let listed = a.run("document.list", json!({})).unwrap();
        assert_eq!(listed["active_document_id"], json!(ids[2]), "{listed}");
        let rows = listed["documents"].as_array().unwrap();
        assert_eq!(rows.iter().map(|r| r["document_id"].clone()).collect::<Vec<_>>(), ids.iter().map(|i| json!(i)).collect::<Vec<_>>());
        assert_eq!(rows.iter().map(|r| r["name"].clone()).collect::<Vec<_>>(), [json!("Sheet 1"), json!("Sheet 2"), json!("Sheet 3")]);
        assert_eq!(rows.iter().map(|r| r["active"].clone()).collect::<Vec<_>>(), [json!(false), json!(false), json!(true)]);
        assert!(rows.iter().all(|r| r["path"].is_null() && r["dirty"] == json!(true)), "{listed}");

        let shown = a.run("document.activate", json!({"document_id": ids[0]})).unwrap();
        assert_eq!((shown["document_id"].clone(), shown["name"].clone()), (json!(ids[0]), json!("Sheet 1")), "{shown}");
        assert_eq!((a.docs.active, &a.session.document_id), (0, &ids[0]));
        assert_eq!(a.run("document.list", json!({})).unwrap()["active_document_id"], json!(ids[0]));
        // A command acts on the shown design, and only on it.
        a.run("solid.box", json!({"length": 1, "width": 1, "height": 1, "corner": [50, 0, 0]})).unwrap();
        assert!((shown_volume(&mut a) - 1001.0).abs() < 1e-6);
        switch(&mut a, 1);
        assert!((shown_volume(&mut a) - 8000.0).abs() < 1e-6, "the other tab is as it was");

        // A name that no open design has changes nothing, and neither does the design already shown.
        for bad in [json!({"document_id": "no-such-design"}), json!({}), json!({"document_id": ids[0], "index": 1})] {
            assert!(a.run("document.activate", bad.clone()).is_err(), "{bad}");
            assert_eq!(a.docs.active, 1, "{bad}");
        }
        a.run("document.activate", json!({"document_id": ids[1]})).unwrap();
        assert_eq!(a.docs.active, 1);
    }

    /// A caller kept to some folders sees no path for a design saved outside them and cannot show
    /// it; a design inside them, or one never saved, is listed and shown as before.
    #[test]
    fn a_scoped_caller_neither_sees_nor_shows_a_design_from_outside_its_folders() {
        use solidcraft_engine::io::vfs::{self, PathScope};
        let base = std::env::temp_dir().join(format!("solidcraft-doc-list-scope-{}", std::process::id()));
        let (inside, outside) = (base.join("inside"), base.join("outside"));
        std::fs::create_dir_all(&inside).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let (mine, theirs) = (inside.join("a.solidcraft"), outside.join("b.solidcraft"));
        let text = |p: &std::path::Path| p.to_string_lossy().into_owned();

        let (mut a, ids) = three_sheets();
        for (i, p) in [(0, &mine), (1, &theirs)] {
            switch(&mut a, i);
            a.session.path = Some(text(p));
        }
        switch(&mut a, 2);

        let scope = || Some(PathScope::new([inside.clone()]));
        let listed = vfs::scoped(scope(), || a.run("document.list", json!({}))).unwrap();
        let rows = listed["documents"].as_array().unwrap();
        assert_eq!(rows[0]["path"], json!(text(&mine)), "{listed}");
        assert!(rows[1]["path"].is_null(), "an outside path is not told: {listed}");
        assert!(rows[2]["path"].is_null(), "{listed}");
        assert_eq!(rows[1]["document_id"], json!(ids[1]), "the design itself is still listed");
        assert_eq!(rows[1]["name"], json!("b"), "a saved design is named by its file");

        let refused = vfs::scoped(scope(), || a.run("document.activate", json!({"document_id": ids[1]}))).unwrap_err();
        assert!(refused.contains("outside the folders this caller may use"), "{refused}");
        assert!(!refused.contains(&text(&theirs)) && !refused.contains("b.solidcraft"), "the refusal names the hidden path: {refused}");
        assert_eq!(a.docs.active, 2, "a refused design is not shown");
        vfs::scoped(scope(), || a.run("document.activate", json!({"document_id": ids[0]}))).unwrap();
        assert_eq!(a.docs.active, 0);
        // The unsaved design has no file to refuse, and an unscoped caller is not limited.
        vfs::scoped(scope(), || a.run("document.activate", json!({"document_id": ids[2]}))).unwrap();
        a.run("document.activate", json!({"document_id": ids[1]})).unwrap();
        assert_eq!(a.docs.active, 1);
        let _ = std::fs::remove_dir_all(base);
    }

    /// While the assistant follows its own conversation's design, another tab is not shown.
    #[test]
    fn document_activate_keeps_to_the_design_the_assistant_follows() {
        let (mut a, ids) = three_sheets();
        a.agent_chat.following_document = true;
        assert!(a.run("document.activate", json!({"document_id": ids[0]})).is_err());
        assert_eq!(a.docs.active, 2);
        assert!(a.run("document.activate", json!({"document_id": ids[2]})).is_ok(), "the design shown stays shown");
        a.agent_chat.following_document = false;
        a.run("document.activate", json!({"document_id": ids[0]})).unwrap();
        assert_eq!(a.docs.active, 0);
    }

    /// `doc.open` with `new_tab` opens the file in a tab of its own, keeps the design that was
    /// open, and a failed open leaves the tabs and the shown design as they were.
    #[test]
    fn doc_open_with_new_tab_keeps_the_open_design() {
        let dir = std::env::temp_dir().join(format!("solidcraft-doc-open-tab-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("sheet.solidcraft").to_string_lossy().into_owned();
        let mut saved = Session::default();
        saved.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5})).unwrap();
        saved.execute("file.save_as", &json!({"path": file})).unwrap();

        let mut a = app();
        a.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        let first = a.session.document_id.clone();
        a.run("doc.open", json!({"path": file, "new_tab": true})).unwrap();
        assert_eq!((a.docs.count(), a.docs.active), (2, 1));
        assert_eq!(a.session.path.as_deref(), Some(file.as_str()));
        assert!((shown_volume(&mut a) - 125.0).abs() < 1e-6);
        switch(&mut a, 0);
        assert_eq!(a.session.document_id, first);
        assert!((shown_volume(&mut a) - 1000.0).abs() < 1e-6, "the design that was open is as it was");

        // The file is open in the second tab already: it is shown, not opened a second time.
        let shown = a.run("doc.open", json!({"path": file, "new_tab": true})).unwrap();
        assert_eq!((a.docs.count(), a.docs.active), (2, 1), "{shown}");
        assert_eq!(shown["path"], json!(file));
        switch(&mut a, 0);

        let missing = dir.join("missing.solidcraft").to_string_lossy().into_owned();
        assert!(a.run("doc.open", json!({"path": missing, "new_tab": true})).is_err());
        assert_eq!((a.docs.count(), a.docs.active), (2, 0), "no stray tab");
        assert_eq!(a.session.document_id, first);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// `new_tab` is taken off the parameters only when it is a boolean or null: any other value
    /// stays, for the engine's contract check to see. `document.list` is told from the commands
    /// that change the design, in any spelling, so it is not held while a file loads.
    #[test]
    fn new_tab_values_and_the_list_request_are_read_exactly() {
        for (given, taken, kept) in [
            (json!({"new_tab": true}), true, false),
            (json!({"new_tab": false}), false, false),
            (json!({"new_tab": null}), false, false),
            (json!({"new_tab": "yes"}), false, true),
            (json!({"new_tab": 1}), false, true),
            (json!({}), false, false),
        ] {
            let mut params = given.clone();
            assert_eq!(take_new_tab(&mut params), taken, "{given}");
            assert_eq!(params.get("new_tab").is_some(), kept, "{given}");
        }
        let request = |method: &str, params: Value| crate::ControlRequest::new(method, params).0;
        assert!(crate::control_job::lists_designs(&request("engine.execute", json!({"command": "document.list"}))));
        assert!(crate::control_job::lists_designs(&request("command", json!({"id": "DOCUMENT.LIST", "params": null}))));
        assert!(!crate::control_job::lists_designs(&request("engine.execute", json!({"command": "document.activate"}))));
        assert!(!crate::control_job::lists_designs(&request("engine.execute", json!({"command": "file.new"}))));
        assert!(!crate::control_job::lists_designs(&request("document.inspect", json!({}))));
    }

    /// The tabs are the window's: a batch or a script that opens or shows one would roll back, or
    /// go on, in the wrong design, so they are refused as the other tab switches are.
    #[test]
    fn a_batch_may_not_open_or_show_a_design() {
        let (a, ids) = three_sheets();
        for step in
            [json!({"command": "file.new", "params": {"new_tab": true}}), json!({"command": "document.activate", "params": {"document_id": ids[0]}})]
        {
            assert!(crate::control::batch_refusal(std::slice::from_ref(&step)).is_some(), "{step}");
        }
        for step in [json!({"command": "file.new", "params": {"new_tab": false}}), json!({"command": "document.list"})] {
            assert!(crate::control::batch_refusal(std::slice::from_ref(&step)).is_none(), "{step}");
        }
        assert_eq!(a.docs.count(), 3);
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
    fn measuring_and_drawing(keep: bool, remember: bool) -> (SolveApp, Vec<solidcraft_engine::Sel>) {
        let mut a = app();
        a.preferences.keep_work_on_tab_switch = keep;
        a.preferences.remember_toolbar_tab = remember;
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
        // Without remember_toolbar_tab, so the toolbar tab comes from this preference alone.
        let (mut a, picks) = measuring_and_drawing(true, false);
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

    /// Without the preference (the default), a switch cancels the dialog and the tool, as before
    /// #9360; with `remember_toolbar_tab` on (off by default) the design keeps its toolbar tab.
    #[test]
    fn a_tab_switch_cancels_the_dialog_and_tool_by_default() {
        let defaults = crate::prefs::Prefs::default();
        assert!(!defaults.keep_work_on_tab_switch && !defaults.remember_toolbar_tab);
        let (mut a, _) = measuring_and_drawing(false, true);
        switch(&mut a, 0);
        assert!(a.dialog.is_none() && a.tool.is_none());
        assert_eq!(a.ui.tab, "INSPECT", "the box's design is back on the tab it was using");
        switch(&mut a, 1);
        assert!(a.dialog.is_none() && a.tool.is_none());
        // The sketch itself stays open in its design, so its tab is SKETCH.
        assert_eq!(a.ui.tab, "SKETCH");
    }

    /// With both preferences off, a switch shows MODEL, as before.
    #[test]
    fn without_remember_toolbar_tab_a_switch_shows_model() {
        let (mut a, _) = measuring_and_drawing(false, false);
        switch(&mut a, 0);
        assert!(a.dialog.is_none() && a.tool.is_none());
        assert_eq!(a.ui.tab, "MODEL");
        switch(&mut a, 1);
        assert_eq!(a.ui.tab, "SKETCH", "an open sketch still shows its own tab");
    }

    /// A new tab with a box in it, so [`new_design`] doesn't reuse it as a blank design.
    fn new_design_with_a_box(a: &mut SolveApp) {
        new_design(a);
        a.run("solid.box", json!({"length": 1, "width": 1, "height": 1})).unwrap();
    }

    /// Each design comes back on the toolbar tab it was last using: one left on SURFACE is on
    /// SURFACE again after another design, left on MODEL, was shown; a new design starts on MODEL.
    #[test]
    fn each_design_remembers_its_own_toolbar_tab() {
        let mut a = app();
        a.preferences.remember_toolbar_tab = true;
        a.run("solid.box", json!({"length": 1, "width": 1, "height": 1})).unwrap();
        a.ui.tab = "SURFACE".into();
        new_design_with_a_box(&mut a);
        assert_eq!(a.docs.count(), 2);
        assert_eq!(a.ui.tab, "MODEL", "a new design starts on MODEL");
        new_design_with_a_box(&mut a);
        a.ui.tab = "INSPECT".into();
        for (i, tab) in [(0, "SURFACE"), (1, "MODEL"), (2, "INSPECT"), (0, "SURFACE"), (2, "INSPECT"), (1, "MODEL")] {
            switch(&mut a, i);
            assert_eq!(a.ui.tab, tab, "design {i}");
        }
        // A tab changed after a switch is the one remembered next time.
        a.ui.tab = "VIEW".into();
        switch(&mut a, 0);
        switch(&mut a, 1);
        assert_eq!(a.ui.tab, "VIEW");
    }

    /// A remembered tab hidden from the toolbar since falls back to MODEL, as a hidden tab does.
    #[test]
    fn a_remembered_tab_hidden_since_falls_back_to_model() {
        let mut a = app();
        a.run("solid.box", json!({"length": 1, "width": 1, "height": 1})).unwrap();
        a.ui.tab = "SURFACE".into();
        new_design_with_a_box(&mut a);
        assert_eq!(a.docs.count(), 2);
        a.preferences.hidden_toolbar_tabs = vec!["SURFACE".into()];
        switch(&mut a, 0);
        assert_eq!(a.ui.tab, "MODEL");
    }

    /// A design with a box and a drawing of two sheets, open: the sheets' ids.
    fn two_sheet_drawing(a: &mut SolveApp) -> [u64; 2] {
        a.run("solid.box", json!({"length": 40, "width": 20, "height": 3})).unwrap();
        a.run("drawing.create", json!({})).unwrap();
        a.run("drawing.sheet.add", json!({"name": "Inspection sheet"})).unwrap();
        let drawing = a.session.drawing("test").unwrap();
        [0, 1].map(|i| drawing.sheets[i].id)
    }

    /// K10 (#3890): each tab keeps its own Drawing workspace. The sheet and the zoom chosen in one
    /// tab come back with it, and the other tab's never show in it.
    #[test]
    fn a_drawing_tab_keeps_its_sheet_and_zoom() {
        use crate::drawing_sheet_page::{active_sheet, select_sheet, set_zoom, zoom_percent};
        let ctx = egui::Context::default();
        let mut a = app();
        let [_, second] = two_sheet_drawing(&mut a);
        select_sheet(&mut a, second);
        set_zoom(&mut a, &ctx, 1.5);
        new_design(&mut a);
        let [other, _] = two_sheet_drawing(&mut a);
        select_sheet(&mut a, other);
        set_zoom(&mut a, &ctx, 0.5);
        switch(&mut a, 0);
        assert_eq!(active_sheet(&a), Some(second), "the first tab is back on its second sheet");
        assert!((zoom_percent(&a, &ctx) - 150.0).abs() < 1.0, "at 150 %: {}", zoom_percent(&a, &ctx));
        switch(&mut a, 1);
        assert_eq!(active_sheet(&a), Some(other));
        assert!((zoom_percent(&a, &ctx) - 50.0).abs() < 1.0, "at 50 %: {}", zoom_percent(&a, &ctx));
    }

    /// K10 (#3890): `ui.documents` says which drawing each tab has open, and the entries follow
    /// their tabs when a tab moves.
    #[test]
    fn each_tab_keeps_its_open_drawing_and_says_which() {
        let mut a = app();
        two_sheet_drawing(&mut a);
        let drawing = a.session.drawing("test").unwrap();
        let expected = json!({"id": solidcraft_engine::doc::ids::encode(drawing.id), "name": drawing.name});
        new_design(&mut a);
        let drawings = |a: &mut SolveApp| {
            let reply = control(a, &json!({}));
            reply["documents"].as_array().unwrap().iter().map(|tab| tab["drawing"].clone()).collect::<Vec<_>>()
        };
        assert_eq!(drawings(&mut a), [expected.clone(), Value::Null]);
        control(&mut a, &json!({"action": "move", "from": 0, "to": 1}));
        assert_eq!(drawings(&mut a), [Value::Null, expected.clone()]);
        switch(&mut a, 0);
        assert_eq!(drawings(&mut a), [Value::Null, expected]);
    }

    /// An open drawing never locks model commands; a tab's sheet sketch stays in its tab.
    #[test]
    fn a_tabs_model_commands_run_with_its_drawing_open() {
        let mut a = app();
        a.preferences.workspace_follows_view = false;
        let [sheet, _] = two_sheet_drawing(&mut a);
        a.run("solid.sphere", json!({"diameter":4})).unwrap();
        assert!(a.session.open_drawing.is_some());
        crate::workspace::enter(&mut a, "drawing").unwrap();
        a.run("solid.sphere", json!({"diameter": 4})).unwrap();
        assert!(a.session.open_drawing.is_some(), "with the drawing still open");
        a.run("drawing.sketch.create", json!({"owner": solidcraft_engine::doc::ids::encode(sheet)})).unwrap();
        assert!(a.session.active_sheet_sketch.is_some(), "the sheet sketch is open");
        new_design(&mut a);
        a.run("solid.sphere", json!({"diameter":4})).unwrap();
        assert!(a.session.active_sheet_sketch.is_none(), "the other design has its own edit receiver");
        switch(&mut a, 0);
        assert!(a.session.active_sheet_sketch.is_some(), "the sheet sketch waits in its own design tab");
        crate::workspace::enter(&mut a, "drawing").unwrap();
        a.run("sketch.finish", json!({})).unwrap();
        assert!(a.session.active_sheet_sketch.is_none());
        a.run("solid.sphere", json!({"diameter":4})).unwrap();
    }

    /// K10 (#3890): without "Keep dialogs and tools open when switching designs", a tab switch
    /// closes the Drawing workspace's panels, as it cancels a dialog. With it, a panel waits in its
    /// own tab and never shows in another.
    #[test]
    fn a_tab_switch_closes_the_drawing_panels_by_default() {
        for keep in [false, true] {
            let mut a = app();
            a.preferences.keep_work_on_tab_switch = keep;
            two_sheet_drawing(&mut a);
            crate::drawing_workspace::enter(&mut a, true).unwrap();
            a.drawing_workspace.settings_open = true;
            new_design(&mut a);
            assert!(!a.drawing_workspace.settings_open, "Sheet Settings stays out of the other tab");
            switch(&mut a, 0);
            assert_eq!(a.drawing_workspace.settings_open, keep, "keep: {keep}");
        }
    }

    /// K10 (#3890): a new design in one tab (`file.new`, as `doc.open` and `doc.recover`) starts
    /// that tab's Drawing workspace over and leaves the other tab's.
    #[test]
    fn opening_a_file_resets_only_that_tabs_drawing_page() {
        use crate::drawing_sheet_page::{active_sheet, select_sheet, set_zoom, zoom_percent};
        let ctx = egui::Context::default();
        let mut a = app();
        let [_, second] = two_sheet_drawing(&mut a);
        select_sheet(&mut a, second);
        set_zoom(&mut a, &ctx, 1.5);
        new_design(&mut a);
        let blank = zoom_percent(&a, &ctx);
        two_sheet_drawing(&mut a);
        set_zoom(&mut a, &ctx, 0.5);
        a.run("file.new", json!({})).unwrap();
        assert!((zoom_percent(&a, &ctx) - blank).abs() < 1e-3, "this tab starts over");
        switch(&mut a, 0);
        assert_eq!(active_sheet(&a), Some(second));
        assert!((zoom_percent(&a, &ctx) - 150.0).abs() < 1.0, "the other tab keeps its page");
    }

    /// K10 (#3890): a design opened into the blank tab starts its Drawing workspace over, as
    /// `doc.open` does: no panel or zoom is left from the blank design.
    #[test]
    fn adopting_a_design_resets_the_drawing_workspace() {
        let ctx = egui::Context::default();
        let mut a = app();
        let blank = crate::drawing_sheet_page::zoom_percent(&a, &ctx);
        a.drawing_workspace.settings_open = true;
        crate::drawing_sheet_page::set_zoom(&mut a, &ctx, 2.0);
        assert!(active_is_blank(&a));
        let mut design = Session::default();
        design.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        assert!(adopt(&mut a, design));
        assert_eq!(a.docs.count(), 1, "the blank tab took it");
        assert!(!a.drawing_workspace.settings_open);
        assert!((crate::drawing_sheet_page::zoom_percent(&a, &ctx) - blank).abs() < 1e-3);
    }

    /// K10 (#3890): a drawing edit is an unsaved change of its design: closing its tab asks once.
    #[test]
    fn closing_a_tab_with_an_unsaved_drawing_edit_asks_once() {
        let mut a = app();
        two_sheet_drawing(&mut a);
        new_design(&mut a);
        switch(&mut a, 0);
        a.session.mark_saved();
        assert!(!dirty(&a, 0));
        a.run("drawing.sheet.add", json!({"name": "Third sheet"})).unwrap();
        assert!(dirty(&a, 0));
        close(&mut a, 0, false);
        assert_eq!((a.docs.closing, a.docs.count()), (Some(0), 2));
        close(&mut a, 0, true);
        assert_eq!(a.docs.count(), 1);
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
