//! The operations panel: every long operation under way, one row each, in the view's bottom-right
//! corner above the frame-rate readout (Preferences ▸ Layout ▸ "Show operations in the view"). A row is
//! the operation's short name, a progress bar (with the step it is on of how many, when the work
//! counts its steps) and a red X that cancels it. A new operation goes on top of the ones running;
//! one that ends fades out, and the rows above it move down into its place.
//!
//! The rows are read each frame from the work itself (the file being opened and the ones queued
//! behind it, a feature dialog's OK computing, the history's commands and a delete's check
//! computing off the window thread, an agent's command, a save, the bodies of an opened design
//! being meshed), so nothing has to be registered or forgotten. Work reports its progress, and
//! learns it was cancelled, through [`solidcraft_engine::doc::progress::Progress`]: the timeline's
//! evaluation reports each feature it reaches, and long work inside one reports its steps through
//! the kernel's watch ([`solidcraft_engine::kernel::watch::Watch`]); each stops at its next check
//! once cancelled. Cancel from the control channel with `ui.operations` ([`control`]).

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};

use crate::SolveApp;
use crate::theme::Tokens;

/// How long a finished operation takes to fade out (ms).
pub const FADE_MS: f64 = 400.0;

/// How long the rows take to move into the place of one that went (s).
const SLIDE_S: f32 = 0.2;

/// The panel's width, and a row's height.
const WIDTH: f32 = 300.0;
const ROW: f32 = 30.0;

/// The panel's distance from the view's edges and from what is below it.
const MARGIN: f32 = 12.0;

/// One operation under way, as the panel shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// Stays the same for as long as the operation waits and runs: its kind, and its
    /// [`serial`] where several can be under way.
    pub key: String,
    /// The operation's short name.
    pub label: String,
    /// Steps done of how many, when the work counts them.
    pub steps: Option<(usize, usize)>,
    /// Waiting for the one before it.
    pub queued: bool,
    /// Asked to stop, and finishing its current step.
    pub cancelling: bool,
    /// Gone because it was cancelled (fading out).
    pub cancelled: bool,
    /// How to cancel it; `None`: it can't be (a save being written).
    cancel: Option<Cancel>,
    /// An agent's command: drawn in the agent cursor's colour.
    agent: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Cancel {
    /// The running file operation.
    File,
    /// The `n`th queued file operation (from 0).
    FileQueued(usize),
    /// The feature dialog's OK, computing.
    Apply,
    /// The history's command computing off the window thread.
    History,
    /// The `n`th history command waiting its turn (from 0).
    HistoryQueued(usize),
    /// A delete's check.
    DeleteCheck,
    /// Opening a feature's editor.
    FeatureEdit,
    /// An agent's command.
    Agent,
}

impl Row {
    fn new(key: String, label: String) -> Self {
        Row { key, label, steps: None, queued: false, cancelling: false, cancelled: false, cancel: None, agent: false }
    }

    /// Whether its X does anything.
    pub fn cancellable(&self) -> bool {
        self.cancel.is_some() && !self.cancelling
    }

    /// The bar's fill (0..=1), or `None` while the work hasn't said how far it is.
    pub fn fraction(&self) -> Option<f32> {
        let (done, total) = self.steps.filter(|(_, total)| *total > 0)?;
        Some((done.min(total) as f32 / total as f32).clamp(0.0, 1.0))
    }

    /// The text over the bar: the step it is on of how many (`3 / 12`), queued or cancelling.
    pub fn status(&self) -> Option<String> {
        if self.cancelled {
            return Some(crate::i18n::tr("operations.stopped").to_string());
        }
        if self.cancelling {
            return Some(crate::i18n::tr("operations.cancelling").to_string());
        }
        if self.queued {
            return Some(crate::i18n::tr("operations.queued").to_string());
        }
        let (done, total) = self.steps.filter(|(_, total)| *total > 1)?;
        Some(format!("{} / {total}", (done + 1).min(total)))
    }
}

/// A new operation's number, which names its row for as long as it waits, runs and runs again.
pub fn serial() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

fn command_name(id: &str) -> String {
    crate::i18n::tr_label(crate::command_label(id)).to_string()
}

