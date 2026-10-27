//! Commands that recompute the history, computed off the window thread as OK is (#1201): the
//! history marker dragged or clicked, a feature suppressed, reordered or deleted from the
//! timeline, undo and redo, the marker put back after a sketch. On a part with a slow feature
//! after the marker (a line of text cut into a block), each of these ran that feature's boolean
//! on the window thread, and the window froze until it was done.
//!
//! The command is evaluated on a copy of the session ([`solidcraft_engine::Session::ahead`]),
//! which leaves what it computes in the evaluation cache the session shares. Done within the
//! app's wait ([`crate::dialogs::APPLY_AT_ONCE`]), the command just runs; otherwise the window
//! goes on, saying it computes, and the command runs for real once the evaluation is in, taking
//! the features it evaluated ([`solidcraft_engine::Session::prime`]) rather than computing them
//! again. Commands asked for meanwhile wait their turn; a marker moved again
//! replaces the move still waiting, so a drag computes where it ends rather than every slot it
//! crossed.
//!
//! A delete first checks what it takes with it and what it would make fail (#2678): the
//! timeline's Delete asks `timeline.dependents`, Delete on the selection runs `selection.delete`
//! as a dry run. Each check evaluates the history after the deleted feature, so it is computed
//! the same way ([`DeleteCheck`]): the window goes on while it computes, then the delete runs, or
//! the confirmation opens, with the check's results already in the cache.

use serde_json::Value;

use crate::SolveApp;

/// A command computing off the window thread.
pub struct Deferred {
    done: std::sync::mpsc::Receiver<Option<solidcraft_engine::Evaluated>>,
    /// What it evaluated, once in (`Some(None)`: it failed, or its thread is gone).
    evaluated: Option<Option<solidcraft_engine::Evaluated>>,
    /// When it started (ms).
    pub(crate) started: f64,
    command: (String, Value),
    /// The design's revision it was computed for: a design changed meanwhile computes it again.
    revision: u64,
    /// Every result the evaluation ahead computed, whatever the evaluation cache dropped since,
    /// for the run here (a parameter change evaluates twice: the first evaluation is not the
    /// one `evaluated` holds).
    kept: Option<std::sync::Arc<solidcraft_engine::doc::KeptResults>>,
}

