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
    use serde_json::Value;

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

    #[test]
    fn no_file_left_behind_scenarios() {
        let suite: Value = serde_json::from_str(include_str!("testdata/nflb-scenarios.json")).expect("valid scenario file");
        let scenarios = suite["scenarios"].as_array().expect("scenario array");
        assert_eq!(scenarios.len(), 37);
        for scenario in scenarios {
            let id = scenario["id"].as_str().expect("scenario id");
            let docs = serde_json::from_value(scenario["docs"].clone()).expect("scenario documents");
            let mut model = Closing::new(docs);
            for (index, step) in scenario["steps"].as_array().expect("scenario steps").iter().enumerate() {
                if let Some(want) = step.get("expect") {
                    let got = serde_json::to_value(model.snapshot()).expect("serializable snapshot");
                    assert!(
                        difference(&got, want, "snapshot").is_none(),
                        "{id}, step {}: {}",
                        index + 1,
                        difference(&got, want, "snapshot").unwrap_or_default()
                    );
                    continue;
                }
                let doc_id = step["id"].as_str().unwrap_or_default();
                let input = step["do"].as_str().expect("input name");
                let accepted = match input {
                    "tick" => model.tick(doc_id, step["on"].as_bool().expect("tick value")),
                    "saveSelected" => model.save_selected(),
                    "dontSave" => model.dont_save(),
                    "cancel" => model.cancel(),
                    "rowSave" => model.row_save(doc_id),
                    "rowDiscard" => model.row_discard(doc_id),
                    "picked" => model.picked(step["path"].as_str().expect("picked path").to_owned()),
                    "pickCancelled" => model.pick_cancelled(),
                    "saveDone" => model.save_done(),
                    "saveFailed" => model.save_failed(step["reason"].as_str().expect("failure reason").to_owned()),
                    "docChanged" => model.doc_changed(doc_id),
                    _ => panic!("{id}: unknown input {input}"),
                };
                assert_eq!(accepted, !step["ignored"].as_bool().unwrap_or(false), "{id}, step {}: {input} acceptance", index + 1);
            }
        }
    }
}