/// The operations under way, oldest first.
pub fn current(app: &SolveApp) -> Vec<Row> {
    let mut rows = Vec::new();
    // The bodies of a design just opened, meshed for the view.
    if let Some((done, total)) = crate::open_meshing::progress(app) {
        rows.push(Row { steps: Some((done, total)), ..Row::new("meshing".into(), crate::i18n::tr("operations.meshing").to_string()) });
    }
    if let Some(saving) = &app.saving {
        let command = saving.command();
        rows.push(Row::new(format!("save:{command}"), command_name(command)));
    }
    // A file being opened, inserted or exported, and those waiting behind it.
    let mut queued = 0;
    for file in app.file_load.operations() {
        let key = format!("file:{}", file.serial);
        let label = if file.name.is_empty() { command_name(file.command) } else { format!("{} {}", command_name(file.command), file.name) };
        let cancel = if file.queued {
            queued += 1;
            Cancel::FileQueued(queued - 1)
        } else {
            Cancel::File
        };
        rows.push(Row { steps: file.steps, queued: file.queued, cancelling: file.cancelled, cancel: Some(cancel), ..Row::new(key, label) });
    }
    if let Some(job) = &app.applying {
        let command: Option<&str> = match app.dialog.as_ref().and_then(crate::dialogs::Dialog::command) {
            Some(command) => Some(command),
            None => job.cmds.first().map(|(id, _)| id.as_str()),
        };
        let label = command.map_or_else(|| crate::i18n::tr("operations.feature").to_string(), command_name);
        rows.push(Row { steps: evaluation_steps(&job.progress), cancel: Some(Cancel::Apply), ..Row::new("apply".into(), label) });
    }
    if let Some(edit) = &app.pending_feature_edit {
        let cancelling = !edit.opens();
        rows.push(Row {
            cancelling,
            cancel: Some(Cancel::FeatureEdit),
            ..Row::new("feature_edit".into(), crate::i18n::tr("operations.feature_edit").to_string())
        });
    }
    if let Some(check) = &app.delete_check {
        rows.push(Row {
            steps: evaluation_steps(&check.progress),
            cancel: Some(Cancel::DeleteCheck),
            ..Row::new("delete_check".into(), command_name("selection.delete"))
        });
    }
    // The history's command computing, then those waiting their turn.
    if let Some(job) = &app.deferred {
        let key = format!("history:{}", job.serial);
        rows.push(Row { steps: evaluation_steps(&job.progress), cancel: Some(Cancel::History), ..Row::new(key, command_name(&job.command.0)) });
    }
    for (n, (id, _, serial)) in app.deferred_queue.iter().enumerate() {
        rows.push(Row { queued: true, cancel: Some(Cancel::HistoryQueued(n)), ..Row::new(format!("history:{serial}"), command_name(id)) });
    }
    if app.agent_computing().is_some() {
        let steps = app.agent_steps();
        let label = steps.as_ref().map(|s| s.current()).or_else(|| app.agent_computing().map(|(_, label)| label)).unwrap_or_default();
        rows.push(Row { steps: steps.map(|s| s.counts()), cancel: Some(Cancel::Agent), agent: true, ..Row::new("agent".into(), label) });
    }
    rows
}

/// How far a watched evaluation is: the long work inside one feature while there is some, else
/// the features evaluated of the timeline's.
fn evaluation_steps(progress: &solidcraft_engine::doc::progress::Progress) -> Option<(usize, usize)> {
    if let Some((_, done, total)) = progress.work().filter(|(_, done, total)| done < total) {
        return Some((done, total));
    }
    let (done, total) = progress.features();
    (total > 0).then_some((done, total))
}

/// Cancel the operation `key` names (as [`current`] lists it). Its work stops at its next check
/// and nothing of it is applied; operations queued behind it go on.
pub fn cancel(app: &mut SolveApp, key: &str) -> Result<(), String> {
    let row = current(app).into_iter().find(|r| r.key == key).ok_or_else(|| format!("no operation `{key}` is under way"))?;
    let Some(how) = row.cancel else { return Err(crate::i18n::format("operations.cannot_cancel", &[("name", &row.label)])) };
    if row.cancelling {
        return Ok(());
    }
    match how {
        Cancel::File => app.file_load.cancel_running(),
        Cancel::FileQueued(n) => app.file_load.cancel_queued(n),
        // Dropping the job stops its worker; the dialog stays open with its inputs.
        Cancel::Apply => app.applying = None,
        Cancel::History => {
            app.deferred = None;
            app.start_after_roll = None;
        }
        Cancel::HistoryQueued(n) => {
            if n < app.deferred_queue.len() {
                app.deferred_queue.remove(n);
            }
        }
        Cancel::DeleteCheck => {
            if let Some(check) = app.delete_check.take() {
                check.progress.cancel();
            }
        }
        Cancel::FeatureEdit => {
            if let Some(edit) = app.pending_feature_edit.as_mut() {
                edit.cancel();
            }
        }
        Cancel::Agent => app.stop_agent_command(),
    }
    app.operations.cancelled.push(row.key.clone());
    app.set_status(crate::i18n::format("operations.cancelled", &[("name", &row.label)]), false);
    Ok(())
}

