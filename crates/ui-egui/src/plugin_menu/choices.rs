//! A plugin entry can supply a ranked list for one of its own fields. Each open dialog
//! asks on a scratch design and keeps the answer only for those inputs on that revision.

use std::collections::BTreeSet;
use std::sync::{Arc, mpsc};

use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_engine::doc::progress::{self, Progress};

/// The exact inputs to discovery. The choice the person made is replaced by the query token,
/// so choosing another option neither restarts discovery nor changes the order of its results.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Request {
    session: u64,
    revision: u64,
    plugin: String,
    query: String,
    run: Value,
}

impl Request {
    pub(super) fn new(
        session: u64,
        revision: u64,
        plugin: &super::Listed,
        entry: &str,
        mut base_params: Value,
        field: &str,
        query: &str,
    ) -> Option<Self> {
        base_params.as_object_mut()?.insert(field.to_owned(), Value::String(query.to_owned()));
        let run = json!({"path":plugin.path, "approval":plugin.approval, "entry":entry, "params":base_params});
        Some(Self { session, revision, plugin: plugin.id.clone(), query: query.to_owned(), run })
    }

    fn same_inputs(&self, other: &Self) -> bool {
        self.session == other.session && self.plugin == other.plugin && self.query == other.query && self.run == other.run
    }
}

#[derive(Debug, PartialEq)]
pub(super) struct Choice {
    pub(super) value: String,
    pub(super) label: String,
    pub(super) summary: Option<String>,
}

type Reply = Result<Vec<Choice>, String>;

pub(super) enum View<'a> {
    Inactive,
    Waiting,
    Failed(&'a str),
    Ready { options: &'a [Choice], selected: usize },
}

enum Status {
    Waiting,
    Ready { options: Vec<Choice>, selected: usize },
    Failed(String),
}

struct Current {
    request: Request,
    status: Status,
    /// A selection made by the person. The plug-in's default can follow its ranking on each revision.
    preferred: Option<String>,
}

struct Worker {
    reply: mpsc::Receiver<Reply>,
    progress: Arc<Progress>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.progress.cancel();
    }
}

#[derive(Default)]
pub(super) struct State {
    current: Option<Current>,
    /// A cancelled worker keeps its slot until it finishes. Rapid changes replace only
    /// `current`; no scratch copies or queued jobs accumulate behind the worker.
    worker: Option<Worker>,
}

impl State {
    /// `before` and `after` were built around a verified geometric rebind of the same picks.
    /// Only carry the explicit preference. Even on those same faces a new query must establish
    /// which options fit, along with their current dimensions and ranking.
    pub(super) fn rebind(&mut self, before: &Request, after: &Request) {
        let Some(current) = &mut self.current else { return };
        if current.request != *before || before.revision == after.revision || current.preferred.is_none() {
            return;
        }
        if let Some(worker) = &self.worker {
            worker.progress.cancel();
        }
        current.request = after.clone();
        current.status = Status::Waiting;
    }

    pub(super) fn forget_preference(&mut self) {
        if let Some(current) = &mut self.current {
            current.preferred = None;
        }
    }

    pub(super) fn sync(&mut self, request: Option<&Request>, scratch: impl FnOnce() -> Session) {
        self.sync_with(request, |request| start(request, scratch()));
    }

    fn sync_with(&mut self, request: Option<&Request>, start: impl FnOnce(&Request) -> Result<Worker, String>) {
        if self.current.as_ref().map(|current| &current.request) != request {
            if let Some(worker) = &self.worker {
                worker.progress.cancel();
            }
            self.current = request.map(|request| {
                let preferred = self.current.take().filter(|previous| previous.request.same_inputs(request)).and_then(|previous| previous.preferred);
                Current { request: request.clone(), status: Status::Waiting, preferred }
            });
        }
        if let Some(worker) = &self.worker {
            match worker.reply.try_recv() {
                Ok(reply) => self.finish(Some(reply)),
                Err(mpsc::TryRecvError::Disconnected) => self.finish(None),
                Err(mpsc::TryRecvError::Empty) => return,
            }
        }
        if let Some(current) = &mut self.current
            && matches!(current.status, Status::Waiting)
        {
            match start(&current.request) {
                Ok(worker) => self.worker = Some(worker),
                Err(error) => current.status = Status::Failed(error),
            }
        }
    }

    fn finish(&mut self, reply: Option<Reply>) {
        let Some(worker) = self.worker.take() else { return };
        if worker.progress.is_cancelled() {
            return;
        }
        if let Some(current) = &mut self.current {
            current.status = match reply {
                Some(Ok(options)) => {
                    let selected = current.preferred.as_ref().and_then(|value| options.iter().position(|choice| &choice.value == value));
                    if selected.is_none() {
                        current.preferred = None;
                    }
                    Status::Ready { options, selected: selected.unwrap_or(0) }
                }
                Some(Err(error)) => Status::Failed(error),
                None => Status::Failed(solidcraft_ui_text::tr("ui.plugin_no_result").into()),
            };
        }
    }