impl Deferred {
    pub(crate) fn finished(&mut self) -> bool {
        if self.evaluated.is_none() {
            match self.done.try_recv() {
                Ok(evaluated) => self.evaluated = Some(evaluated),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.evaluated = Some(None),
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        self.evaluated.is_some()
    }
}

/// How a command asked to run off the window thread goes.
enum Start {
    /// It computes off the window thread.
    Later(Deferred),
    /// It runs now, with what was evaluated for it ahead (if anything).
    Now(Option<solidcraft_engine::Evaluated>, Option<std::sync::Arc<solidcraft_engine::doc::KeptResults>>),
}

/// A delete's check computing off the window thread: each check command's reply, in order.
pub struct DeleteCheck {
    done: std::sync::mpsc::Receiver<Vec<Result<Value, String>>>,
    /// When it started (ms).
    pub(crate) started: f64,
    then: AfterCheck,
    /// The design's revision it was computed for: a design changed meanwhile checks again.
    revision: u64,
}

/// What a delete does once its check is in.
#[derive(Clone, Debug)]
pub(crate) enum AfterCheck {
    /// The timeline's Delete of these features (one report for the complete selection).
    Timeline(Vec<u64>),
    /// Delete with these `selection.delete` parameters (the report of its dry run).
    Selection(Value),
}

impl AfterCheck {
    /// Check again, for the design as it is now.
    fn restart(self, app: &mut SolveApp) {
        match self {
            AfterCheck::Timeline(ids) => {
                let ids: Vec<u64> = ids.into_iter().filter(|id| app.session.doc.feature(*id).is_some()).collect();
                if !ids.is_empty() {
                    crate::timeline::delete_features(app, ids);
                }
            }
            AfterCheck::Selection(params) => crate::delete::request(app, params),
        }
    }

    /// Carry on with the check's replies.
    fn finish(self, app: &mut SolveApp, replies: Vec<Result<Value, String>>) {
        match self {
            AfterCheck::Timeline(ids) => match replies.into_iter().collect::<Result<Vec<_>, _>>() {
                Ok(reports) => crate::timeline::delete_checked_features(app, ids, &reports),
                Err(error) => app.set_status(error, true),
            },
            AfterCheck::Selection(params) => {
                let report = replies.into_iter().next().unwrap_or_else(|| Err("the delete was not checked".into()));
                crate::delete::checked(app, params, report);
            }
        }
    }
}

/// Commands a later one of the same kind replaces while they wait: where the marker ends is all
/// that counts.
fn replaced_by_a_later_one(id: &str) -> bool {
    id == "timeline.roll_to"
}

impl SolveApp {
    /// Run a command that may recompute much of the history, off the window thread when it takes
    /// a while (see the module notes). `Ok(Null)` when it is left computing or waiting its turn:
    /// its outcome then shows as any command's does (an error in the status line).
    pub fn run_heavy(&mut self, id: &str, params: Value) -> Result<Value, String> {
        if self.deferred.is_some() {
            if replaced_by_a_later_one(id) && self.deferred_queue.last().is_some_and(|(q, _)| q == id) {
                self.deferred_queue.pop();
            }
            self.deferred_queue.push((id.to_string(), params));
            return Ok(Value::Null);
        }
        match self.begin_deferred(id, &params) {
            Start::Later(job) => {
                self.deferred = Some(job);
                Ok(Value::Null)
            }
            Start::Now(evaluated, kept) => self.run_evaluated(id, params, evaluated.as_ref(), kept),
        }
    }

    /// Run a command for real, taking the features `evaluated` computed for it ahead, and any
    /// other result that evaluation `kept`, so the window thread computes none of them again.
    fn run_evaluated(
        &mut self,
        id: &str,
        params: Value,
        evaluated: Option<&solidcraft_engine::Evaluated>,
        kept: Option<std::sync::Arc<solidcraft_engine::doc::KeptResults>>,
    ) -> Result<Value, String> {
        if let Some(evaluated) = evaluated {
            self.session.prime(evaluated);
        }
        let r = self.run_with_kept(kept, |app| app.run(id, params));
        self.session.unprime();
        r
    }

    /// Since when a command, or a delete's check, has been computing off the window thread (ms),
    /// if one is.
    pub fn computing_since(&self) -> Option<f64> {
        let started = [
            self.deferred.as_ref().map(|d| d.started),
            self.delete_check.as_ref().map(|c| c.started),
            self.pending_feature_edit.as_ref().map(|edit| edit.started),
        ];
        started.into_iter().flatten().reduce(f64::min)
    }

    /// Run a delete's check `commands` and carry on with `then` once their replies are in: off
    /// the window thread when it takes longer than the app's wait (see the module notes),
    /// otherwise right away. A Delete while another delete is being checked is left out.
    pub(crate) fn check_delete(&mut self, commands: Vec<(String, Value)>, then: AfterCheck) {
        if self.delete_check.is_some() {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(wait) = self.apply_off_thread
            && !self.file_load.busy()
            && self.file_picker.is_none()
        {
            let ahead = self.session.ahead();
            let (tx, rx) = std::sync::mpsc::channel();
            let checks = commands.clone();
            let spawned = std::thread::Builder::new().name("delete check".into()).spawn(move || {
                let replies = ahead.replies(&checks).into_iter().map(|r| r.map_err(|e| e.to_string())).collect();
                let _ = tx.send(replies);
            });
            if spawned.is_ok() {
                let check = DeleteCheck { done: rx, started: crate::now_ms(), then, revision: self.session.revision };
                // As with the history's commands, zero always leaves it for a later frame.
                match (wait.is_zero(), check.done.recv_timeout(wait)) {
                    (false, Ok(replies)) => check.then.finish(self, replies),
                    _ => self.delete_check = Some(check),
                }
                return;
            }
        }
        let replies = commands.iter().map(|(id, params)| self.session.execute(id, params).map_err(|e| e.to_string())).collect();
        then.finish(self, replies);
    }

    /// Each frame: a delete whose check is in carries on (checked again when the design changed
    /// meanwhile).
    pub(crate) fn poll_delete_check(&mut self, ctx: &egui::Context) {
        let Some(check) = &self.delete_check else { return };
        let replies = match check.done.try_recv() {
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
                return;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => None,
            Ok(replies) => Some(replies),
        };
        let Some(check) = self.delete_check.take() else { return };
        match replies {
            None => self.set_status("the delete could not be checked", true),
            Some(_) if check.revision != self.session.revision => check.then.restart(self),
            Some(replies) => check.then.finish(self, replies),
        }
    }

    /// Each frame: a command whose evaluation is in runs, then the next one waiting starts.
    pub(crate) fn poll_deferred(&mut self, ctx: &egui::Context) {
        if let Some(job) = &mut self.deferred {
            if !job.finished() {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
                return;
            }
            if let Some(job) = self.deferred.take() {
                let (id, params) = job.command;
                let superseded = replaced_by_a_later_one(&id) && self.deferred_queue.first().is_some_and(|(q, _)| *q == id);
                if job.revision != self.session.revision {
                    // The design changed meanwhile: computed again for the design as it is now.
                    let _ = self.run_heavy(&id, params);
                } else if !superseded {
                    let _ = self.run_evaluated(&id, params, job.evaluated.flatten().as_ref(), job.kept);
                }
            }
        }
        if self.deferred.is_none() && !self.deferred_queue.is_empty() {
            let (id, params) = self.deferred_queue.remove(0);
            let _ = self.run_heavy(&id, params);
        }
        if self.deferred.is_none()
            && self.deferred_queue.is_empty()
            && let Some((id, sketch)) = self.start_after_roll.take()
            && self.dialog.is_none()
            && self.tool.is_none()
        {
            self.start_interactive(&id, Some(sketch));
        }
    }

    /// Start evaluating a command ahead. `None` when it should just run: not asked for
    /// ([`SolveApp::apply_off_thread`]), a file operation in progress, no threads (the web), or
    /// done within the wait.
    #[cfg(not(target_arch = "wasm32"))]
    fn begin_deferred(&self, id: &str, params: &Value) -> Start {
        let Some(wait) = self.apply_off_thread else { return Start::Now(None, None) };
        if self.file_load.busy() || self.file_picker.is_some() {
            return Start::Now(None, None);
        }
        let ahead = self.session.ahead();
        let kept = ahead.kept();
        let command = (id.to_string(), params.clone());
        let commands = vec![command.clone()];
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new().name("history".into()).spawn(move || {
            let _ = tx.send(ahead.evaluate(&commands).ok());
        });
        if spawned.is_err() {
            return Start::Now(None, None);
        }
        // Zero explicitly requests off-thread application, even if a fast worker
        // finishes before this thread is scheduled again. Apply on the next frame.
        if !wait.is_zero() {
            match rx.recv_timeout(wait) {
                Ok(evaluated) => return Start::Now(evaluated, Some(kept)),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Start::Now(None, Some(kept)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
        Start::Later(Deferred { done: rx, evaluated: None, started: crate::now_ms(), command, revision: self.session.revision, kept: Some(kept) })
    }

    #[cfg(target_arch = "wasm32")]
    fn begin_deferred(&self, _id: &str, _params: &Value) -> Start {
        Start::Now(None, None)
    }
}
