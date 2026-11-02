//! The operation a feature window chooses by itself, from what its material does (#6901):
//!
//! - New Body when the material, joined, would be a separate piece of the body (or there is no
//!   shown body to join), Join when it makes one body with what it touches, and, for Revolve and
//!   the Box, Cylinder, Sphere and Torus, Cut when it lies inside a body. Extrude keeps its own
//!   Cut ([`crate::smart_extrude`]). Pipe, Thicken and Boundary Fill don't choose.
//! - What the material does is found by running the Join on a copy of the design, off the window
//!   thread ([`OpCheck`]), so the choice is what OK makes.
//! - The choice changes only when a finished check says so: no guess in between.
//! - An operation picked by hand stays for good, whatever a check still running says; OK runs
//!   what the window shows, and while the window still chooses, waits for its check.
//! - A feature being edited keeps its operation.
//! - With Preferences › Design › "Choose the operation automatically" off (`auto_operation`), no
//!   window chooses: each keeps the operation it opens with, Extrude its own Join or Cut.
//! - Only shown bodies take part: when some bodies are hidden, the window names the shown ones as
//!   the feature's `targets` ([`scope_targets`]).

use serde_json::Value;
use solidcraft_engine::Addition;

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind, OPS};

/// Operation indices (see [`OPS`]).
pub const NEW: usize = 0;
pub const JOIN: usize = 1;
pub const CUT: usize = 2;

/// A feature window's own choice of operation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OpAuto {
    /// The operation was picked by hand: the window no longer chooses.
    pub manual: bool,
    /// The operation this window last chose (one that differs now was picked by hand).
    set: Option<usize>,
    /// Extrude: the additive operation the last finished check chose, held while the next runs.
    held: Option<usize>,
    /// A check for the current inputs is running.
    pub pending: bool,
    /// OK (with Apply's "again") waits for that check.
    pub ok_waiting: Option<bool>,
}

/// How a window chooses.
#[derive(Clone, Copy)]
struct Rule {
    /// Extrude: its own rule decides Cut; this one only New Body against Join.
    extrude: bool,
    /// Material inside a body is a Cut.
    cut: bool,
}

fn rule(kind: &Kind) -> Option<Rule> {
    match kind {
        Kind::Extrude { .. } => Some(Rule { extrude: true, cut: false }),
        Kind::Revolve { .. } => Some(Rule { extrude: false, cut: true }),
        Kind::Loft { .. } | Kind::Sweep { .. } => Some(Rule { extrude: false, cut: false }),
        Kind::Primitive { cmd, .. } => Some(Rule { extrude: false, cut: *cmd != "solid.coil" }),
        _ => None,
    }
}

fn operation(kind: &mut Kind) -> Option<&mut usize> {
    match kind {
        Kind::Extrude { operation, .. }
        | Kind::Revolve { operation, .. }
        | Kind::Loft { operation, .. }
        | Kind::Sweep { operation, .. }
        | Kind::Primitive { operation, .. } => Some(operation),
        _ => None,
    }
}

/// What a check found, with the Join it evaluated when the material joins.
type Found = (Option<Addition>, Option<solidcraft_engine::Evaluated>);

/// The finished checks and the one running, for the open window.
#[derive(Default)]
pub struct OpCheck {
    /// The running check: its key, commands, the design revision it started on, its result.
    running: Option<(u64, Vec<(String, Value)>, u64, std::sync::mpsc::Receiver<Found>)>,
    /// What stops the running check's evaluation ([`OpCheck::abandon`]).
    progress: Option<std::sync::Arc<solidcraft_engine::doc::progress::Progress>>,
    done: Option<(u64, Option<Addition>)>,
    /// The Join the last check evaluated, with its commands and the design revision: OK of the
    /// same commands takes it rather than computing it again.
    joined: Option<(Vec<(String, Value)>, u64, solidcraft_engine::Evaluated)>,
}

impl OpCheck {
    /// A check is running.
    pub fn busy(&self) -> bool {
        self.running.is_some()
    }

    /// The window it was for closed, or no longer chooses: stop its check (at the evaluation's
    /// next feature) and forget what the checks found, so nothing waits for a window that is gone.
    pub(crate) fn abandon(&mut self) {
        if let Some(progress) = self.progress.take() {
            progress.cancel();
        }
        self.running = None;
        self.done = None;
        self.joined = None;
    }