    /// Drain the worker deterministically for tests without running another UI frame.
    #[cfg(test)]
    pub(super) fn wait(&mut self) {
        if let Some(worker) = &self.worker {
            self.finish(worker.reply.recv().ok());
        }
    }

    pub(super) fn view(&self) -> View<'_> {
        match self.current.as_ref().map(|current| &current.status) {
            None => View::Inactive,
            Some(Status::Waiting) => View::Waiting,
            Some(Status::Failed(error)) => View::Failed(error),
            Some(Status::Ready { options, selected }) => View::Ready { options, selected: *selected },
        }
    }

    pub(super) fn choose(&mut self, index: usize) {
        if let Some(Current { status: Status::Ready { options, selected }, preferred, .. }) = &mut self.current
            && let Some(choice) = options.get(index)
        {
            *selected = index;
            *preferred = Some(choice.value.clone());
        }
    }

    /// Apply the submitted choice only when the refreshed list contains that exact value.
    pub(super) fn select_value(&mut self, value: &str) -> bool {
        let View::Ready { options, .. } = self.view() else { return false };
        let Some(index) = options.iter().position(|choice| choice.value == value) else { return false };
        self.choose(index);
        true
    }

    /// Run and Preview also check the request at the point of use: an edit can precede the
    /// next frame that syncs this dialog.
    fn selected_choice(&self, request: &Request) -> Option<&Choice> {
        let current = self.current.as_ref().filter(|current| &current.request == request)?;
        let Status::Ready { options, selected } = &current.status else { return None };
        options.get(*selected)
    }

    pub(super) fn selected(&self, request: &Request) -> Option<&str> {
        self.selected_choice(request).map(|choice| choice.value.as_str())
    }

    pub(super) fn selected_label(&self, request: &Request) -> Option<&str> {
        self.selected_choice(request).map(|choice| choice.label.as_str())
    }
}

fn start(request: &Request, mut scratch: Session) -> Result<Worker, String> {
    super::dry_run(&mut scratch, &request.plugin);
    let run = request.run.clone();
    let query = request.query.clone();
    let invalid = solidcraft_ui_text::tr("ui.plugin_dialog_invalid_choices").to_owned();
    let progress = Arc::new(Progress::default());
    let watched = progress.clone();
    let (send, reply) = mpsc::channel();
    std::thread::Builder::new()
        .name("plugin-choices".into())
        .spawn(move || {
            let stop = scratch.stop_token();
            if watched.is_cancelled() || stop.is_stopped() {
                return;
            }
            let result = stop.within(|| progress::watch(watched, || scratch.execute("plugin.run", &run)));
            let answer = result.map_err(|error| error.to_string()).and_then(|result| parse_choices(&result, &query).ok_or(invalid));
            // Release the scratch design before opening the slot for a replacement.
            drop(scratch);
            let _ = send.send(answer);
        })
        .map_err(|error| error.to_string())?;
    Ok(Worker { reply, progress })
}

/// The engine wraps the entry's output in `result`. Only accept a complete list, in guest
/// order: silently skipping a bad first option would present another fit as the best one.
fn parse_choices(reply: &Value, query: &str) -> Option<Vec<Choice>> {
    const MAX_OPTIONS: usize = 64;
    let rows = reply.pointer("/result/options")?.as_array()?;
    if rows.is_empty() || rows.len() > MAX_OPTIONS {
        return None;
    }
    let mut seen = BTreeSet::new();
    let mut options = Vec::with_capacity(rows.len());
    for row in rows {
        let value = bounded_text(row.get("value")?, 256)?;
        if value == query || !seen.insert(value) {
            return None;
        }
        let label = bounded_text(row.get("label")?, 256)?.to_owned();
        let summary = match row.get("summary") {
            None => None,
            Some(summary) => Some(bounded_text(summary, 1024)?.to_owned()),
        };
        options.push(Choice { value: value.to_owned(), label, summary });
    }
    Some(options)
}