/// A row on screen: what it showed last, and since when it is gone (fading out).
#[derive(Clone, Debug)]
struct Shown {
    row: Row,
    gone_at: Option<f64>,
}

/// The panel's rows, kept from frame to frame for the fades.
#[derive(Clone, Debug, Default)]
pub struct Panel {
    /// Oldest first: the bottom row.
    shown: Vec<Shown>,
    /// Where the panel was drawn this frame, for the toasts to keep clear of.
    pub(crate) rect: Option<Rect>,
    /// Operations cancelled, which fade out as cancelled rather than as done.
    cancelled: Vec<String>,
}

impl Panel {
    /// Take in the operations under way `now` (ms), oldest first: a new one goes on top, one
    /// still running is brought up to date where it is, and one gone starts fading; rows that
    /// faded out go.
    pub fn update(&mut self, rows: Vec<Row>, now: f64) {
        for shown in &mut self.shown {
            match rows.iter().find(|r| r.key == shown.row.key) {
                Some(row) => (shown.row, shown.gone_at) = (row.clone(), None),
                None if shown.gone_at.is_none() => {
                    shown.gone_at = Some(now);
                    // Nothing left to cancel. It finished: a full bar while it fades; it was
                    // cancelled: the bar where it stopped, saying so.
                    if self.cancelled.contains(&shown.row.key) || shown.row.cancelling {
                        shown.row.cancelled = true;
                    } else {
                        shown.row.steps = shown.row.steps.map(|(_, total)| (total, total));
                    }
                    shown.row.cancel = None;
                    shown.row.cancelling = false;
                    shown.row.queued = false;
                }
                None => {}
            }
        }
        self.shown.retain(|s| s.gone_at.is_none_or(|gone| now - gone < FADE_MS));
        let shown = &self.shown;
        self.cancelled.retain(|key| shown.iter().any(|s| s.row.key == *key));
        for row in rows {
            if !self.shown.iter().any(|s| s.row.key == row.key) {
                self.shown.push(Shown { row, gone_at: None });
            }
        }
    }

    /// The rows on screen, bottom first, with how opaque each is (fading out: less).
    pub fn rows(&self, now: f64) -> impl Iterator<Item = (&Row, f32)> {
        self.shown.iter().map(move |s| (&s.row, s.gone_at.map_or(1.0, |gone| (1.0 - (now - gone) / FADE_MS).clamp(0.0, 1.0) as f32)))
    }

    /// Whether anything is on screen.
    pub fn is_empty(&self) -> bool {
        self.shown.is_empty()
    }
}