    /// Take in a finished check.
    pub(crate) fn poll(&mut self) {
        let Some((key, _, _, rx)) = &self.running else { return };
        let (result, evaluated) = match rx.try_recv() {
            Ok(found) => found,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => (None, None),
        };
        self.done = Some((*key, result));
        self.progress = None;
        if let Some((_, cmds, revision, _)) = self.running.take() {
            self.joined = evaluated.map(|e| (cmds, revision, e));
        }
    }

    /// The Join a check evaluated for exactly `cmds` at `revision`, for OK.
    pub(crate) fn joined_for(&self, cmds: &[(String, Value)], revision: u64) -> Option<solidcraft_engine::Evaluated> {
        self.joined.as_ref().filter(|(c, r, _)| c == cmds && *r == revision).map(|(_, _, e)| e.clone())
    }

    /// The finished check of `cmds` (`key`), or None while it runs; one is started when none
    /// runs. A check for older inputs runs to its end first: the latest inputs go next.
    fn result(
        &mut self,
        key: u64,
        ahead: impl FnOnce() -> solidcraft_engine::Ahead,
        cmds: Vec<(String, Value)>,
        revision: u64,
        inside: bool,
    ) -> Option<Option<Addition>> {
        if let Some((k, result)) = self.done
            && k == key
        {
            return Some(result);
        }
        if self.running.is_none() {
            self.start(key, ahead(), cmds, revision, inside);
        }
        self.done.filter(|(k, _)| *k == key).map(|(_, r)| r)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn start(&mut self, key: u64, ahead: solidcraft_engine::Ahead, cmds: Vec<(String, Value)>, revision: u64, inside: bool) {
        let (tx, rx) = std::sync::mpsc::channel();
        let job = cmds.clone();
        let progress = std::sync::Arc::new(solidcraft_engine::doc::progress::Progress::default());
        let watching = progress.clone();
        let spawned = std::thread::Builder::new().name("operation check".into()).spawn(move || {
            let found = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                solidcraft_engine::doc::progress::watch(watching, || ahead.addition(&job, inside))
            }))
            .unwrap_or((None, None));
            let _ = tx.send(found);
        });
        match spawned {
            Ok(_) => {
                self.running = Some((key, cmds, revision, rx));
                self.progress = Some(progress);
            }
            Err(_) => self.done = Some((key, None)),
        }
    }

    /// No threads on the web: check right away.
    #[cfg(target_arch = "wasm32")]
    fn start(&mut self, key: u64, ahead: solidcraft_engine::Ahead, cmds: Vec<(String, Value)>, revision: u64, inside: bool) {
        let (result, evaluated) = ahead.addition(&cmds, inside);
        self.done = Some((key, result));
        self.joined = evaluated.map(|e| (cmds, revision, e));
    }
}

/// The window's commands as the check runs them (New Body, Join or a Cut this window chose all
/// run as a Join), with what tells them from other inputs.
fn check_commands(cmds: &[(String, Value)], revision: u64) -> (Vec<(String, Value)>, u64) {
    let mut out = cmds.to_vec();
    for (_, p) in &mut out {
        if let Some(op) = p.get_mut("operation")
            && op.as_str().is_some_and(|o| matches!(o, "new" | "join" | "cut"))
        {
            *op = Value::from("join");
        }
    }
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(&out).unwrap_or_default().hash(&mut h);
    revision.hash(&mut h);
    (out, h.finish())
}

