//! Several open designs, as tabs in the application bar. The active design lives in the app as
//! always (`app.session`, `app.cam`…); the others wait here with their own session (undo,
//! selection), camera, agent conversation and per-design view state, and are swapped in when
//! their tab is picked.
//! Closing a design with unsaved changes asks first, and so does quitting: one "save changes?"
//! per design with unsaved changes ([`request_quit`]).

use crate::ui_kit::{Container, Surface};
use egui::{FontId, Rect, Sense, pos2, vec2};
use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_engine::render::Camera;

use crate::SolveApp;
use crate::theme::Tokens;
use crate::ui_kit;

/// Most designs open at once.
pub const MAX_OPEN: usize = 32;

/// A design that is not the active one: everything that belongs to it alone.
pub struct DocSlot {
    pub(crate) session: Session,
    pub(crate) autosave: Option<solidcraft_engine::recovery::Autosaver>,
    pub(crate) big_operation: bool,
    cam: Camera,
    pre_sketch_marker: Option<Option<usize>>,
    pub(crate) pending_feature_edit: Option<crate::feature_edit::PendingFeatureEdit>,
    pre_sketch_cam: Option<crate::PreSketchView>,
    view: DocView,
    occurrence_move: Option<crate::browser::OccMove>,
    agent_chat: crate::agent_chat::AgentChat,
}

/// The per-design part of the UI state the session doesn't keep (what is hidden, shown or locked
/// is in the session's `visibility`, saved with the design).
#[derive(Clone, Debug, Default)]
struct DocView {
    render: crate::render_workspace::ViewState,
    /// Bodies and components copied or cut in this design: their names and paths mean nothing
    /// in another one (copied features stay with the session the same way).
    copied_bodies: Vec<String>,
    copied_placements: Vec<Vec<u64>>,
    cut_bodies: Vec<String>,
    cut_placements: Vec<Vec<u64>>,
}

impl DocSlot {
    fn new(session: Session) -> DocSlot {
        DocSlot {
            session,
            autosave: None,
            big_operation: false,
            cam: Camera::default(),
            pre_sketch_marker: None,
            pending_feature_edit: None,
            pre_sketch_cam: None,
            view: DocView::default(),
            occurrence_move: None,
            agent_chat: Default::default(),
        }
    }
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
    /// Save As is answering a Close/Quit prompt.
    saving: bool,
    /// Close Other Tabs / Close All Tabs: the tabs still to close, in increasing order (the last
    /// is closed first, so the ones before it keep their places). One with unsaved changes asks
    /// first; Cancel stops the rest (#3055).
    pub batch: Option<Vec<usize>>,
}

impl Documents {
    pub(crate) fn background_mut(&mut self) -> impl Iterator<Item = &mut DocSlot> {
        self.slots.iter_mut().filter_map(Option::as_mut)
    }

    /// The designs in the tabs not shown.
    pub(crate) fn background(&self) -> impl Iterator<Item = &Session> {
        self.slots.iter().filter_map(Option::as_ref).map(|slot| &slot.session)
    }

    /// Number of open designs (at least the active one).
    pub fn count(&self) -> usize {
        self.slots.len().max(1)
    }
}

fn take_view(app: &mut SolveApp) -> DocView {
    DocView {
        render: std::mem::take(&mut app.render_workspace.view),
        copied_bodies: std::mem::take(&mut app.copied_bodies),
        copied_placements: std::mem::take(&mut app.copied_placements),
        cut_bodies: std::mem::take(&mut app.cut_bodies),
        cut_placements: std::mem::take(&mut app.cut_placements),
    }
}

fn put_view(app: &mut SolveApp, v: DocView) {
    app.render_workspace.view = v.render;
    crate::render_workspace::invalidate_images(app);
    crate::render_workspace::invalidate_scene(app);
    crate::render_workspace::sync_backend(app);
    app.copied_bodies = v.copied_bodies;
    app.copied_placements = v.copied_placements;
    app.cut_bodies = v.cut_bodies;
    app.cut_placements = v.cut_placements;
}