fn bounded_text(value: &Value, max_bytes: usize) -> Option<&str> {
    value.as_str().filter(|text| text.len() <= max_bytes && !text.trim().is_empty() && !text.chars().any(char::is_control))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin() -> super::super::Listed {
        super::super::Listed {
            id: "gears".into(),
            path: "gears".into(),
            approval: "approved".into(),
            name: "Gears".into(),
            version: "1".into(),
            description: String::new(),
            disabled: false,
            requests_folders: false,
            groups: Default::default(),
            entries: vec![],
            images: Default::default(),
        }
    }

    fn request(revision: u64) -> Request {
        Request::new(1, revision, &plugin(), "make", json!({"teeth":24}), "fit", "list").unwrap()
    }

    fn worker() -> (Worker, mpsc::Sender<Reply>, Arc<Progress>) {
        let (send, reply) = mpsc::channel();
        let progress = Arc::new(Progress::default());
        (Worker { reply, progress: progress.clone() }, send, progress)
    }

    fn options() -> Vec<Choice> {
        parse_choices(
            &json!({"result":{"options":[{"value":"fine","label":"Fine","summary":"Small teeth"},{"value":"coarse","label":"Coarse"}]}}),
            "list",
        )
        .unwrap()
    }

    fn no_start(_: &Request) -> Result<Worker, String> {
        panic!("unexpected choice query")
    }

    #[test]
    fn query_identifies_the_design_package_entry_and_inputs_but_ignores_the_selected_fit() {
        fn build(tab: u64, entry: &str, plugin: &super::super::Listed, params: Value) -> Request {
            Request::new(tab, 2, plugin, entry, params, "fit", "list").unwrap()
        }
        let base = request(2);
        let mut p = plugin();
        let choices = json!({"teeth":24, "fit":"coarse"});
        assert_eq!(base, build(1, "make", &p, choices.clone()));
        assert_eq!(base.run, json!({"path":"gears","approval":"approved","entry":"make","params":{"teeth":24,"fit":"list"}}));
        assert_ne!(base, build(2, "make", &p, choices.clone()));
        assert_ne!(base, build(1, "make", &p, json!({"teeth":25})));
        assert_ne!(base, build(1, "other", &p, choices.clone()));
        assert_ne!(base, request(3));
        p.approval = "updated".into();
        assert_ne!(base, build(1, "make", &p, choices));
    }

    #[test]
    fn options_require_an_ordered_nonempty_bounded_list_with_unique_runnable_values() {
        let good = options();
        assert_eq!(good[0].value, "fine");
        assert_eq!(good[0].summary.as_deref(), Some("Small teeth"));
        assert!(good[1].summary.is_none());
        let parse = |rows| parse_choices(&json!({"result":{"options": rows}}), "list");
        let row = json!({"value":"fine","label":"Fine"});
        assert!(parse_choices(&json!({"options":[row.clone()]}), "list").is_none());
        for bad in [
            json!([]),
            json!("none"),
            json!([row.clone(), row.clone()]),
            json!([{"value":"list","label":"Discover again"}]),
            json!([{"value":"x"}]),
            json!([{"value":" ","label":"X"}]),
            json!([{"value":"x","label":"h\ni"}]),
            json!([{"value":"x","label":"X","summary":1}]),
            json!([{"value":"x".repeat(257),"label":"X"}]),
            json!([{"value":"x","label":"X","summary":"x".repeat(1025)}]),
            json!((0..65).map(|n| json!({"value":n.to_string(),"label":"X"})).collect::<Vec<_>>()),
        ] {
            assert!(parse(bad.clone()).is_none(), "{bad}");
        }
    }

    #[test]
    fn a_refresh_preserves_the_explicit_value_and_reads_its_new_label_and_rank() {
        let (a, b) = (request(2), request(3));
        let mut state = State::default();
        let (job, send, _) = worker();
        state.sync_with(Some(&a), |_| Ok(job));
        assert!(matches!(state.view(), View::Waiting));
        state.sync_with(Some(&a), no_start);
        send.send(Ok(options())).unwrap();
        state.wait();
        assert_eq!(state.selected(&a), Some("fine"));
        state.choose(1);
        state.choose(99);
        state.sync_with(Some(&a), no_start);
        assert_eq!(state.selected(&a), Some("coarse"));
        assert_eq!(state.selected(&b), None, "an edit before sync invalidates the old choice for Run");

        let (job, send, _) = worker();
        state.sync_with(Some(&b), |_| Ok(job));
        assert_eq!(state.selected(&a), None);
        assert_eq!(state.selected(&b), None);
        assert_eq!(state.selected_label(&a), None);
        assert!(!state.select_value("coarse"), "the old result cannot authorize a run while refreshing");
        let mut refreshed = options();
        refreshed.insert(0, Choice { value: "medium".into(), label: "Medium".into(), summary: None });
        refreshed[2].label = "Updated coarse".into();
        send.send(Ok(refreshed)).unwrap();
        state.sync_with(Some(&b), no_start);
        assert_eq!(state.selected(&b), Some("coarse"));
        assert_eq!(state.selected_label(&b), Some("Updated coarse"));
        assert!(matches!(state.view(), View::Ready { selected: 2, .. }));
        assert!(!state.select_value("removed"));
        assert_eq!(state.selected(&b), Some("coarse"));
        assert!(state.select_value("fine"));
        assert_eq!(state.selected(&b), Some("fine"));
        state.sync_with(None, no_start);
        assert!(matches!(state.view(), View::Inactive));
    }

    #[test]
    fn a_verified_face_rebind_preserves_only_the_explicit_preference() {
        let geometry = |revision, index, x, style| {
            Request::new(
                1,
                revision,
                &plugin(),
                "make",
                json!({"style": style, "faces":[{"type":"face","body":"Blank",
                "index":index,"point":[x,0,6]}]}),
                "fit",
                "list",
            )
            .unwrap()
        };
        let before = geometry(2, 1, 5.0, "spur");
        let after = geometry(3, 2, 6.0, "spur");
        let mut state = State::default();
        let (job, send, _) = worker();
        state.sync_with(Some(&before), |_| Ok(job));
        send.send(Ok(options())).unwrap();
        state.wait();
        state.choose(1);
        // An edited setting or explicit repick differs from the request that owned the choice.
        for other in [geometry(2, 1, 5.0, "helical"), geometry(2, 0, 4.0, "spur")] {
            state.rebind(&other, &after);
            assert_eq!(state.selected(&before), Some("coarse"));
            assert_eq!(state.selected(&after), None);
        }
        state.rebind(&before, &after);
        assert!(matches!(state.view(), View::Waiting));
        assert_eq!(state.selected(&before), None);
        assert_eq!(state.selected(&after), None);
        let (job, send, _) = worker();
        state.sync_with(Some(&after), |_| Ok(job));
        send.send(Ok(options())).unwrap();
        state.wait();
        assert_eq!(state.selected(&after), Some("coarse"));
    }

    #[test]
    fn absent_choices_and_changed_inputs_use_the_fresh_default() {
        fn complete(state: &mut State, request: &Request, items: Vec<Choice>) {
            let (job, send, _) = worker();
            state.sync_with(Some(request), |_| Ok(job));
            send.send(Ok(items)).unwrap();
            state.wait();
        }
        let mut state = State::default();
        complete(&mut state, &request(1), options());
        state.choose(1);
        let mut fewer = options();
        fewer.truncate(1);
        complete(&mut state, &request(2), fewer);
        assert!(!state.select_value("coarse"));
        assert_eq!(state.selected(&request(2)), Some("fine"));

        complete(&mut state, &request(3), options());
        assert_eq!(state.selected(&request(3)), Some("fine"), "a previously missing choice does not silently reappear as the selection");
        state.choose(1);
        let new_inputs = Request::new(1, 3, &plugin(), "make", json!({"teeth":25}), "fit", "list").unwrap();
        complete(&mut state, &new_inputs, options());
        assert_eq!(state.selected(&new_inputs), Some("fine"), "new inputs get their own recommendation");
        let mut reordered = options();
        reordered.reverse();
        let next = Request { revision: 4, ..new_inputs };
        complete(&mut state, &next, reordered);
        assert_eq!(state.selected(&next), Some("coarse"), "with no explicit choice the new best fit wins");
    }

    #[test]
    fn rapid_edits_cancel_one_worker_and_coalesce_even_if_the_original_inputs_return() {
        let (a, b) = (request(2), request(3));
        let mut state = State::default();
        let (old, reply, cancelled) = worker();
        state.sync_with(Some(&a), |_| Ok(old));
        state.sync_with(Some(&b), no_start);
        assert!(cancelled.is_cancelled());
        state.sync_with(None, no_start);
        state.sync_with(Some(&a), no_start);
        reply.send(Ok(options())).unwrap();
        let (latest, _send, cancelled) = worker();
        state.sync_with(Some(&a), |key| {
            assert_eq!(key, &a);
            Ok(latest)
        });
        assert!(matches!(state.view(), View::Waiting), "the cancelled reply was for A but cannot become current");
        assert_eq!(state.selected(&a), None);
        drop(state);
        assert!(cancelled.is_cancelled(), "closing the dialog cancels its query");
    }

    #[test]
    fn worker_errors_stay_with_their_inputs_and_a_new_request_recovers() {
        let (a, b) = (request(2), request(3));
        let mut state = State::default();
        let (job, send, _) = worker();
        state.sync_with(Some(&a), |_| Ok(job));
        send.send(Err("incompatible picks".into())).unwrap();
        state.sync_with(Some(&a), no_start);
        assert!(matches!(state.view(), View::Failed("incompatible picks")));
        state.sync_with(Some(&a), no_start);
        let (job, send, _) = worker();
        state.sync_with(Some(&b), |_| Ok(job));
        send.send(Ok(options())).unwrap();
        state.wait();
        assert_eq!(state.selected(&b), Some("fine"));
    }
}