/// Bring the window's operation in step with its material (once per frame, after Extrude's own
/// choice and before the window is drawn).
pub fn update(app: &mut SolveApp, d: &mut Dialog, ctx: &egui::Context) {
    d.op_auto.pending = false;
    // A check still running is taken in whatever the window does now, so it never looks busy.
    app.op_check.poll();
    let picked = ctx.data_mut(|m| m.remove_temp::<bool>(picked_id())).unwrap_or(false);
    // A window that doesn't choose (or no longer does) has no check running for it.
    let Some(rule) = rule(&d.kind) else {
        app.op_check.abandon();
        return;
    };
    // Turned off in Preferences › Design: the window keeps its operation, as before.
    if d.editing.is_some() || !app.preferences.auto_operation {
        app.op_check.abandon();
        return;
    }
    let Some(op) = operation(&mut d.kind).map(|op| *op) else { return };
    let changed = if rule.extrude { !matches!(d.kind, Kind::Extrude { auto_op: true, .. }) } else { d.op_auto.set.is_some_and(|set| set != op) };
    if picked || changed {
        d.op_auto.manual = true;
        // Extrude's own choice stops too.
        if let Kind::Extrude { auto_op, .. } = &mut d.kind {
            *auto_op = false;
        }
    }
    if d.op_auto.manual {
        return;
    }
    // Extrude cuts into a body by its own rule; this one doesn't undo that.
    if rule.extrude && op == CUT {
        d.op_auto.set = Some(op);
        return;
    }
    // No shown body to join: nothing to check.
    if solidcraft_engine::shown_scope(&app.session).is_some_and(|shown| shown.is_empty()) {
        if let Some(slot) = operation(&mut d.kind) {
            *slot = NEW;
        }
        d.op_auto.set = Some(NEW);
        d.op_auto.held = Some(NEW);
        return;
    }
    let Ok(cmds) = crate::dialogs::apply_commands(app, d) else {
        d.op_auto.set = Some(op);
        return;
    };
    let (cmds, key) = check_commands(&cmds, app.session.revision);
    let session = &app.session;
    let found = app.op_check.result(key, || session.ahead(), cmds, session.revision, rule.cut);
    let chosen = match found {
        Some(Some(Addition::Apart)) => Some(NEW),
        Some(Some(Addition::Joins)) => Some(JOIN),
        Some(Some(Addition::Inside)) if rule.cut => Some(CUT),
        // Extrude goes into material only by its own rule.
        Some(Some(Addition::Inside)) => Some(JOIN),
        // No check to go by (no body, a Join that fails): the operation stays.
        Some(None) => None,
        None => {
            d.op_auto.pending = true;
            // Extrude's own choice comes back every frame: hold the last finished one.
            if rule.extrude { d.op_auto.held } else { None }
        }
    };
    let now = chosen.unwrap_or(op);
    if rule.extrude && found.is_some() {
        d.op_auto.held = chosen;
    }
    if let Some(slot) = operation(&mut d.kind) {
        *slot = now;
    }
    d.op_auto.set = Some(now);
}

/// Where a pick in an Operation row is noted for the next [`update`].
pub(crate) fn picked_id() -> egui::Id {
    egui::Id::new("op_auto_picked")
}

/// The Operation row of a window that chooses by itself: any pick there, the shown option
/// included, is the user's, and the window stops choosing. Returns whether there was one.
pub fn op_row(ui: &mut egui::Ui, id: &str, operation: &mut usize) -> bool {
    let picked = crate::dialogs::form_row(ui, "Operation", |ui| crate::ui_kit::dropdown_picked(ui, id, operation, &crate::dialogs::OP_LABELS));
    if picked {
        ui.ctx().data_mut(|d| d.insert_temp(picked_id(), true));
    }
    picked
}

/// OK in a window still waiting for its check: it applies once the check is in. Returns whether
/// OK applies now (`again`: Apply, which keeps the window open).
pub fn hold_ok(d: &mut Dialog, ok: bool, again: &mut bool, cancel: bool) -> bool {
    if cancel {
        d.op_auto.ok_waiting = None;
        return false;
    }
    if ok && d.op_auto.pending {
        d.op_auto.ok_waiting = Some(*again);
        return false;
    }
    if let Some(waiting) = d.op_auto.ok_waiting
        && !d.op_auto.pending
    {
        d.op_auto.ok_waiting = None;
        *again = waiting;
        return true;
    }
    ok
}

/// The shown bodies as the `targets` of a new feature's Join, Cut or Intersect that names none,
/// when some bodies are hidden: a hidden body is left alone. With none shown, such a feature has
/// nothing to change.
pub fn scope_targets(app: &SolveApp, d: &Dialog, cmds: &mut [(String, Value)]) -> Result<(), String> {
    if rule(&d.kind).is_none() || d.editing.is_some() {
        return Ok(());
    }
    let Some(shown) = solidcraft_engine::shown_scope(&app.session) else { return Ok(()) };
    for (_, p) in cmds {
        let changes = p.get("operation").and_then(Value::as_str).is_some_and(|o| OPS.contains(&o) && o != "new");
        let named = ["targets", "participants"].iter().any(|k| p.get(*k).and_then(Value::as_array).is_some_and(|a| !a.is_empty()));
        if !changes || named {
            continue;
        }
        if shown.is_empty() {
            return Err(crate::i18n::tr("dialog.no_shown_body").to_string());
        }
        if let Some(map) = p.as_object_mut() {
            map.insert("targets".into(), Value::from(shown.clone()));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "op_auto_tests.rs"]
mod tests;