/// Bring the panel up to date and draw it in `view`'s bottom-right corner, above the frame-rate
/// readout and its details, left of a pane over that corner. Off in Preferences: nothing.
pub(crate) fn show(app: &mut SolveApp, ui: &egui::Ui, view: Rect) {
    app.operations.rect = None;
    if !app.preferences.operations_panel {
        app.operations = Panel::default();
        return;
    }
    let now = crate::now_ms();
    let rows = current(app);
    app.operations.update(rows, now);
    if app.operations.is_empty() {
        return;
    }
    let ctx = ui.ctx().clone();
    let t = Tokens::get();
    let painter = ui.painter().clone();
    let (pad, gap) = (t.space_4, t.space_3);
    // In the bottom-right corner, above the frame-rate readout, beside the panes over the view
    // and above the navigation bar where they meet.
    let n = app.operations.shown.len() as f32;
    let height = 2.0 * pad + n * ROW + (n - 1.0).max(0.0) * gap;
    let bottom = app.frame_rate.top.filter(|top| top.is_finite() && *top > view.top()).map_or(view.bottom() - MARGIN, |top| top - 6.0);
    let at = Rect::from_min_max(pos2(view.right() - MARGIN - WIDTH, bottom - height), pos2(view.right() - MARGIN, bottom));
    let mut column = crate::frame_rate::beside_panes(at, view, &crate::shell::covered_rects(app));
    if let Some(nav) = app.viewport.layout.as_ref().map(|l| l.navigation).filter(|n| n.is_positive() && n.intersects(column.expand(4.0))) {
        column = column.translate(vec2(0.0, nav.top() - 6.0 - column.bottom()));
    }
    let bottom = column.bottom();
    // Each row slides to its place: the rows above one that went move down into its place.
    let mut target = bottom - pad;
    let mut placed = Vec::new();
    for (row, alpha) in app.operations.rows(now) {
        let y = ctx.animate_value_with_time(egui::Id::new(("sc_operation_y", &row.key)), target, SLIDE_S);
        placed.push((row.clone(), alpha, y));
        target -= ROW + gap;
    }
    let Some(top) = placed.iter().map(|(_, _, y)| *y - ROW).reduce(f32::min) else { return };
    let panel = Rect::from_min_max(pos2(column.left(), top - pad), pos2(column.right(), bottom));
    if panel.height() <= 0.0 || panel.top() < view.top() || panel.width() < 120.0 {
        return;
    }
    // Clicks on the panel are its own, not the view's.
    let _ = ui.interact(panel, egui::Id::new("sc_operations"), Sense::click());
    painter.rect(panel, t.radius_sm, t.overlay, t.control_border_stroke(t.border), egui::StrokeKind::Inside);
    crate::scenario::publish_rect("operations:panel", panel);
    app.operations.rect = Some(panel);
    let mut cancel = None;
    for (row, alpha, y) in placed {
        let r = Rect::from_min_max(pos2(panel.left() + pad, y - ROW), pos2(panel.right() - pad, y));
        if paint_row(ui, &painter, &t, &row, alpha, r) {
            cancel = Some(row.key.clone());
        }
    }
    if let Some(key) = cancel {
        let _ = self::cancel(app, &key);
    }
    // The bars and fades move: keep drawing while the panel shows.
    ctx.request_repaint_after(std::time::Duration::from_millis(33));
}

/// One row in `r`: name, bar with its step, and the X. True when the X was clicked.
fn paint_row(ui: &egui::Ui, painter: &egui::Painter, t: &Tokens, row: &Row, alpha: f32, r: Rect) -> bool {
    let fade = |c: Color32| c.gamma_multiply(alpha);
    let accent = if row.agent { crate::icons::pointer::agent() } else { t.form_primary };
    crate::scenario::publish_rect(&format!("operations:row:{}", row.key), r);
    // The X on the right.
    let x = Rect::from_center_size(pos2(r.right() - ROW * 0.5, r.center().y), vec2(ROW - 6.0, ROW - 6.0));
    let response = ui.interact(x, egui::Id::new(("sc_operation_cancel", &row.key)), if row.cancellable() { Sense::click() } else { Sense::hover() });
    let response = response.on_hover_text(if row.cancellable() {
        crate::i18n::format("operations.cancel", &[("name", &row.label)])
    } else {
        crate::i18n::format("operations.cannot_cancel", &[("name", &row.label)])
    });
    if row.cancellable() && response.hovered() {
        painter.rect_filled(x, t.radius_sm, fade(t.hover));
    }
    let colour = if row.cancellable() { fade(t.error) } else { fade(t.text_dim).gamma_multiply(0.5) };
    let arm = x.shrink(x.width() * 0.3);
    let stroke = Stroke::new(2.0, colour);
    painter.line_segment([arm.left_top(), arm.right_bottom()], stroke);
    painter.line_segment([arm.right_top(), arm.left_bottom()], stroke);
    crate::scenario::publish_handle(&format!("operations:cancel:{}", row.key), x.center());
    // The name on the left, cut short to its share of the row.
    let name_w = (r.width() - ROW) * 0.4;
    let font = FontId::proportional(t.font_label);
    let job = egui::text::LayoutJob {
        wrap: egui::text::TextWrapping::truncate_at_width(name_w - 6.0),
        ..egui::text::LayoutJob::simple_singleline(row.label.clone(), font, fade(t.text))
    };
    let galley = painter.layout_job(job);
    painter.galley(pos2(r.left(), r.center().y - galley.size().y * 0.5), galley, fade(t.text));
    // The bar between them, the step over it.
    let bar = Rect::from_min_max(pos2(r.left() + name_w, r.center().y - 7.0), pos2(x.left() - 6.0, r.center().y + 7.0));
    painter.rect_filled(bar, t.space_3, fade(t.dialog_rule));
    match row.fraction() {
        Some(f) if f > 0.0 => {
            painter.rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * f, bar.height())), t.space_3, fade(accent));
        }
        Some(_) => {}
        // Not told how far it is: a stripe going to and fro.
        None if !row.queued && !row.cancelling && !row.cancelled => {
            let phase = (ui.ctx().input(|i| i.time) * 0.8).fract() as f32;
            let w = bar.width() * 0.3;
            let left = bar.left() + (bar.width() - w) * (1.0 - (phase * 2.0 - 1.0).abs());
            painter.rect_filled(Rect::from_min_size(pos2(left, bar.top()), vec2(w, bar.height())), t.space_3, fade(accent));
        }
        None => {}
    }
    if let Some(status) = row.status() {
        painter.text(bar.center(), Align2::CENTER_CENTER, status, FontId::monospace(t.font_small), fade(t.text));
    }
    row.cancellable() && response.clicked()
}

