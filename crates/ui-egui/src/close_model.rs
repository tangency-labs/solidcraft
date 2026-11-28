//! The rules for one action that closes a set of documents.
//!
//! This has no UI or I/O: callers answer the requests and draw the snapshot.
//! Based on the No File Left Behind 0.4.0 reference model (0BSD).

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Document {
    pub id: String,
    pub name: String,
    pub path: Option<String>,
    pub dirty: bool,
    #[serde(default)]
    pub read_only: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RowState {
    Unsaved,
    Saving,
    Saved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Prompt {
    List,
    Single,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Outcome {
    Proceeded,
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum NoteKind {
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Note {
    pub kind: NoteKind,
    pub text: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PickWhy {
    NeverSaved,
    ReadOnly,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub(crate) enum Request {
    Pick { id: String, why: PickWhy },
    Save { id: String, path: String },
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Row {
    pub id: String,
    pub name: String,
    #[serde(rename = "where")]
    pub where_: Option<String>,
    pub read_only: bool,
    pub ticked: bool,
    pub state: RowState,
    pub note: Option<Note>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Snapshot {
    pub prompt: Prompt,
    pub count: usize,
    pub rows: Vec<Row>,
    pub request: Option<Request>,
    pub outcome: Option<Outcome>,
    pub open: Vec<String>,
    pub closed: Vec<String>,
    pub saved: BTreeMap<String, String>,
    pub lost: Vec<String>,
    pub abandoned: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Clean,
    Unsaved,
    Saved,
}

#[derive(Clone, Debug)]
struct Entry {
    document: Document,
    never_saved: bool,
    state: State,
    open: bool,
    ticked: bool,
    note: Option<Note>,
    saved_to: Option<String>,
    lost: bool,
}

#[derive(Clone, Debug)]
struct Run {
    queue: VecDeque<String>,
    target: Option<String>,
    failed: bool,
    changed: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Closing {
    docs: Vec<Entry>,
    request: Option<Request>,
    run: Option<Run>,
    outcome: Option<Outcome>,
    abandoned: Vec<String>,
    single: bool,
}

impl Closing {
    pub fn new(docs: Vec<Document>) -> Self {
        let docs: Vec<_> = docs
            .into_iter()
            .map(|document| {
                let state = if document.dirty { State::Unsaved } else { State::Clean };
                Entry {
                    never_saved: document.path.is_none(),
                    ticked: document.dirty,
                    document,
                    state,
                    open: true,
                    note: None,
                    saved_to: None,
                    lost: false,
                }
            })
            .collect();
        let count = docs.iter().filter(|d| d.state == State::Unsaved).count();
        let mut closing = Self { docs, request: None, run: None, outcome: None, abandoned: Vec::new(), single: count == 1 };
        if count == 0 {
            closing.proceed();
        }
        closing
    }

    pub fn snapshot(&self) -> Snapshot {
        let mut names = BTreeMap::<String, usize>::new();
        let mut used_names = BTreeSet::new();
        let rows: Vec<_> = self
            .docs
            .iter()
            .filter(|d| self.outcome.is_none() && d.open && d.state != State::Clean)
            .map(|d| {
                let mut name = d.document.name.clone();
                if d.never_saved {
                    let base = name.clone();
                    let count = names.entry(base.clone()).or_default();
                    *count += 1;
                    if *count > 1 {
                        name = format!("{base} {count}");
                    }
                    while used_names.contains(&name) {
                        *count += 1;
                        name = format!("{base} {count}");
                    }
                    used_names.insert(name.clone());
                }
                let saving = matches!(&self.request, Some(Request::Save { id, .. }) if *id == d.document.id);
                Row {
                    id: d.document.id.clone(),
                    name,
                    where_: d.document.path.clone(),
                    read_only: d.document.read_only,
                    ticked: d.state == State::Unsaved && d.ticked,
                    state: if saving {
                        RowState::Saving
                    } else if d.state == State::Saved {
                        RowState::Saved
                    } else {
                        RowState::Unsaved
                    },
                    note: d.note.clone(),
                }
            })
            .collect();
        Snapshot {
            prompt: if self.outcome.is_some() {
                Prompt::None
            } else if self.single && rows.len() == 1 {
                Prompt::Single
            } else {
                Prompt::List
            },
            count: if self.outcome.is_some() { 0 } else { self.unsaved().count() },
            rows,
            request: self.request.clone(),
            outcome: self.outcome,
            open: self.docs.iter().filter(|d| d.open).map(|d| d.document.id.clone()).collect(),
            closed: self.docs.iter().filter(|d| !d.open).map(|d| d.document.id.clone()).collect(),
            saved: self.docs.iter().filter_map(|d| d.saved_to.as_ref().map(|path| (d.document.id.clone(), path.clone()))).collect(),
            lost: self.docs.iter().filter(|d| d.lost).map(|d| d.document.id.clone()).collect(),
            abandoned: self.abandoned.clone(),
        }
    }

    fn unsaved(&self) -> impl Iterator<Item = &Entry> {
        self.docs.iter().filter(|d| d.open && d.state == State::Unsaved)
    }

    fn doc_mut(&mut self, id: &str) -> Option<&mut Entry> {
        self.docs.iter_mut().find(|d| d.document.id == id)
    }

    fn idle(&self) -> bool {
        self.outcome.is_none() && self.request.is_none()
    }

    fn answerable(&self, id: &str) -> bool {
        self.idle() && self.unsaved().any(|d| d.document.id == id)
    }

    pub fn tick(&mut self, id: &str, on: bool) -> bool {
        if !self.answerable(id) || self.snapshot().prompt == Prompt::Single {
            return false;
        }
        if let Some(d) = self.doc_mut(id) {
            d.ticked = on;
        }
        true
    }

    pub fn save_selected(&mut self) -> bool {
        if !self.idle() {
            return false;
        }
        let queue = self.unsaved().filter(|d| d.ticked).map(|d| d.document.id.clone()).collect();
        self.start(queue, None);
        true
    }

    pub fn dont_save(&mut self) -> bool {
        if !self.idle() {
            return false;
        }
        self.proceed();
        true
    }

    pub fn cancel(&mut self) -> bool {
        if self.outcome.is_some() || matches!(self.request, Some(Request::Pick { .. })) {
            return false;
        }
        if let Some(Request::Save { id, .. }) = self.request.take() {
            self.abandoned.push(id);
        }
        self.run = None;
        self.outcome = Some(Outcome::Stopped);
        true
    }

    pub fn row_save(&mut self, id: &str) -> bool {
        if !self.answerable(id) {
            return false;
        }
        self.start(VecDeque::from([id.to_owned()]), Some(id.to_owned()));
        true
    }

    pub fn row_discard(&mut self, id: &str) -> bool {
        if !self.answerable(id) {
            return false;
        }
        if let Some(d) = self.doc_mut(id) {
            d.open = false;
            d.lost = true;
        }
        self.last_row_gone();
        true
    }

    /// Refresh an original-path save from live document metadata before starting its write.
    pub fn refresh_save_location(&mut self, path: Option<String>, read_only: bool) -> bool {
        let Some(Request::Save { id, path: requested }) = self.request.clone() else {
            return false;
        };
        let Some(d) = self.doc_mut(&id) else {
            return false;
        };
        // A chosen picker target already has the user's consent; do not ask for it again.
        if d.document.path.as_deref() != Some(requested.as_str()) || d.document.read_only {
            return false;
        }
        let changed = d.document.path != path || d.document.read_only != read_only;
        d.document.path.clone_from(&path);
        d.document.read_only = read_only;
        let request = match path {
            None => Request::Pick { id, why: PickWhy::NeverSaved },
            Some(_) if read_only => Request::Pick { id, why: PickWhy::ReadOnly },
            Some(path) => Request::Save { id, path },
        };
        let changed = changed || self.request.as_ref() != Some(&request);
        self.request = Some(request);
        changed
    }

    /// A host-side conflict question can ask for a copy instead of replacing the file.
    pub fn request_location(&mut self) -> bool {
        let Some(Request::Save { id, .. }) = &self.request else {
            return false;
        };
        self.request = Some(Request::Pick { id: id.clone(), why: PickWhy::ReadOnly });
        true
    }

    pub fn picked(&mut self, path: String) -> bool {
        let Some(Request::Pick { id, .. }) = &self.request else {
            return false;
        };
        self.request = Some(Request::Save { id: id.clone(), path });
        true
    }

    pub fn pick_cancelled(&mut self) -> bool {
        let Some(Request::Pick { id, .. }) = self.request.clone() else {
            return false;
        };
        if let Some(d) = self.doc_mut(&id) {
            d.note = Some(Note { kind: NoteKind::Cancelled, text: None });
        }
        if let Some(run) = &mut self.run {
            run.failed = true;
        }
        self.request = None;
        self.finish_run();
        true
    }

    /// A host-side Save Copy or equivalent may redirect the write to another path.
    pub fn save_done_at(&mut self, path: String) -> bool {
        let Some(Request::Save { path: requested, .. }) = &mut self.request else {
            return false;
        };
        *requested = path;
        self.save_done()
    }

    pub fn save_done(&mut self) -> bool {
        let Some(Request::Save { id, path }) = self.request.clone() else {
            return false;
        };
        let changed = self.run.as_ref().is_some_and(|run| run.changed);
        if let Some(d) = self.doc_mut(&id) {
            d.document.path = Some(path.clone());
            d.document.read_only = false;
            d.saved_to = Some(path);
            d.note = None;
            d.state = if changed { State::Unsaved } else { State::Saved };
        }
        if let Some(run) = &mut self.run {
            run.changed = false;
            run.queue.pop_front();
        }
        self.request = None;
        self.next();
        true
    }

    pub fn save_failed(&mut self, reason: String) -> bool {
        let Some(Request::Save { id, .. }) = self.request.clone() else {
            return false;
        };
        if let Some(d) = self.doc_mut(&id) {
            d.note = Some(Note { kind: NoteKind::Failed, text: Some(reason) });
        }
        if let Some(run) = &mut self.run {
            run.failed = true;
            run.changed = false;
            run.queue.pop_front();
        }
        self.request = None;
        self.next();
        true
    }

    pub fn doc_changed(&mut self, id: &str) -> bool {
        if self.outcome.is_some() || !self.docs.iter().any(|d| d.open && d.document.id == id) {
            return false;
        }
        // A change during Save Selected needs fresh consent, even if the row was unticked.
        if self.run.as_ref().is_some_and(|run| run.target.is_none())
            && let Some(d) = self.doc_mut(id)
        {
            d.ticked = true;
        }
        if matches!(&self.request, Some(Request::Save { id: saving_id, .. }) if saving_id == id) {
            if let Some(run) = &mut self.run {
                run.changed = true;
            }
            return true;
        }
        if let Some(d) = self.doc_mut(id) {
            if d.state == State::Clean {
                d.ticked = true;
            }
            d.state = State::Unsaved;
            d.saved_to = None;
        }
        true
    }

    fn start(&mut self, queue: VecDeque<String>, target: Option<String>) {
        for id in &queue {
            if let Some(d) = self.doc_mut(id) {
                d.note = None;
            }
        }
        self.run = Some(Run { queue, target, failed: false, changed: false });
        self.next();
    }

    fn next(&mut self) {
        let id = self.run.as_ref().and_then(|run| run.queue.front());
        let Some(id) = id else {
            self.finish_run();
            return;
        };
        let Some(d) = self.docs.iter().find(|d| d.document.id == *id) else {
            return;
        };
        self.request = Some(match &d.document.path {
            None => Request::Pick { id: id.clone(), why: PickWhy::NeverSaved },
            Some(_) if d.document.read_only => Request::Pick { id: id.clone(), why: PickWhy::ReadOnly },
            Some(path) => Request::Save { id: id.clone(), path: path.clone() },
        });
    }

    fn finish_run(&mut self) {
        let Some(run) = self.run.take() else {
            return;
        };
        if let Some(target) = run.target {
            if let Some(d) = self.doc_mut(&target)
                && d.state == State::Saved
            {
                d.open = false;
            }
            self.last_row_gone();
        } else if !run.failed && !self.unsaved().any(|d| d.ticked) {
            self.proceed();
        }
    }

    fn last_row_gone(&mut self) {
        if self.unsaved().next().is_none() {
            self.proceed();
        }
    }

    fn proceed(&mut self) {
        for d in &mut self.docs {
            if d.open && d.state == State::Unsaved {
                d.lost = true;
            }
            d.open = false;
        }
        self.request = None;
        self.run = None;
        self.outcome = Some(Outcome::Proceeded);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn difference(got: &Value, want: &Value, at: &str) -> Option<String> {
        match want {
            Value::Array(expected) => {
                let Some(actual) = got.as_array().filter(|actual| actual.len() == expected.len()) else {
                    return Some(format!("{at}: wanted {want}, got {got}"));
                };
                actual.iter().zip(expected).enumerate().find_map(|(index, (got, want))| difference(got, want, &format!("{at}[{index}]")))
            }
            Value::Object(expected) => {
                if !got.is_object() {
                    return Some(format!("{at}: wanted {want}, got {got}"));
                }
                expected.iter().find_map(|(key, want)| difference(&got[key], want, &format!("{at}.{key}")))
            }
            _ if got != want => Some(format!("{at}: wanted {want}, got {got}")),
            _ => None,
        }
    }

    #[test]
    fn never_saved_names_follow_visible_row_order() {
        let docs = (0..4)
            .map(|index| Document { id: index.to_string(), name: "Untitled".to_owned(), path: None, dirty: index % 2 == 0, read_only: false })
            .collect();
        let mut model = Closing::new(docs);
        let snapshot = model.snapshot();
        let names: Vec<_> = snapshot.rows.iter().map(|row| (row.id.as_str(), row.name.as_str())).collect();
        assert_eq!(names, [("0", "Untitled"), ("2", "Untitled 2")]);

        assert!(model.doc_changed("1"));
        let snapshot = model.snapshot();
        let names: Vec<_> = snapshot.rows.iter().map(|row| (row.id.as_str(), row.name.as_str())).collect();
        assert_eq!(names, [("0", "Untitled"), ("1", "Untitled 2"), ("2", "Untitled 3")]);
        assert!(snapshot.rows.iter().all(|row| row.ticked));
    }

    #[test]
    fn never_saved_names_avoid_user_supplied_suffix_collisions() {
        let docs = ["Untitled", "Untitled", "Untitled 2", "Untitled 2"]
            .into_iter()
            .enumerate()
            .map(|(index, name)| Document { id: index.to_string(), name: name.to_owned(), path: None, dirty: index != 3, read_only: false })
            .collect();
        let mut model = Closing::new(docs);
        let row_names = |model: &Closing| {
            let names: Vec<_> = model.snapshot().rows.into_iter().map(|row| row.name).collect();
            assert_eq!(names.iter().collect::<BTreeSet<_>>().len(), names.len(), "every never-saved row has a distinct label");
            names
        };
        assert_eq!(row_names(&model), ["Untitled", "Untitled 2", "Untitled 2 2"]);
        assert!(model.row_discard("1"));
        assert_eq!(row_names(&model), ["Untitled", "Untitled 2"]);
        assert!(model.doc_changed("3"));
        assert_eq!(row_names(&model), ["Untitled", "Untitled 2", "Untitled 2 2"]);
    }

    #[test]
    fn never_saved_row_names_survive_partial_save() {
        let docs =
            (0..2).map(|index| Document { id: index.to_string(), name: "Untitled".to_owned(), path: None, dirty: true, read_only: false }).collect();
        let mut model = Closing::new(docs);
        assert!(model.save_selected());
        assert!(model.picked("first.sc".to_owned()));
        assert!(model.save_failed("The disk is full".to_owned()));
        assert!(model.picked("second.sc".to_owned()));
        assert!(model.save_done());

        let snapshot = model.snapshot();
        assert_eq!(snapshot.outcome, None);
        assert!(snapshot.closed.is_empty());
        let names: Vec<_> = snapshot.rows.iter().map(|row| (row.id.as_str(), row.name.as_str())).collect();
        assert_eq!(names, [("0", "Untitled"), ("1", "Untitled 2")]);
        let saved = snapshot.rows.iter().find(|row| row.id == "1").expect("saved row stays visible");
        assert_eq!(saved.where_.as_deref(), Some("second.sc"));
        assert_eq!(saved.state, RowState::Saved);
        assert!(!saved.ticked);
    }

    #[test]
    fn original_save_location_refresh_does_not_replace_a_picker_choice() {
        let docs = || {
            ["first", "second"]
                .into_iter()
                .map(|id| Document { id: id.to_owned(), name: id.to_owned(), path: Some(format!("{id}.sc")), dirty: true, read_only: false })
                .collect()
        };
        let mut model = Closing::new(docs());
        assert!(!model.refresh_save_location(Some("first.sc".to_owned()), true), "there is no pending save yet");
        assert!(model.save_selected());
        assert!(!model.refresh_save_location(Some("first.sc".to_owned()), false), "unchanged metadata is ignored");
        assert!(model.refresh_save_location(Some("first.sc".to_owned()), true));
        let snapshot = model.snapshot();
        assert_eq!(snapshot.request, Some(Request::Pick { id: "first".to_owned(), why: PickWhy::ReadOnly }));
        assert!(snapshot.rows.first().is_some_and(|row| row.read_only));
        assert!(model.picked("first copy.sc".to_owned()));
        assert!(!model.refresh_save_location(Some("first.sc".to_owned()), true));
        assert_eq!(model.snapshot().request, Some(Request::Save { id: "first".to_owned(), path: "first copy.sc".to_owned() }));

        let mut model = Closing::new(docs());
        assert!(model.save_selected());
        assert!(model.refresh_save_location(Some("retargeted.sc".to_owned()), false));
        let snapshot = model.snapshot();
        assert_eq!(snapshot.request, Some(Request::Save { id: "first".to_owned(), path: "retargeted.sc".to_owned() }));
        assert_eq!(snapshot.rows.first().and_then(|row| row.where_.as_deref()), Some("retargeted.sc"));
        assert!(model.refresh_save_location(None, false));
        assert_eq!(model.snapshot().request, Some(Request::Pick { id: "first".to_owned(), why: PickWhy::NeverSaved }));
    }

    #[test]
    fn cancelled_redirected_picker_stops_the_run_without_closing_rows() {
        let docs = ["first", "second"]
            .into_iter()
            .map(|id| Document { id: id.to_owned(), name: id.to_owned(), path: Some(format!("{id}.sc")), dirty: true, read_only: false })
            .collect();
        let mut model = Closing::new(docs);
        assert!(!model.request_location(), "an idle model has no save to redirect");
        assert!(model.save_selected());
        assert!(model.request_location());
        assert_eq!(model.snapshot().request, Some(Request::Pick { id: "first".to_owned(), why: PickWhy::ReadOnly }));
        assert!(!model.request_location(), "a pending picker cannot be redirected again");
        assert!(model.pick_cancelled());

        let snapshot = model.snapshot();
        assert_eq!(snapshot.outcome, None);
        assert_eq!(snapshot.request, None);
        assert!(snapshot.closed.is_empty());
        assert_eq!(snapshot.rows.len(), 2);
        assert!(snapshot.rows.iter().all(|row| row.state == RowState::Unsaved && row.ticked));
        let first = snapshot.rows.first().expect("the cancelled row stays");
        assert_eq!(first.note, Some(Note { kind: NoteKind::Cancelled, text: None }));
        assert!(snapshot.rows.iter().skip(1).all(|row| row.note.is_none()));

        assert!(model.save_selected());
        assert!(model.request_location());
        assert!(model.picked("first copy.sc".to_owned()));
        assert!(model.save_done());
        assert!(model.save_failed("The disk is full".to_owned()));
        assert_eq!(model.snapshot().rows.first().and_then(|row| row.where_.as_deref()), Some("first copy.sc"));
    }

    #[test]
    fn redirected_save_uses_the_completed_path_on_retry() {
        let docs = ["first", "second"]
            .into_iter()
            .map(|id| Document { id: id.to_owned(), name: id.to_owned(), path: Some(format!("{id}.sc")), dirty: true, read_only: false })
            .collect();
        let mut model = Closing::new(docs);
        assert!(!model.save_done_at("ignored.sc".to_owned()), "a completion without a save must be ignored");
        assert!(model.save_selected());
        assert!(model.save_done_at("first copy.sc".to_owned()));
        assert!(model.save_failed("The disk is full".to_owned()));
        let snapshot = model.snapshot();
        let copied = snapshot.rows.iter().find(|row| row.id == "first").expect("saved copy stays listed");
        assert_eq!(copied.where_.as_deref(), Some("first copy.sc"));
        assert_eq!(snapshot.saved.get("first").map(String::as_str), Some("first copy.sc"));

        assert!(model.doc_changed("first"));
        assert!(model.save_selected());
        assert_eq!(
            model.snapshot().request,
            Some(Request::Save { id: "first".to_owned(), path: "first copy.sc".to_owned() }),
            "retry writes the copy rather than replacing the original file"
        );
    }

    #[test]
    fn unticked_document_changed_during_selected_saves_stays_open() {
        let docs = ["first", "second"]
            .into_iter()
            .map(|id| Document { id: id.to_owned(), name: id.to_owned(), path: Some(format!("{id}.sc")), dirty: true, read_only: false })
            .collect();
        let mut model = Closing::new(docs);
        assert!(model.tick("second", false));
        assert!(model.doc_changed("second"));
        assert!(model.snapshot().rows.iter().any(|row| row.id == "second" && !row.ticked), "idle changes preserve the user's tick choice");
        assert!(model.save_selected());
        assert!(model.doc_changed("second"));
        assert!(model.save_done());

        let snapshot = model.snapshot();
        assert_eq!(snapshot.outcome, None);
        assert_eq!(snapshot.request, None);
        assert!(snapshot.closed.is_empty());
        assert!(snapshot.lost.is_empty());
        let changed = snapshot.rows.iter().find(|row| row.id == "second").expect("changed row stays listed");
        assert_eq!(changed.state, RowState::Unsaved);
        assert!(changed.ticked, "the later change needs another saving decision");
    }

    enum Input {
        Tick(&'static str, bool),
        SaveSelected,
        DontSave,
        Cancel,
        RowSave(&'static str),
        RowDiscard(&'static str),
        Picked(&'static str),
        PickCancelled,
        SaveDone,
        SaveFailed(&'static str),
        DocChanged(&'static str),
    }

    enum Step {
        /// The model takes this input.
        Accepted(Input),
        /// The model ignores this input (it is not allowed in the current state).
        Ignored(Input),
        /// The snapshot matches this value (only the keys named here are compared).
        Expect(Value),
    }

    fn play(docs: Vec<Document>, steps: Vec<Step>) {
        let mut model = Closing::new(docs);
        for (index, step) in steps.into_iter().enumerate() {
            let number = index + 1;
            let (input, wanted) = match step {
                Step::Expect(want) => {
                    let got = serde_json::to_value(model.snapshot()).expect("serializable snapshot");
                    let found = difference(&got, &want, "snapshot");
                    assert!(found.is_none(), "step {number}: {}", found.unwrap_or_default());
                    continue;
                }
                Step::Accepted(input) => (input, true),
                Step::Ignored(input) => (input, false),
            };
            let accepted = match input {
                Input::Tick(id, on) => model.tick(id, on),
                Input::SaveSelected => model.save_selected(),
                Input::DontSave => model.dont_save(),
                Input::Cancel => model.cancel(),
                Input::RowSave(id) => model.row_save(id),
                Input::RowDiscard(id) => model.row_discard(id),
                Input::Picked(path) => model.picked(path.to_owned()),
                Input::PickCancelled => model.pick_cancelled(),
                Input::SaveDone => model.save_done(),
                Input::SaveFailed(reason) => model.save_failed(reason.to_owned()),
                Input::DocChanged(id) => model.doc_changed(id),
            };
            assert_eq!(accepted, wanted, "step {number}: input acceptance");
        }
    }

    /// Nothing unsaved: no prompt, the action goes ahead
    #[test]
    fn nothing_unsaved_means_no_prompt_and_the_action_goes_ahead() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: false,
                    read_only: false,
                },
                Document {
                    id: "gearbox".to_owned(),
                    name: "Gearbox".to_owned(),
                    path: Some("~/Designs/Gearbox.sc".to_owned()),
                    dirty: false,
                    read_only: false,
                },
            ],
            vec![Step::Expect(json!({"prompt": "none", "outcome": "proceeded", "open": [], "lost": []}))],
        );
    }

    /// One unsaved document keeps the single prompt
    #[test]
    fn one_unsaved_document_keeps_the_single_prompt() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document {
                    id: "gearbox".to_owned(),
                    name: "Gearbox".to_owned(),
                    path: Some("~/Designs/Gearbox.sc".to_owned()),
                    dirty: false,
                    read_only: false,
                },
            ],
            vec![Step::Expect(json!({"prompt": "single", "count": 1, "rows": [{"id": "bracket"}], "outcome": null, "open": ["bracket", "gearbox"]}))],
        );
    }

    /// Two or more unsaved: one list, a row each in order, all ticked, clean documents left out
    #[test]
    fn two_unsaved_documents_open_one_ticked_list_without_clean_ones() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "lid".to_owned(), name: "Lid".to_owned(), path: Some("~/Designs/Lid.sc".to_owned()), dirty: true, read_only: false },
                Document { id: "untitled".to_owned(), name: "Untitled".to_owned(), path: None.map(str::to_owned), dirty: true, read_only: false },
                Document {
                    id: "gearbox".to_owned(),
                    name: "Gearbox".to_owned(),
                    path: Some("~/Designs/Gearbox.sc".to_owned()),
                    dirty: false,
                    read_only: false,
                },
            ],
            vec![Step::Expect(
                json!({"prompt": "list", "count": 3, "request": null, "outcome": null, "rows": [{"id": "bracket", "name": "Bracket", "where": "~/Designs/Bracket.sc", "ticked": true, "state": "unsaved", "note": null}, {"id": "lid", "where": "~/Designs/Lid.sc", "ticked": true}, {"id": "untitled", "where": null, "ticked": true}]}),
            )],
        );
    }

    /// Save Selected saves in row order without asking where, then everything closes
    #[test]
    fn save_selected_saves_in_row_order_without_asking_where_then_closes_all() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "lid".to_owned(), name: "Lid".to_owned(), path: Some("~/Designs/Lid.sc".to_owned()), dirty: true, read_only: false },
                Document {
                    id: "gearbox".to_owned(),
                    name: "Gearbox".to_owned(),
                    path: Some("~/Designs/Gearbox.sc".to_owned()),
                    dirty: false,
                    read_only: false,
                },
            ],
            vec![
                Step::Accepted(Input::SaveSelected),
                Step::Expect(
                    json!({"request": {"type": "save", "id": "bracket", "path": "~/Designs/Bracket.sc"}, "rows": [{"id": "bracket", "state": "saving"}, {"id": "lid", "state": "unsaved"}]}),
                ),
                Step::Accepted(Input::SaveDone),
                Step::Expect(
                    json!({"request": {"type": "save", "id": "lid"}, "rows": [{"id": "bracket", "state": "saved"}, {"id": "lid", "state": "saving"}], "open": ["bracket", "lid", "gearbox"]}),
                ),
                Step::Accepted(Input::SaveDone),
                Step::Expect(
                    json!({"prompt": "none", "outcome": "proceeded", "open": [], "saved": {"bracket": "~/Designs/Bracket.sc", "lid": "~/Designs/Lid.sc"}, "lost": []}),
                ),
            ],
        );
    }

    /// Save Selected asks where only for the never-saved document
    #[test]
    fn save_selected_asks_where_only_for_the_never_saved_document() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "untitled".to_owned(), name: "Untitled".to_owned(), path: None.map(str::to_owned), dirty: true, read_only: false },
            ],
            vec![
                Step::Accepted(Input::SaveSelected),
                Step::Expect(json!({"request": {"type": "save", "id": "bracket"}})),
                Step::Accepted(Input::SaveDone),
                Step::Expect(json!({"request": {"type": "pick", "id": "untitled", "why": "never-saved"}})),
                Step::Accepted(Input::Picked("~/Designs/Clip.sc")),
                Step::Expect(json!({"request": {"type": "save", "id": "untitled", "path": "~/Designs/Clip.sc"}})),
                Step::Accepted(Input::SaveDone),
                Step::Expect(json!({"outcome": "proceeded", "saved": {"untitled": "~/Designs/Clip.sc"}, "lost": []})),
            ],
        );
    }

    /// A save fails: nothing closes, the list stays, the row says why, the saved one shows as saved
    #[test]
    fn a_failed_save_closes_nothing_and_the_row_says_why() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "lid".to_owned(), name: "Lid".to_owned(), path: Some("~/Designs/Lid.sc".to_owned()), dirty: true, read_only: false },
                Document {
                    id: "gearbox".to_owned(),
                    name: "Gearbox".to_owned(),
                    path: Some("~/Designs/Gearbox.sc".to_owned()),
                    dirty: false,
                    read_only: false,
                },
            ],
            vec![
                Step::Accepted(Input::SaveSelected),
                Step::Accepted(Input::SaveDone),
                Step::Accepted(Input::SaveFailed("The disk is full")),
                Step::Expect(
                    json!({"prompt": "list", "outcome": null, "request": null, "count": 1, "open": ["bracket", "lid", "gearbox"], "closed": [], "rows": [{"id": "bracket", "state": "saved", "ticked": false, "note": null}, {"id": "lid", "state": "unsaved", "ticked": true, "note": {"kind": "failed", "text": "The disk is full"}}]}),
                ),
            ],
        );
    }

    /// A failed save doesn't stop the others: the later documents are still saved, and nothing closes
    #[test]
    fn a_failed_save_does_not_stop_the_later_documents_saving() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "lid".to_owned(), name: "Lid".to_owned(), path: Some("~/Designs/Lid.sc".to_owned()), dirty: true, read_only: false },
                Document { id: "untitled".to_owned(), name: "Untitled".to_owned(), path: None.map(str::to_owned), dirty: true, read_only: false },
            ],
            vec![
                Step::Accepted(Input::SaveSelected),
                Step::Accepted(Input::SaveFailed("The disk is full")),
                Step::Expect(json!({"request": {"type": "save", "id": "lid"}})),
                Step::Accepted(Input::SaveDone),
                Step::Expect(json!({"request": {"type": "pick", "id": "untitled"}})),
                Step::Accepted(Input::Picked("~/Designs/Clip.sc")),
                Step::Accepted(Input::SaveDone),
                Step::Expect(
                    json!({"prompt": "list", "outcome": null, "count": 1, "closed": [], "rows": [{"id": "bracket", "state": "unsaved", "note": {"kind": "failed"}}, {"id": "lid", "state": "saved"}, {"id": "untitled", "state": "saved", "where": "~/Designs/Clip.sc"}]}),
                ),
            ],
        );
    }

    /// A cancelled picker stops the run: no second picker, nothing closes
    #[test]
    fn a_cancelled_picker_stops_the_run_with_no_second_picker() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "untitled".to_owned(), name: "Untitled".to_owned(), path: None.map(str::to_owned), dirty: true, read_only: false },
                Document { id: "untitled2".to_owned(), name: "Untitled 2".to_owned(), path: None.map(str::to_owned), dirty: true, read_only: false },
            ],
            vec![
                Step::Accepted(Input::SaveSelected),
                Step::Accepted(Input::SaveDone),
                Step::Expect(json!({"request": {"type": "pick", "id": "untitled"}})),
                Step::Accepted(Input::PickCancelled),
                Step::Expect(
                    json!({"prompt": "list", "request": null, "outcome": null, "closed": [], "count": 2, "rows": [{"id": "bracket", "state": "saved"}, {"id": "untitled", "state": "unsaved", "note": {"kind": "cancelled"}}, {"id": "untitled2", "state": "unsaved", "note": null}]}),
                ),
            ],
        );
    }

    /// After the failure is fixed, Save Selected again saves the rest and everything closes
    #[test]
    fn save_selected_again_after_the_failure_is_fixed_saves_the_rest_and_closes_all() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "lid".to_owned(), name: "Lid".to_owned(), path: Some("~/Designs/Lid.sc".to_owned()), dirty: true, read_only: false },
            ],
            vec![
                Step::Accepted(Input::SaveSelected),
                Step::Accepted(Input::SaveDone),
                Step::Accepted(Input::SaveFailed("The disk is full")),
                Step::Accepted(Input::SaveSelected),
                Step::Expect(json!({"request": {"type": "save", "id": "lid"}, "rows": [{"id": "bracket"}, {"id": "lid", "note": null}]})),
                Step::Accepted(Input::SaveDone),
                Step::Expect(
                    json!({"outcome": "proceeded", "open": [], "lost": [], "saved": {"bracket": "~/Designs/Bracket.sc", "lid": "~/Designs/Lid.sc"}}),
                ),
            ],
        );
    }

    /// An unticked document closes without saving when the ticked ones have saved
    #[test]
    fn unticked_documents_close_unsaved_once_the_ticked_ones_have_saved() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "lid".to_owned(), name: "Lid".to_owned(), path: Some("~/Designs/Lid.sc".to_owned()), dirty: true, read_only: false },
                Document { id: "untitled".to_owned(), name: "Untitled".to_owned(), path: None.map(str::to_owned), dirty: true, read_only: false },
            ],
            vec![
                Step::Accepted(Input::Tick("lid", false)),
                Step::Expect(json!({"rows": [{"ticked": true}, {"id": "lid", "ticked": false}, {"ticked": true}]})),
                Step::Accepted(Input::SaveSelected),
                Step::Accepted(Input::SaveDone),
                Step::Accepted(Input::Picked("~/Designs/Clip.sc")),
                Step::Accepted(Input::SaveDone),
                Step::Expect(
                    json!({"outcome": "proceeded", "open": [], "lost": ["lid"], "saved": {"bracket": "~/Designs/Bracket.sc", "untitled": "~/Designs/Clip.sc"}}),
                ),
            ],
        );
    }

    /// When a save fails, the unticked documents don't close either
    #[test]
    fn unticked_documents_stay_open_when_a_save_fails() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "lid".to_owned(), name: "Lid".to_owned(), path: Some("~/Designs/Lid.sc".to_owned()), dirty: true, read_only: false },
                Document { id: "untitled".to_owned(), name: "Untitled".to_owned(), path: None.map(str::to_owned), dirty: true, read_only: false },
            ],
            vec![
                Step::Accepted(Input::Tick("untitled", false)),
                Step::Accepted(Input::SaveSelected),
                Step::Accepted(Input::SaveDone),
                Step::Accepted(Input::SaveFailed("The disk is full")),
                Step::Expect(
                    json!({"outcome": null, "closed": [], "lost": [], "rows": [{"id": "bracket"}, {"id": "lid"}, {"id": "untitled", "ticked": false, "state": "unsaved"}]}),
                ),
            ],
        );
    }

    /// Save Selected with nothing ticked closes everything unsaved, like Don't Save
    #[test]
    fn save_selected_with_nothing_ticked_closes_everything_like_dont_save() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "lid".to_owned(), name: "Lid".to_owned(), path: Some("~/Designs/Lid.sc".to_owned()), dirty: true, read_only: false },
            ],
            vec![
                Step::Accepted(Input::Tick("bracket", false)),
                Step::Accepted(Input::Tick("lid", false)),
                Step::Accepted(Input::SaveSelected),
                Step::Expect(json!({"outcome": "proceeded", "lost": ["bracket", "lid"], "saved": {}})),
            ],
        );
    }

    /// Don't Save closes every listed document without saving
    #[test]
    fn dont_save_closes_every_listed_document_without_saving() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "lid".to_owned(), name: "Lid".to_owned(), path: Some("~/Designs/Lid.sc".to_owned()), dirty: true, read_only: false },
                Document { id: "untitled".to_owned(), name: "Untitled".to_owned(), path: None.map(str::to_owned), dirty: true, read_only: false },
                Document {
                    id: "gearbox".to_owned(),
                    name: "Gearbox".to_owned(),
                    path: Some("~/Designs/Gearbox.sc".to_owned()),
                    dirty: false,
                    read_only: false,
                },
            ],
            vec![
                Step::Accepted(Input::DontSave),
                Step::Expect(json!({"prompt": "none", "outcome": "proceeded", "open": [], "lost": ["bracket", "lid", "untitled"], "saved": {}})),
            ],
        );
    }

    /// Cancel closes nothing and stops the action
    #[test]
    fn cancel_closes_nothing_and_stops_the_action() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "lid".to_owned(), name: "Lid".to_owned(), path: Some("~/Designs/Lid.sc".to_owned()), dirty: true, read_only: false },
                Document {
                    id: "gearbox".to_owned(),
                    name: "Gearbox".to_owned(),
                    path: Some("~/Designs/Gearbox.sc".to_owned()),
                    dirty: false,
                    read_only: false,
                },
            ],
            vec![
                Step::Accepted(Input::Cancel),
                Step::Expect(json!({"prompt": "none", "outcome": "stopped", "open": ["bracket", "lid", "gearbox"], "closed": [], "lost": []})),
                Step::Ignored(Input::SaveSelected),
                Step::Ignored(Input::DontSave),
            ],
        );
    }

    /// Cancel closes nothing more: rows already answered stay answered
    #[test]
    fn cancel_after_row_answers_closes_nothing_more_and_answered_rows_stay_answered() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "lid".to_owned(), name: "Lid".to_owned(), path: Some("~/Designs/Lid.sc".to_owned()), dirty: true, read_only: false },
                Document { id: "untitled".to_owned(), name: "Untitled".to_owned(), path: None.map(str::to_owned), dirty: true, read_only: false },
            ],
            vec![
                Step::Accepted(Input::RowSave("bracket")),
                Step::Accepted(Input::SaveDone),
                Step::Accepted(Input::RowDiscard("lid")),
                Step::Expect(json!({"prompt": "list", "count": 1, "rows": [{"id": "untitled"}], "closed": ["bracket", "lid"]})),
                Step::Accepted(Input::Cancel),
                Step::Expect(
                    json!({"outcome": "stopped", "open": ["untitled"], "closed": ["bracket", "lid"], "saved": {"bracket": "~/Designs/Bracket.sc"}, "lost": ["lid"]}),
                ),
            ],
        );
    }

    /// A row's Save saves that document only; it closes and its row leaves
    #[test]
    fn a_row_save_saves_only_that_document_and_its_row_leaves() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "lid".to_owned(), name: "Lid".to_owned(), path: Some("~/Designs/Lid.sc".to_owned()), dirty: true, read_only: false },
                Document { id: "untitled".to_owned(), name: "Untitled".to_owned(), path: None.map(str::to_owned), dirty: true, read_only: false },
            ],
            vec![
                Step::Accepted(Input::Tick("untitled", false)),
                Step::Accepted(Input::RowSave("lid")),
                Step::Expect(json!({"request": {"type": "save", "id": "lid"}})),
                Step::Accepted(Input::SaveDone),
                Step::Expect(
                    json!({"prompt": "list", "outcome": null, "count": 2, "closed": ["lid"], "rows": [{"id": "bracket", "ticked": true, "state": "unsaved"}, {"id": "untitled", "ticked": false, "state": "unsaved"}]}),
                ),
            ],
        );
    }

    /// A row's Save that fails leaves the row, with the reason
    #[test]
    fn a_row_save_that_fails_keeps_the_row_with_the_reason() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "lid".to_owned(), name: "Lid".to_owned(), path: Some("~/Designs/Lid.sc".to_owned()), dirty: true, read_only: false },
            ],
            vec![
                Step::Accepted(Input::RowSave("lid")),
                Step::Accepted(Input::SaveFailed("The disk is full")),
                Step::Expect(
                    json!({"prompt": "list", "closed": [], "request": null, "rows": [{"id": "bracket", "note": null}, {"id": "lid", "state": "unsaved", "note": {"kind": "failed", "text": "The disk is full"}}]}),
                ),
            ],
        );
    }

    /// A row's Save on a never-saved document asks where; cancelling leaves the row
    #[test]
    fn a_row_save_on_a_never_saved_document_asks_where_and_cancelling_keeps_the_row() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "untitled".to_owned(), name: "Untitled".to_owned(), path: None.map(str::to_owned), dirty: true, read_only: false },
            ],
            vec![
                Step::Accepted(Input::RowSave("untitled")),
                Step::Expect(json!({"request": {"type": "pick", "id": "untitled", "why": "never-saved"}})),
                Step::Accepted(Input::PickCancelled),
                Step::Expect(
                    json!({"prompt": "list", "closed": [], "rows": [{"id": "bracket"}, {"id": "untitled", "state": "unsaved", "note": {"kind": "cancelled"}}]}),
                ),
            ],
        );
    }

    /// A row's Discard closes that document only
    #[test]
    fn a_row_discard_closes_only_that_document() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "lid".to_owned(), name: "Lid".to_owned(), path: Some("~/Designs/Lid.sc".to_owned()), dirty: true, read_only: false },
                Document { id: "untitled".to_owned(), name: "Untitled".to_owned(), path: None.map(str::to_owned), dirty: true, read_only: false },
            ],
            vec![
                Step::Accepted(Input::RowDiscard("lid")),
                Step::Expect(
                    json!({"prompt": "list", "count": 2, "closed": ["lid"], "lost": ["lid"], "rows": [{"id": "bracket", "ticked": true}, {"id": "untitled", "ticked": true}]}),
                ),
            ],
        );
    }

    /// Answering the last row carries on with the action
    #[test]
    fn answering_the_last_row_carries_on_with_the_action() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "lid".to_owned(), name: "Lid".to_owned(), path: Some("~/Designs/Lid.sc".to_owned()), dirty: true, read_only: false },
                Document {
                    id: "gearbox".to_owned(),
                    name: "Gearbox".to_owned(),
                    path: Some("~/Designs/Gearbox.sc".to_owned()),
                    dirty: false,
                    read_only: false,
                },
            ],
            vec![
                Step::Accepted(Input::RowSave("bracket")),
                Step::Accepted(Input::SaveDone),
                Step::Expect(json!({"prompt": "list", "count": 1, "outcome": null})),
                Step::Accepted(Input::RowDiscard("lid")),
                Step::Expect(
                    json!({"prompt": "none", "outcome": "proceeded", "open": [], "saved": {"bracket": "~/Designs/Bracket.sc"}, "lost": ["lid"]}),
                ),
            ],
        );
    }

    /// After a failed Save Selected, discarding the failed row carries on and closes the saved ones too
    #[test]
    fn discarding_the_failed_row_after_a_partial_save_closes_the_saved_ones_too() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "lid".to_owned(), name: "Lid".to_owned(), path: Some("~/Designs/Lid.sc".to_owned()), dirty: true, read_only: false },
            ],
            vec![
                Step::Accepted(Input::SaveSelected),
                Step::Accepted(Input::SaveDone),
                Step::Accepted(Input::SaveFailed("The disk is full")),
                Step::Expect(json!({"closed": []})),
                Step::Accepted(Input::RowDiscard("lid")),
                Step::Expect(json!({"outcome": "proceeded", "open": [], "saved": {"bracket": "~/Designs/Bracket.sc"}, "lost": ["lid"]})),
            ],
        );
    }

    /// A row that already saved has nothing left to answer
    #[test]
    fn a_row_that_already_saved_has_nothing_left_to_answer() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "lid".to_owned(), name: "Lid".to_owned(), path: Some("~/Designs/Lid.sc".to_owned()), dirty: true, read_only: false },
            ],
            vec![
                Step::Accepted(Input::SaveSelected),
                Step::Accepted(Input::SaveDone),
                Step::Accepted(Input::SaveFailed("The disk is full")),
                Step::Ignored(Input::RowSave("bracket")),
                Step::Ignored(Input::RowDiscard("bracket")),
                Step::Ignored(Input::Tick("bracket", true)),
            ],
        );
    }

    /// A read-only file asks for a new place, and the row knows why
    #[test]
    fn a_read_only_file_asks_for_a_new_place_and_the_row_says_why() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: true,
                },
                Document { id: "lid".to_owned(), name: "Lid".to_owned(), path: Some("~/Designs/Lid.sc".to_owned()), dirty: true, read_only: false },
            ],
            vec![
                Step::Expect(json!({"rows": [{"id": "bracket", "readOnly": true}, {"id": "lid", "readOnly": false}]})),
                Step::Accepted(Input::SaveSelected),
                Step::Expect(json!({"request": {"type": "pick", "id": "bracket", "why": "read-only"}})),
                Step::Accepted(Input::Picked("~/Designs/Bracket copy.sc")),
                Step::Accepted(Input::SaveDone),
                Step::Expect(
                    json!({"request": {"type": "save", "id": "lid"}, "rows": [{"id": "bracket", "state": "saved", "where": "~/Designs/Bracket copy.sc", "readOnly": false}, {"id": "lid"}]}),
                ),
                Step::Accepted(Input::SaveDone),
                Step::Expect(json!({"outcome": "proceeded", "saved": {"bracket": "~/Designs/Bracket copy.sc"}})),
            ],
        );
    }

    /// While a save is running every button except Cancel waits
    #[test]
    fn while_saving_every_button_except_cancel_waits() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "lid".to_owned(), name: "Lid".to_owned(), path: Some("~/Designs/Lid.sc".to_owned()), dirty: true, read_only: false },
                Document { id: "untitled".to_owned(), name: "Untitled".to_owned(), path: None.map(str::to_owned), dirty: true, read_only: false },
            ],
            vec![
                Step::Accepted(Input::SaveSelected),
                Step::Expect(json!({"request": {"type": "save", "id": "bracket"}, "rows": [{"state": "saving"}, {}, {}]})),
                Step::Ignored(Input::SaveSelected),
                Step::Ignored(Input::DontSave),
                Step::Ignored(Input::RowSave("lid")),
                Step::Ignored(Input::RowDiscard("lid")),
                Step::Ignored(Input::Tick("lid", false)),
                Step::Expect(json!({"request": {"type": "save", "id": "bracket"}, "rows": [{}, {"id": "lid", "ticked": true}, {}]})),
            ],
        );
    }

    /// Cancel during a slow save abandons it and stops the rest; what already saved stays saved; nothing closes
    #[test]
    fn cancel_during_a_slow_save_abandons_it_keeps_what_saved_and_closes_nothing() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "lid".to_owned(), name: "Lid".to_owned(), path: Some("~/Designs/Lid.sc".to_owned()), dirty: true, read_only: false },
                Document { id: "untitled".to_owned(), name: "Untitled".to_owned(), path: None.map(str::to_owned), dirty: true, read_only: false },
            ],
            vec![
                Step::Accepted(Input::SaveSelected),
                Step::Accepted(Input::SaveDone),
                Step::Expect(json!({"request": {"type": "save", "id": "lid"}})),
                Step::Accepted(Input::Cancel),
                Step::Expect(
                    json!({"outcome": "stopped", "request": null, "abandoned": ["lid"], "open": ["bracket", "lid", "untitled"], "closed": [], "saved": {"bracket": "~/Designs/Bracket.sc"}, "lost": []}),
                ),
                Step::Ignored(Input::SaveDone),
            ],
        );
    }

    /// While a save picker is open, it is the picker that gets answered, not the list
    #[test]
    fn while_a_picker_is_open_the_picker_is_answered_not_the_list() {
        play(
            vec![
                Document { id: "untitled".to_owned(), name: "Untitled".to_owned(), path: None.map(str::to_owned), dirty: true, read_only: false },
                Document { id: "untitled2".to_owned(), name: "Untitled 2".to_owned(), path: None.map(str::to_owned), dirty: true, read_only: false },
            ],
            vec![
                Step::Accepted(Input::SaveSelected),
                Step::Expect(json!({"request": {"type": "pick", "id": "untitled"}})),
                Step::Ignored(Input::Cancel),
                Step::Ignored(Input::DontSave),
                Step::Accepted(Input::PickCancelled),
                Step::Accepted(Input::Cancel),
                Step::Expect(json!({"outcome": "stopped", "closed": []})),
            ],
        );
    }

    /// A document that changes again after it saved needs saving again; its row stays
    #[test]
    fn a_document_that_changes_after_saving_needs_saving_again_and_keeps_its_row() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "lid".to_owned(), name: "Lid".to_owned(), path: Some("~/Designs/Lid.sc".to_owned()), dirty: true, read_only: false },
            ],
            vec![
                Step::Accepted(Input::SaveSelected),
                Step::Accepted(Input::SaveDone),
                Step::Accepted(Input::SaveFailed("The disk is full")),
                Step::Expect(json!({"count": 1})),
                Step::Accepted(Input::DocChanged("bracket")),
                Step::Expect(json!({"count": 2, "rows": [{"id": "bracket", "state": "unsaved", "ticked": true}, {"id": "lid"}]})),
                Step::Accepted(Input::SaveSelected),
                Step::Expect(json!({"request": {"type": "save", "id": "bracket"}})),
            ],
        );
    }

    /// A document that gains changes while the list is open gets a row, ticked
    #[test]
    fn a_clean_document_that_gains_changes_while_the_list_is_open_gets_a_ticked_row() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "lid".to_owned(), name: "Lid".to_owned(), path: Some("~/Designs/Lid.sc".to_owned()), dirty: true, read_only: false },
                Document {
                    id: "gearbox".to_owned(),
                    name: "Gearbox".to_owned(),
                    path: Some("~/Designs/Gearbox.sc".to_owned()),
                    dirty: false,
                    read_only: false,
                },
            ],
            vec![
                Step::Expect(json!({"count": 2})),
                Step::Accepted(Input::DocChanged("gearbox")),
                Step::Expect(
                    json!({"prompt": "list", "count": 3, "rows": [{"id": "bracket"}, {"id": "lid"}, {"id": "gearbox", "state": "unsaved", "ticked": true}]}),
                ),
            ],
        );
    }

    /// The single prompt: Save saves it and the action goes ahead
    #[test]
    fn single_prompt_save_saves_it_and_the_action_goes_ahead() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document {
                    id: "gearbox".to_owned(),
                    name: "Gearbox".to_owned(),
                    path: Some("~/Designs/Gearbox.sc".to_owned()),
                    dirty: false,
                    read_only: false,
                },
            ],
            vec![
                Step::Accepted(Input::SaveSelected),
                Step::Expect(json!({"request": {"type": "save", "id": "bracket"}})),
                Step::Accepted(Input::SaveDone),
                Step::Expect(json!({"outcome": "proceeded", "open": [], "lost": []})),
            ],
        );
    }

    /// The single prompt: a failed save closes nothing and the prompt stays, with the reason
    #[test]
    fn single_prompt_failed_save_closes_nothing_and_keeps_the_prompt_with_the_reason() {
        play(
            vec![
                Document { id: "untitled".to_owned(), name: "Untitled".to_owned(), path: None.map(str::to_owned), dirty: true, read_only: false },
                Document {
                    id: "gearbox".to_owned(),
                    name: "Gearbox".to_owned(),
                    path: Some("~/Designs/Gearbox.sc".to_owned()),
                    dirty: false,
                    read_only: false,
                },
            ],
            vec![
                Step::Accepted(Input::SaveSelected),
                Step::Accepted(Input::Picked("~/Designs/Clip.sc")),
                Step::Accepted(Input::SaveFailed("The disk is full")),
                Step::Expect(
                    json!({"prompt": "single", "outcome": null, "closed": [], "rows": [{"id": "untitled", "where": null, "note": {"kind": "failed", "text": "The disk is full"}}]}),
                ),
            ],
        );
    }

    /// The single prompt: Don't Save closes it unsaved
    #[test]
    fn single_prompt_dont_save_closes_it_unsaved() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document {
                    id: "gearbox".to_owned(),
                    name: "Gearbox".to_owned(),
                    path: Some("~/Designs/Gearbox.sc".to_owned()),
                    dirty: false,
                    read_only: false,
                },
            ],
            vec![Step::Accepted(Input::DontSave), Step::Expect(json!({"outcome": "proceeded", "lost": ["bracket"], "open": []}))],
        );
    }

    /// The single prompt becomes the list when a second document gains changes
    #[test]
    fn single_prompt_becomes_the_list_when_a_second_document_gains_changes() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document {
                    id: "gearbox".to_owned(),
                    name: "Gearbox".to_owned(),
                    path: Some("~/Designs/Gearbox.sc".to_owned()),
                    dirty: false,
                    read_only: false,
                },
            ],
            vec![
                Step::Expect(json!({"prompt": "single"})),
                Step::Accepted(Input::DocChanged("gearbox")),
                Step::Expect(json!({"prompt": "list", "count": 2})),
            ],
        );
    }

    /// A document that saved and then changes while the others are still saving does not close unasked: the list stays
    #[test]
    fn a_document_that_changes_after_saving_during_a_run_does_not_close_unasked() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "lid".to_owned(), name: "Lid".to_owned(), path: Some("~/Designs/Lid.sc".to_owned()), dirty: true, read_only: false },
            ],
            vec![
                Step::Accepted(Input::SaveSelected),
                Step::Accepted(Input::SaveDone),
                Step::Accepted(Input::DocChanged("bracket")),
                Step::Expect(json!({"request": {"type": "save", "id": "lid"}})),
                Step::Accepted(Input::SaveDone),
                Step::Expect(
                    json!({"prompt": "list", "outcome": null, "closed": [], "lost": [], "count": 1, "rows": [{"id": "bracket", "state": "unsaved", "ticked": true}, {"id": "lid", "state": "saved"}]}),
                ),
                Step::Accepted(Input::SaveSelected),
                Step::Accepted(Input::SaveDone),
                Step::Expect(json!({"outcome": "proceeded", "lost": []})),
            ],
        );
    }

    /// A document that gains changes during a run of saves gets a row and holds the list open
    #[test]
    fn a_clean_document_that_gains_changes_during_a_run_gets_a_row_and_holds_the_list_open() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "lid".to_owned(), name: "Lid".to_owned(), path: Some("~/Designs/Lid.sc".to_owned()), dirty: true, read_only: false },
                Document {
                    id: "gearbox".to_owned(),
                    name: "Gearbox".to_owned(),
                    path: Some("~/Designs/Gearbox.sc".to_owned()),
                    dirty: false,
                    read_only: false,
                },
            ],
            vec![
                Step::Accepted(Input::SaveSelected),
                Step::Accepted(Input::DocChanged("gearbox")),
                Step::Accepted(Input::SaveDone),
                Step::Accepted(Input::SaveDone),
                Step::Expect(
                    json!({"prompt": "list", "outcome": null, "closed": [], "lost": [], "count": 1, "rows": [{"id": "bracket", "state": "saved"}, {"id": "lid", "state": "saved"}, {"id": "gearbox", "state": "unsaved", "ticked": true}]}),
                ),
            ],
        );
    }

    /// A change that arrives while that document is being saved leaves it needing a save
    #[test]
    fn a_change_while_the_document_is_being_saved_leaves_it_needing_a_save() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "lid".to_owned(), name: "Lid".to_owned(), path: Some("~/Designs/Lid.sc".to_owned()), dirty: true, read_only: false },
            ],
            vec![
                Step::Accepted(Input::SaveSelected),
                Step::Expect(json!({"request": {"type": "save", "id": "bracket"}})),
                Step::Accepted(Input::DocChanged("bracket")),
                Step::Accepted(Input::SaveDone),
                Step::Accepted(Input::SaveDone),
                Step::Expect(
                    json!({"prompt": "list", "outcome": null, "closed": [], "count": 1, "rows": [{"id": "bracket", "state": "unsaved", "ticked": true}, {"id": "lid", "state": "saved"}]}),
                ),
            ],
        );
    }

    /// A row's Save whose document changes during the save does not close it
    #[test]
    fn a_row_save_whose_document_changes_during_the_save_does_not_close_it() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document { id: "lid".to_owned(), name: "Lid".to_owned(), path: Some("~/Designs/Lid.sc".to_owned()), dirty: true, read_only: false },
            ],
            vec![
                Step::Accepted(Input::RowSave("bracket")),
                Step::Accepted(Input::DocChanged("bracket")),
                Step::Accepted(Input::SaveDone),
                Step::Expect(json!({"prompt": "list", "closed": [], "count": 2, "rows": [{"id": "bracket", "state": "unsaved"}, {"id": "lid"}]})),
            ],
        );
    }

    /// The single prompt has no tick box: its document can't be unticked
    #[test]
    fn single_prompt_has_no_tick_so_its_document_cannot_be_unticked() {
        play(
            vec![
                Document {
                    id: "bracket".to_owned(),
                    name: "Bracket".to_owned(),
                    path: Some("~/Designs/Bracket.sc".to_owned()),
                    dirty: true,
                    read_only: false,
                },
                Document {
                    id: "gearbox".to_owned(),
                    name: "Gearbox".to_owned(),
                    path: Some("~/Designs/Gearbox.sc".to_owned()),
                    dirty: false,
                    read_only: false,
                },
            ],
            vec![
                Step::Ignored(Input::Tick("bracket", false)),
                Step::Accepted(Input::SaveSelected),
                Step::Expect(json!({"request": {"type": "save", "id": "bracket"}})),
            ],
        );
    }
}