/// Put `slot` into the app and return what was there.
fn swap_in(app: &mut SolveApp, mut slot: DocSlot) -> DocSlot {
    if crate::render_workspace::active(app) {
        let _ = crate::render_workspace::enter(app, false);
    }
    // Whatever was going on in the design leaves with it.
    crate::sketch_text_move::cancel_drag(app);
    crate::dialogs::cancel(app);
    crate::dim_view::cancel();
    app.tool = None;
    app.params = None;
    app.design_properties = None;
    app.bom = None;
    app.cancel_view_animation();
    app.viewport.orbit_free = None;
    app.preview.reset();
    app.viewport.hover = None;
    app.menu.props = None;
    crate::sketch_attachment_ui::clear(app);
    crate::agent_chat::leave_document(app);
    app.agent_chat.swap_document(&mut slot.agent_chat);
    let out_rev = app.session.revision;
    let mut out = DocSlot {
        session: std::mem::replace(&mut app.session, slot.session),
        autosave: app.autosave.as_mut().and_then(|a| std::mem::replace(&mut a.active, slot.autosave)),
        big_operation: std::mem::replace(&mut app.big_operation, slot.big_operation),
        cam: std::mem::replace(&mut app.cam, slot.cam),
        pre_sketch_marker: std::mem::replace(&mut app.pre_sketch_marker, slot.pre_sketch_marker),
        pending_feature_edit: std::mem::replace(&mut app.pending_feature_edit, slot.pending_feature_edit),
        pre_sketch_cam: std::mem::replace(&mut app.pre_sketch_cam, slot.pre_sketch_cam),
        view: take_view(app),
        occurrence_move: std::mem::replace(&mut app.tree.occurrence_move, slot.occurrence_move),
        agent_chat: slot.agent_chat,
    };
    // Only the displayed design owns the process-wide crash replay journal.
    app.session.crash_journal = out.session.crash_journal;
    out.session.crash_journal = false;
    put_view(app, slot.view);
    // Preferences the session holds go with the user, not the tab (#680).
    app.session.auto_project = out.session.auto_project;
    app.session.metric_tap_drill_major_less_pitch = out.session.metric_tap_drill_major_less_pitch;
    app.session.surface_extrude_symmetric_per_side = out.session.surface_extrude_symmetric_per_side;
    // So does the host's pre-command check: every tab is held to it.
    app.session.set_command_check(out.session.command_check());
    // Views cache by revision: the incoming design must not look unchanged.
    let before = app.session.revision;
    app.session.invalidate_view(out_rev);
    if let Some(pending) = &mut app.pending_feature_edit {
        pending.view_changed(before, app.session.revision);
    }
    app.ui.ribbon_context = None;
    app.ui.tab = "MODEL".into();
    crate::workspace::sync_context(app);
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
    let Some(incoming) = app.docs.slots.get_mut(i).and_then(Option::take) else { return };
    let out = swap_in(app, incoming);
    let a = app.docs.active;
    if let Some(s) = app.docs.slots.get_mut(a) {
        *s = Some(out);
    }
    app.docs.active = i;
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
    if app.file_picker_busy() {
        return Err("finish choosing a file before switching designs".into());
    }
    switch(app, index);
    Ok(true)
}