/// `ui.operations`: list the operations under way (`{}`), or cancel one (`{"cancel": key}`).
pub fn control(app: &mut SolveApp, p: &Value) -> Result<Value, String> {
    if let Some(key) = p.get("cancel").and_then(Value::as_str) {
        return cancel(app, key).map(|()| json!({"cancelled": key}));
    }
    let rows: Vec<Value> = current(app)
        .iter()
        .rev()
        .map(|r| {
            json!({
                "key": r.key,
                "label": r.label,
                "steps": r.steps.map(|(done, total)| json!({"done": done, "total": total})),
                "queued": r.queued,
                "cancelling": r.cancelling,
                "cancellable": r.cancellable(),
            })
        })
        .collect();
    Ok(json!({"operations": rows, "shown": app.operations.rect.is_some()}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(key: &str, steps: Option<(usize, usize)>) -> Row {
        Row { steps, cancel: Some(Cancel::Apply), ..Row::new(key.into(), key.into()) }
    }

    /// A new operation goes on top; one that ends shows a full bar, can't be cancelled and fades
    /// out over `FADE_MS`, then goes, and the rows above it take its place.
    #[test]
    fn new_rows_go_on_top_and_ended_ones_fade_out_then_go() {
        let mut panel = Panel::default();
        panel.update(vec![row("a", Some((1, 4)))], 0.0);
        panel.update(vec![row("a", Some((2, 4))), row("b", None)], 100.0);
        let keys: Vec<&str> = panel.rows(100.0).map(|(r, _)| r.key.as_str()).collect();
        assert_eq!(keys, ["a", "b"], "bottom first: the newer is above");
        assert_eq!(panel.rows(100.0).next().unwrap().0.steps, Some((2, 4)), "brought up to date in place");
        // `a` ends.
        panel.update(vec![row("b", None)], 200.0);
        let (a, alpha) = panel.rows(200.0 + FADE_MS / 2.0).next().unwrap();
        assert_eq!((a.key.as_str(), a.steps, a.cancellable()), ("a", Some((4, 4)), false));
        assert!((alpha - 0.5).abs() < 1e-3, "{alpha}");
        panel.update(vec![row("b", None)], 200.0 + FADE_MS);
        let keys: Vec<&str> = panel.rows(200.0 + FADE_MS).map(|(r, _)| r.key.as_str()).collect();
        assert_eq!(keys, ["b"]);
        // One cancelled fades out where it stopped, saying so.
        panel.update(vec![row("b", Some((3, 9)))], 300.0 + FADE_MS);
        panel.cancelled.push("b".into());
        panel.update(Vec::new(), 310.0 + FADE_MS);
        let (b, _) = panel.rows(310.0 + FADE_MS).next().unwrap();
        assert_eq!((b.steps, b.cancelled, b.status()), (Some((3, 9)), true, Some(crate::i18n::tr("operations.stopped").to_string())));
        panel.update(Vec::new(), 400.0 + 2.0 * FADE_MS);
        assert!(panel.is_empty() && panel.cancelled.is_empty());
        // One that comes back while fading is running again.
        panel.update(vec![row("b", None), row("c", None)], 1000.0);
        panel.update(vec![row("c", None)], 1010.0);
        panel.update(vec![row("b", None), row("c", None)], 1020.0);
        assert!(panel.rows(1020.0).all(|(_, alpha)| alpha == 1.0));
    }

    #[test]
    fn steps_read_as_the_step_on_of_how_many() {
        let r = row("a", Some((2, 12)));
        assert_eq!(r.status().as_deref(), Some("3 / 12"));
        assert_eq!(r.fraction(), Some(2.0 / 12.0));
        assert_eq!(row("a", Some((12, 12))).status().as_deref(), Some("12 / 12"));
        assert_eq!(row("a", Some((0, 1))).status(), None, "one step: just the bar");
        assert_eq!(row("a", None).fraction(), None);
        assert_eq!(row("a", Some((5, 0))).fraction(), None, "hostile counts");
    }

    /// A history command computing off the window thread shows as a row with its command's name
    /// and how far its evaluation is; the panel's X (here through `ui.operations`) cancels it:
    /// its worker stops, and nothing of it is applied.
    #[test]
    fn a_history_command_computing_is_listed_and_cancelled() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        h.app.run("solid.box", json!({"length": 5, "width": 5, "height": 20})).unwrap();
        let features = h.app.session.doc.features.len();
        h.app.apply_off_thread = Some(std::time::Duration::ZERO);
        h.app.run_heavy("edit.undo", json!({})).unwrap();
        h.app.run_heavy("edit.undo", json!({})).unwrap();
        let listed = control(&mut h.app, &json!({})).unwrap();
        let rows = listed["operations"].as_array().unwrap().clone();
        assert_eq!(rows.len(), 2, "{listed}");
        let key = |i: usize| rows[i]["key"].as_str().unwrap().to_string();
        assert_eq!((rows[0]["queued"].clone(), rows[1]["queued"].clone()), (json!(true), json!(false)), "newest first: {listed}");
        assert_eq!(rows[1]["label"], command_name("edit.undo"));
        let progress = h.app.deferred.as_ref().unwrap().progress.clone();
        // The waiting one goes, and the running one keeps its key; then it stops.
        control(&mut h.app, &json!({"cancel": key(0)})).unwrap();
        assert!(h.app.deferred_queue.is_empty());
        assert_eq!(current(&h.app).iter().map(|r| r.key.clone()).collect::<Vec<_>>(), [key(1)]);
        control(&mut h.app, &json!({"cancel": key(1)})).unwrap();
        assert!(h.app.deferred.is_none() && progress.is_cancelled());
        h.frames(3);
        assert_eq!(h.app.session.doc.features.len(), features, "nothing was undone");
        assert!(control(&mut h.app, &json!({"cancel": key(1)})).is_err(), "gone");
    }

    /// The panel shows the rows in the view's corner, above the frame-rate readout, the newer
    /// above the older; off in Preferences it doesn't.
    #[test]
    fn the_panel_sits_in_the_corner_above_the_frame_rate_and_the_preference_turns_it_off() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.show_frame_rate = true;
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        h.frames(3);
        // A command computing (held until `release` goes) and one waiting behind it.
        let (job, release) = crate::deferred::Deferred::held("edit.undo");
        h.app.deferred = Some(job);
        let (running, waiting) = (h.app.deferred.as_ref().unwrap().serial, serial());
        h.app.deferred_queue.push(("edit.redo".into(), json!({}), waiting));
        h.frames(2);
        let panel = crate::scenario::rect_of("operations:panel").expect("the panel is drawn");
        let view = h.app.viewport.rect.unwrap();
        assert!(view.contains_rect(panel), "{panel:?} in {view:?}");
        assert!(panel.right() > view.center().x && panel.bottom() > view.center().y, "bottom right: {panel:?} in {view:?}");
        if let Some(readout) = crate::scenario::rect_of("viewport:frame_rate") {
            assert!(panel.bottom() <= readout.top(), "above the readout: {panel:?} {readout:?}");
        }
        let older = crate::scenario::rect_of(&format!("operations:row:history:{running}")).unwrap();
        let newer = crate::scenario::rect_of(&format!("operations:row:history:{waiting}")).unwrap();
        assert!(newer.bottom() <= older.top() + 0.5, "the newer on top: {newer:?} {older:?}");
        assert!(crate::scenario::handle_at(&format!("operations:cancel:history:{running}")).is_some());
        h.app.preferences.operations_panel = false;
        h.frame();
        assert!(h.app.operations.rect.is_none() && crate::scenario::rect_of("operations:panel").is_none());
        drop(release);
    }
}