fn same_path(a: &str, b: &str) -> bool {
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

/// Open `session` in a new tab after the active one and switch to it (`fit`: frame its model).
pub fn open_tab(app: &mut SolveApp, session: Session) -> Result<(), String> {
    if app.docs.slots.is_empty() {
        app.docs.slots.push(None);
        app.docs.active = 0;
    }
    if app.docs.slots.len() >= MAX_OPEN {
        return Err(format!("at most {MAX_OPEN} designs can be open"));
    }
    let at = app.docs.active + 1;
    app.docs.slots.insert(at, Some(DocSlot::new(session)));
    switch(app, at);
    app.fit_view();
    Ok(())
}

/// Is the active design an untouched new one (it can be replaced instead of opening a tab)?
pub fn active_is_blank(app: &SolveApp) -> bool {
    let s = &app.session;
    s.path.is_none() && s.doc.features.is_empty() && s.doc.params.is_empty() && s.doc.components.is_empty() && !s.is_dirty()
}

/// Show `session` (a design built or read elsewhere): in place of a blank active design,
/// otherwise in a new tab.
pub fn adopt(app: &mut SolveApp, session: Session) {
    if active_is_blank(app) {
        let rev = app.session.revision;
        reset_chat(app);
        crate::dialogs::cancel(app);
        app.tool = None;
        app.menu.props = None;
        crate::sketch_attachment_ui::clear(app);
        app.session.replace_design(session);
        crate::render_workspace::reset_document(app);
        app.session.invalidate_view(rev);
        app.cam = Camera::default();
        app.fit_view();
    } else if let Err(e) = open_tab(app, session) {
        app.set_status(e, true);
    }
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
    if app.file_picker_busy() {
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
    if n <= 1 {
        // The last design: a blank one takes its place.
        reset_chat(app);
        if let Some(a) = app.autosave.as_mut().and_then(|a| a.active.take()) {
            a.discard();
        }
        app.big_operation = false;
        crate::sketch_text_move::cancel_drag(app);
        let rev = app.session.revision;
        crate::dialogs::cancel(app);
        app.tool = None;
        app.session.replace_design(Session::default());
        app.session.revision = rev + 1;
        crate::prefs::apply_new_design(app);
        app.cam = Camera::default();
        app.fit_view();
        put_view(app, DocView::default());
        app.docs = Documents::default();
        app.home.open = true;
        return;
    }
    if i == app.docs.active {
        // Show a neighbour first, then drop this one.
        let next = if i + 1 < n { i + 1 } else { i - 1 };
        switch(app, next);
    }
    if let Some(slot) = app.docs.slots.remove(i)
        && let Some(a) = slot.autosave
    {
        a.discard();
    }
    if app.docs.active > i {
        app.docs.active -= 1;
    }
}

/// A replacement design starts a fresh conversation while retaining the window's chat services.
pub(crate) fn reset_chat(app: &mut SolveApp) {
    crate::agent_chat::leave_document(app);
    app.agent_chat.swap_document(&mut crate::agent_chat::AgentChat::default());
}

/// Hidden conversations must also stop when AI is disabled or the application quits.
pub(crate) fn disconnect_background_chats(app: &mut SolveApp, wait: bool) {
    for slot in app.docs.background_mut() {
        slot.agent_chat.disconnect_document(&mut slot.session, wait);
    }
}

/// Close several tabs (Close Other Tabs, Close All Tabs; #3055): each with unsaved changes asks
/// "save changes?" in turn, the others close; Cancel keeps the tabs not closed yet.
pub fn close_many(app: &mut SolveApp, mut tabs: Vec<usize>) {
    if app.file_picker_busy() || app.docs.quitting.is_some() || app.docs.closing.is_some() {
        return;
    }
    tabs.sort_unstable();
    tabs.dedup();
    app.docs.batch = Some(tabs);
    batch_step(app);
}

/// Close the next tab of [`close_many`]'s batch, or ask about it when it has unsaved changes.
fn batch_step(app: &mut SolveApp) {
    if app.docs.closing.is_some() || app.docs.quitting.is_some() {
        return;
    }
    let Some(mut left) = app.docs.batch.take() else { return };
    while let Some(i) = left.pop() {
        if i >= app.docs.count() {
            continue;
        }
        if dirty(app, i) {
            app.docs.closing = Some(i);
            app.docs.batch = Some(left);
            return;
        }
        close(app, i, true);
    }
}

/// Quit (the title bar's ×, File ▸ Quit, the window being closed): asks "save changes?" for each
/// design with unsaved changes, then sets `app.quit_requested` for the host to close the window.
/// Cancel, or a Save that didn't save, stays open.
pub fn request_quit(app: &mut SolveApp) {
    if app.file_picker_busy() {
        return;
    }
    if app.docs.quitting.is_none() {
        app.docs.quitting = Some(Vec::new());
    }
    quit_step(app);
}

/// Ask about the next design with unsaved changes, or quit when none is left.
fn quit_step(app: &mut SolveApp) {
    let Some(skip) = app.docs.quitting.clone() else { return };
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

fn dirty(app: &SolveApp, i: usize) -> bool {
    if i == app.docs.active || app.docs.slots.is_empty() {
        return app.session.is_dirty();
    }
    app.docs.slots.get(i).and_then(Option::as_ref).is_some_and(|s| s.session.is_dirty())
}

/// Tab names (with an unsaved-changes flag), in order.
pub fn tabs_info(app: &SolveApp) -> Vec<(String, bool)> {
    if app.docs.slots.is_empty() {
        return vec![(app.session.doc.name.clone(), app.session.is_dirty())];
    }
    app.docs
        .slots
        .iter()
        .map(|s| match s {
            Some(d) => (d.session.doc.name.clone(), d.session.is_dirty()),
            None => (app.session.doc.name.clone(), app.session.is_dirty()),
        })
        .collect()
}

/// Where each tab's design is saved, in `tabs_info`'s order (`None`: not saved yet).
fn tab_paths(app: &SolveApp) -> Vec<Option<String>> {
    if app.docs.slots.is_empty() {
        return vec![app.session.path.clone()];
    }
    app.docs.slots.iter().map(|s| s.as_ref().map_or(&app.session, |d| &d.session).path.clone()).collect()
}

/// A tab's tooltip: where its design is saved (home shortened to `~`), or that it isn't yet; with
/// the full name above when the tab cut it short (#5868).
fn tab_tooltip(name: &str, path: Option<&str>, cut: bool) -> String {
    let place = path.map_or_else(|| crate::i18n::tr_label("Not saved yet").to_string(), crate::home::friendly_path);
    if cut { format!("{name}\n{place}") } else { place }
}

/// The designs a launch with no file to open reopens: those open at the last quit that still
/// exist, when Preferences › General asks for it (#5542).
pub fn designs_to_reopen(app: &SolveApp) -> Vec<String> {
    if !app.preferences.reopen_designs {
        return Vec::new();
    }
    app.preferences.open_designs.iter().filter(|p| std::path::Path::new(p.as_str()).is_file()).take(MAX_OPEN).cloned().collect()
}

/// The control channel's `ui.documents`: list, `new`, `switch`, `close` (by index; `force`
/// closes without asking), `close_others` (all but `index`), `close_all`, `move` (`from`, `to`)
/// or `quit`; `answer`: save | dont_save | cancel answers "save changes?"; `open_elsewhere`:
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
    json!({
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

/// A design tab being dragged along the strip to a new place: its index, and where the pointer
/// grabbed it (from the tab's left edge).
#[derive(Clone, Copy, Debug)]
struct TabDrag {
    from: usize,
    grab: f32,
}

/// The document tabs in the application bar, from `x0` to `x1`: click to switch, × to close,
/// drag to reorder, + for a new design.
pub fn tabs(app: &mut SolveApp, ui: &mut egui::Ui, r: Rect, x0: f32, x1: f32) {
    let t = Tokens::get();
    let info = tabs_info(app);
    let paths = tab_paths(app);
    let n = info.len();
    let (first, shown, w) = layout(n, app.docs.active, x1 - x0);
    let mut pick: Option<usize> = None;
    let mut shut: Option<usize> = None;
    // Each tab fits its name (up to MAX_FIT) while all of them fit; otherwise they share the room.
    const MAX_FIT: f32 = 200.0;
    let natural: Vec<f32> = info
        .iter()
        .map(|(name, dirty)| {
            let shown_name = crate::i18n::Locale::for_language(&app.preferences.language).given_name(name);
            let label = format!("{shown_name}{}", if *dirty { " •" } else { "" });
            let text = ui.painter().layout_no_wrap(label, crate::fonts::semibold(crate::theme::Tokens::get().font_button), t.app_bar_text).size().x;
            (text + 40.0).clamp(72.0, MAX_FIT)
        })
        .collect();
    let fit_all = shown == n && natural.iter().sum::<f32>() <= x1 - x0 - 34.0;
    // A right-click on a tab opens its menu (#3055): the tab and where the menu opens.
    let menu_id = egui::Id::new("sc_doc_tab_menu");
    let mut menu: Option<(usize, egui::Pos2)> = ui.ctx().data(|d| d.get_temp(menu_id));
    // The shown tabs' widths, in strip order.
    let widths: Vec<f32> = (first..first + shown).map(|i| if fit_all { natural.get(i).copied().unwrap_or(w) } else { w }).collect();
    let width = |i: usize| i.checked_sub(first).and_then(|k| widths.get(k)).copied().unwrap_or(w);
    let strip_end = x0 + widths.iter().sum::<f32>();
    let title_bar = app.custom_titlebar || app.integrated_titlebar;
    // A tab being dragged to a new place follows the pointer and the others make room for it;
    // Escape puts it back.
    let drag_id = egui::Id::new("sc_doc_tab_drag");
    let (held_down, released, escape) = ui.input(|i| (i.pointer.primary_down(), i.pointer.primary_released(), i.key_pressed(egui::Key::Escape)));
    let mut drag =
        ui.ctx().data(|d| d.get_temp::<TabDrag>(drag_id)).filter(|d| (held_down || released) && !escape && (first..first + shown).contains(&d.from));
    let pointer = ui.ctx().pointer_latest_pos();
    let held = drag.zip(pointer).map(|(d, p)| (d.from, (p.x - d.grab).clamp(x0, (strip_end - width(d.from)).max(x0))));
    let to = held.map(|(from, left)| (from, drop_slot(left, x0, &widths, first, from)));
    // Where each shown tab sits: in strip order, or with the dragged tab moved to its slot.
    let mut slots: Vec<usize> = (first..first + shown).filter(|&i| to.is_none_or(|(from, _)| from != i)).collect();
    if let Some((from, to)) = to {
        slots.insert(to.saturating_sub(first).min(slots.len()), from);
    }
    let mut lefts = vec![x0; shown];
    let mut at = x0;
    for &i in &slots {
        if let Some(l) = i.checked_sub(first).and_then(|k| lefts.get_mut(k)) {
            *l = at;
        }
        at += width(i);
    }
    // The dragged tab is drawn last, over the others.
    let mut order: Vec<usize> = (first..first + shown).filter(|&i| held.is_none_or(|(from, _)| from != i)).collect();
    order.extend(held.map(|(from, _)| from));
    let mut dropped: Option<(usize, usize)> = None;
    for i in order {
        let Some((name, dirty)) = info.get(i) else { continue };
        let w = width(i);
        let left = match held {
            Some((from, left)) if from == i => left,
            _ => i.checked_sub(first).and_then(|k| lefts.get(k)).copied().unwrap_or(x0),
        };
        let tr = Rect::from_min_size(pos2(left, r.top() + 4.0), vec2(w - 2.0, r.height() - 8.0));
        let active = !app.home.open && (i == app.docs.active || app.docs.slots.is_empty());
        let resp = ui.interact(tr, ui.id().with(("doc_tab", i)), Sense::click_and_drag());
        crate::scenario::publish_target(&format!("tab:design:{i}"), tr);
        // Dragging a tab along the strip moves it. The tabs are also part of the title bar:
        // dragging the only tab, or one mostly up or down, moves the window, and a double click
        // maximizes it.
        if resp.drag_started_by(egui::PointerButton::Primary) {
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(tr.center());
            let moved = pointer.unwrap_or(origin) - origin;
            if n > 1 && (!title_bar || moved.x.abs() >= moved.y.abs()) {
                drag = Some(TabDrag { from: i, grab: origin.x - tr.left() });
            } else if title_bar {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
        } else if title_bar && resp.double_clicked() {
            crate::titlebar::toggle_maximize(ui.ctx());
        }
        if resp.drag_stopped() && drag.is_some_and(|d| d.from == i) {
            // Released over the bar (not dropped somewhere down the window): the tab lands there.
            if let Some((from, to)) = to
                && pointer.is_some_and(|p| p.y < r.bottom() + r.height())
            {
                dropped = Some((from, to));
            }
            drag = None;
        }
        let lit = resp.hovered() || held.is_some_and(|(from, _)| from == i);
        tab_shape(ui.painter(), &t, tr, active, lit || resp.has_focus());
        let ink = if active || lit { t.app_bar_text } else { t.app_bar_text.gamma_multiply(0.7) };
        let name_display = crate::i18n::Locale::for_language(&app.preferences.language).given_name(name);
        let label = format!("{name_display}{}", if *dirty { " •" } else { "" });
        let font = if active {
            crate::fonts::semibold(crate::theme::Tokens::get().font_button)
        } else {
            FontId::proportional(crate::theme::Tokens::get().font_button)
        };
        let g = ui.painter().layout_no_wrap(label, font, ink);
        let text_w = (tr.width() - 30.0).max(10.0);
        let tip = tab_tooltip(&name_display, paths.get(i).cloned().flatten().as_deref(), g.size().x > text_w);
        let clip = Rect::from_min_size(tr.min, vec2(text_w + 10.0, tr.height()));
        ui.painter().with_clip_rect(clip).galley(pos2(tr.left() + 10.0, tr.center().y - g.size().y / 2.0), g, ink);
        // ×
        let xr = Rect::from_center_size(pos2(tr.right() - 13.0, tr.center().y), vec2(16.0, 16.0));
        let show_close = active || resp.hovered() || ui.rect_contains_pointer(tr);
        let xresp = ui.interact(xr, ui.id().with(("doc_close", i)), if show_close { Sense::click() } else { Sense::hover() });
        crate::scenario::publish_target(&format!("tab:close:{i}"), xr);
        if show_close && (xresp.hovered() || xresp.has_focus()) {
            ui.painter().rect_filled(xr, t.radius_sm.saturating_sub(1), t.app_bar_hover_strong);
        }
        if show_close {
            crate::icons::paint_app_bar(
                ui.painter(),
                Rect::from_center_size(xr.center(), vec2(14.0, 14.0)),
                "close",
                if xresp.hovered() || xresp.has_focus() { 1.0 } else { 0.55 },
            );
        }
        if resp.secondary_clicked() {
            menu = Some((i, ui.ctx().pointer_interact_pos().unwrap_or(tr.left_bottom())));
        }
        if xresp.on_hover_text("Close").clicked() {
            shut = Some(i);
        } else if (if held.is_some() { resp } else { resp.on_hover_text(tip) }).clicked() {
            pick = Some(i);
        }
    }
    ui.ctx().data_mut(|d| match drag {
        Some(v) => {
            d.insert_temp(drag_id, v);
        }
        None => d.remove::<TabDrag>(drag_id),
    });
    let mut x = strip_end;
    // More tabs than fit: a chevron lists them all.
    if shown < n {
        let mr = Rect::from_center_size(pos2(x + 12.0, r.center().y), vec2(20.0, 22.0));
        let mresp = ui.interact(mr, ui.id().with("doc_more"), Sense::click());
        crate::scenario::publish_handle("tab:overflow", mr.center());
        if mresp.hovered() {
            ui.painter().rect_filled(mr, t.radius_sm, t.app_bar_hover_button);
        }
        ui.painter().text(
            mr.center(),
            egui::Align2::CENTER_CENTER,
            format!("»{}", n - shown),
            FontId::proportional(crate::theme::Tokens::get().font_body),
            t.app_bar_text,
        );
        let id = egui::Id::new("sc_doc_more");
        if mresp.clone().on_hover_text(crate::i18n::tr_label("All open designs")).clicked() {
            ui.ctx().data_mut(|d| d.insert_temp(id, !d.get_temp::<bool>(id).unwrap_or(false)));
        }
        if ui.ctx().data(|d| d.get_temp::<bool>(id).unwrap_or(false)) {
            let resp = Container::new(ui.ctx(), Surface::Popup)
                .area(id.with("area"))
                .fixed_pos(pos2(mr.left(), r.bottom()))
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    crate::context_menu::menu_frame(ui, |ui| {
                        for (i, (name, dirty)) in info.iter().enumerate() {
                            let label = format!("{name}{}", if *dirty { " •" } else { "" });
                            if ui_kit::list_picker(ui, &format!("document:{i}"), &label, i == app.docs.active).clicked() {
                                pick = Some(i);
                            }
                        }
                    })
                });
            let pressed_outside = ui.ctx().input(|i| i.pointer.any_pressed()) && !resp.response.contains_pointer() && !mresp.contains_pointer();
            if pick.is_some() || pressed_outside {
                ui.ctx().data_mut(|d| d.insert_temp(id, false));
            }
        }
        x += 26.0;
    }
    // The tab's menu: close it, the others, or all of them.
    let mut many: Option<Vec<usize>> = None;
    if let Some((i, at)) = menu.filter(|(i, _)| *i < n) {
        let items = [("ui.closeTab", "Close"), ("ui.closeOtherTabs", "Close Other Tabs"), ("ui.closeAllTabs", "Close All Tabs")];
        let resp =
            Container::new(ui.ctx(), Surface::Popup).area(menu_id.with("area")).fixed_pos(at).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
                crate::context_menu::menu_frame(ui, |ui| {
                    let mut chosen = None;
                    for (id, label) in items {
                        let mut item = crate::context_menu::Item::action(id, label, "");
                        item.enabled = id != "ui.closeOtherTabs" || n > 1;
                        let top = ui.cursor().top();
                        let row = crate::context_menu::list_row(ui, &item);
                        let rect = Rect::from_min_max(pos2(ui.min_rect().left(), top), pos2(ui.min_rect().right(), ui.cursor().top()));
                        crate::scenario::publish_handle(&format!("tab_menu:{id}"), rect.center());
                        if row.is_some() {
                            chosen = Some(id);
                        }
                    }
                    chosen
                })
            });
        match resp.inner {
            Some("ui.closeTab") => shut = Some(i),
            Some("ui.closeOtherTabs") => many = Some((0..n).filter(|k| *k != i).collect()),
            Some("ui.closeAllTabs") => many = Some((0..n).collect()),
            _ => {}
        }
        let pressed_outside = ui.ctx().input(|x| x.pointer.any_pressed()) && !resp.response.contains_pointer();
        menu = if resp.inner.is_some() || pressed_outside || ui.ctx().input(|x| x.key_pressed(egui::Key::Escape)) { None } else { Some((i, at)) };
    }
    ui.ctx().data_mut(|d| match menu {
        Some(m) => {
            d.insert_temp(menu_id, m);
        }
        None => {
            d.remove::<(usize, egui::Pos2)>(menu_id);
        }
    });
    // Choose a new design, open or sample from the File view before creating a tab.
    let pr = Rect::from_center_size(pos2(x + 14.0, r.center().y), vec2(22.0, 22.0));
    let presp = ui.interact(pr, ui.id().with("doc_new"), Sense::click());
    if presp.hovered() {
        ui.painter().rect_filled(pr, t.radius_sm, t.app_bar_hover_button);
    }
    crate::icons::paint_app_bar(
        ui.painter(),
        Rect::from_center_size(pr.center(), vec2(16.0, 16.0)),
        "plus",
        if presp.hovered() { 1.0 } else { 0.55 },
    );
    // A file loading into the window keeps the tabs as they are (dragging still moves it).
    let idle = !app.file_load.busy();
    crate::scenario::publish_handle("tab:new", pr.center());
    if presp.on_hover_text(app.tr("ui.start_page")).clicked() && idle {
        app.home.show(app.session.revision);
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

/// The place among the shown tabs (`first..`, `widths` wide in strip order from `x0`) for
/// tab `from` whose left edge is dragged to `left`: the slot whose place for it is nearest.
pub fn drop_slot(left: f32, x0: f32, widths: &[f32], first: usize, from: usize) -> usize {
    let mut best = (0, f32::INFINITY);
    let mut at = x0;
    let others = widths.iter().enumerate().filter(|(k, _)| first + k != from).map(|(_, w)| *w);
    for (k, w) in others.map(Some).chain([None]).enumerate() {
        if (left - at).abs() < best.1 {
            best = (k, (left - at).abs());
        }
        at += w.unwrap_or(0.0);
    }
    first + best.0
}

/// Tabs are rounded chips on the bar; the active design has the stronger fill.
fn tab_shape(painter: &egui::Painter, t: &Tokens, r: Rect, active: bool, hovered: bool) {
    let fill = if active {
        t.app_bar_tab_active
    } else if hovered {
        t.app_bar_hover
    } else {
        egui::Color32::TRANSPARENT
    };
    painter.rect_filled(r, t.radius_md, fill);
}

/// Which tabs the strip shows in `room` points: (first, how many, tab width). Tabs shrink to
/// `MIN_TAB`; past that a window of tabs around the active one is shown and the rest go in the »
/// list.
pub fn layout(n: usize, active: usize, room: f32) -> (usize, usize, f32) {
    const MIN_TAB: f32 = 90.0;
    let n = n.max(1);
    let avail = (room - 34.0).max(MIN_TAB);
    if avail / n as f32 >= MIN_TAB {
        return (0, n, (avail / n as f32).min(220.0));
    }
    // Leave room for the » button.
    let fit = (((avail - 26.0) / MIN_TAB).floor() as usize).max(1);
    let first = active.saturating_sub(fit - 1).min(n - fit);
    (first, fit, MIN_TAB)
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
    let _language = crate::i18n::scope(&app.preferences.language);
    quit_step(app);
    let Some(i) = app.docs.closing else { return };
    let name = tabs_info(app).get(i).map(|x| x.0.clone()).unwrap_or_default();
    let name_display = crate::i18n::tr_name(&name);
    let mut answer: Option<Answer> = None;
    // Left and Right move between the buttons as they are laid out (Save is picked first, at the
    // right); Enter gives the picked one.
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
    let choice = order.get(at).copied().unwrap_or_default();
    app.docs.choice = choice;
    if enter {
        answer = Some(choice);
    }
    let title = crate::i18n::format("document.save_changes", &[("name", &name_display)]);
    let (closed, action) = ui_kit::Confirm::new(egui::Id::new("sc_close_prompt"), "status_warning", &title)
        .tone(ui_kit::Tone::Warning)
        .actions(crate::i18n::tr("ui.save"), crate::i18n::tr("ui.cancel"), Some(crate::i18n::tr("ui.don_t_save")))
        .secondary_left()
        .selected(match choice {
            Answer::Save => ui_kit::Action::Ok,
            Answer::DontSave => ui_kit::Action::Apply,
            Answer::Cancel => ui_kit::Action::Cancel,
        })
        .show(ctx, true, |ui| {
            ui_kit::text(ui, &crate::i18n::format("document.unsaved_changes", &[("name", &name_display)]), ui_kit::Text::Report("unsaved", false));
        });
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
    if app.file_picker_busy() {
        return;
    }
    let Some(i) = app.docs.closing else { return };
    app.docs.choice = Answer::Save;
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
        }
    }
    quit_step(app);
    batch_step(app);
}

/// Resume Close/Quit only after Save As has answered and the write succeeded.
pub(crate) fn saved_for_close(app: &mut SolveApp) {
    if !std::mem::take(&mut app.docs.saving) || app.docs.closing.is_none() {
        return;
    }
    let saved = !app.session.is_dirty();
    app.docs.closing = None;
    if app.docs.quitting.is_some() {
        if !saved {
            app.docs.quitting = None;
        }
    } else if saved {
        close(app, app.docs.active, true);
    } else {
        // Save As cancelled: the tabs not closed yet stay.
        app.docs.batch = None;
    }
    quit_step(app);
    batch_step(app);
}

#[cfg(test)]
mod tests {
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

    /// #5868: a tab's tooltip says where its design is saved (home as `~`), or that it isn't
    /// yet, with the full name above when the tab cut the name short.
    #[test]
    fn tab_tooltip_says_where_the_design_is_saved() {
        assert_eq!(tab_tooltip("plate", Some("/srv/designs/plate.solidcraft"), false), "/srv/designs/plate.solidcraft");
        assert_eq!(tab_tooltip("a-long-name", Some("/srv/a-long-name.solidcraft"), true), "a-long-name\n/srv/a-long-name.solidcraft");
        assert_eq!(tab_tooltip("Untitled", None, false), "Not saved yet");
        assert_eq!(tab_tooltip("Untitled", None, true), "Untitled\nNot saved yet");
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).filter(|h| !h.is_empty()) {
            // The abbreviation keeps the relative path's native separators after `~/`.
            let relative = std::path::Path::new("Designs").join("bracket.solidcraft");
            let path = std::path::Path::new(&home).join(&relative);
            assert_eq!(tab_tooltip("bracket", path.to_str(), false), format!("~/{}", relative.display()));
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
}
