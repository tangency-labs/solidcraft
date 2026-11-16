//! The views of one design: Model, Sketch, Sheet, Render and Animation, shown as sub-tabs of the
//! design's tab. Pure data and small functions, no egui, so the strip, the commands, the menus
//! and the saved form all read the same [`ViewSet`]. A view change reaches the app through
//! `documents::apply_view`, the one place that knows what a kind means for the window.

use serde_json::{Value, json};

/// Most views one design keeps open (and most a saved set may hold).
pub const MAX_VIEWS: usize = 64;
/// Longest view name, in characters (what `tab.view.rename` takes too).
pub const MAX_NAME: usize = solidcraft_engine::schema::tab_views::MAX_NAME;
/// Longest subject (the id of the sketch or sheet a view shows), in bytes.
const MAX_SUBJECT: usize = 256;
/// Closed views kept for Reopen Closed View.
const CLOSED_CAP: usize = 16;
/// Ids beyond this are not exact in JSON numbers: a saved set holding one is refused.
const MAX_ID: u64 = 1 << 53;

/// What a view shows of its design.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ViewKind {
    Model,
    Sketch,
    Sheet,
    Render,
    Animation,
}

impl ViewKind {
    /// Every kind, in the order the "+" menu lists them.
    pub const ALL: [ViewKind; 5] = [ViewKind::Model, ViewKind::Sketch, ViewKind::Sheet, ViewKind::Render, ViewKind::Animation];

    /// The kind's stable key: its name in commands and in the saved form.
    pub fn key(self) -> &'static str {
        match self {
            ViewKind::Model => "model",
            ViewKind::Sketch => "sketch",
            ViewKind::Sheet => "sheet",
            ViewKind::Render => "render",
            ViewKind::Animation => "animation",
        }
    }

    /// The kind with `key` ([`Self::key`]).
    pub fn from_key(key: &str) -> Option<ViewKind> {
        Self::ALL.into_iter().find(|kind| kind.key() == key)
    }

    /// The English word a new view's default name starts from.
    fn label(self) -> &'static str {
        match self {
            ViewKind::Model => "Model",
            ViewKind::Sketch => "Sketch",
            ViewKind::Sheet => "Sheet",
            ViewKind::Render => "Render",
            ViewKind::Animation => "Animation",
        }
    }
}

/// One view of a design.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesignView {
    /// Unique within its design for as long as the set lives; never reused.
    pub id: u64,
    pub kind: ViewKind,
    /// What the sub-tab says: the default ("Sketch 2") until renamed.
    pub name: String,
    /// The sketch or sheet this view shows (its id); none for Model, Render and Animation.
    pub subject: Option<String>,
}

/// The views of a design in tab order, the active one, and the views closed lately.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewSet {
    views: Vec<DesignView>,
    active: u64,
    /// Closed views with the place each had, the latest last.
    closed: Vec<(usize, DesignView)>,
    next_id: u64,
}

impl Default for ViewSet {
    fn default() -> Self {
        Self::new()
    }
}

impl ViewSet {
    /// A design's first views: one Model view, active.
    pub fn new() -> Self {
        let model = DesignView { id: 1, kind: ViewKind::Model, name: ViewKind::Model.label().into(), subject: None };
        ViewSet { views: vec![model], active: 1, closed: Vec::new(), next_id: 2 }
    }

    /// The views in tab order (never empty).
    pub fn views(&self) -> &[DesignView] {
        &self.views
    }

    /// The shown view.
    pub fn active(&self) -> &DesignView {
        self.views.iter().find(|view| view.id == self.active).unwrap_or(&self.views[0])
    }

    /// How many views are open (at least one).
    pub fn len(&self) -> usize {
        self.views.len()
    }

    /// Whether no view is open (never, a set always keeps one).
    pub fn is_empty(&self) -> bool {
        self.views.is_empty()
    }

    /// The view with `id`.
    pub fn get(&self, id: u64) -> Option<&DesignView> {
        self.views.iter().find(|view| view.id == id)
    }

    /// Where the view with `id` is in tab order.
    pub fn index_of(&self, id: u64) -> Option<usize> {
        self.views.iter().position(|view| view.id == id)
    }

    /// Whether Reopen Closed View has a view to bring back.
    pub fn can_reopen(&self) -> bool {
        !self.closed.is_empty()
    }

    /// Open a view of `kind` showing `subject` and make it the active one. With `reuse`, a view
    /// already showing the same kind and subject is activated instead. A new view is named by
    /// kind and number ("Sketch 2") and goes last. Full (`MAX_VIEWS`): the active view stays.
    /// Returns the id of the view that is active now.
    pub fn open(&mut self, kind: ViewKind, subject: Option<String>, reuse: bool) -> u64 {
        if reuse && let Some(view) = self.views.iter().find(|view| view.kind == kind && view.subject == subject) {
            self.active = view.id;
            return view.id;
        }
        if self.views.len() >= MAX_VIEWS {
            return self.active;
        }
        let id = self.take_id();
        let name = self.default_name(kind);
        self.views.push(DesignView { id, kind, name, subject });
        self.active = id;
        id
    }

    /// Make the view with `id` the active one. False when there is none.
    pub fn activate(&mut self, id: u64) -> bool {
        let found = self.get(id).is_some();
        if found {
            self.active = id;
        }
        found
    }

    /// Make the `i`th view (0 is first) the active one. False when there is none.
    pub fn activate_index(&mut self, i: usize) -> bool {
        match self.views.get(i) {
            Some(view) => {
                self.active = view.id;
                true
            }
            None => false,
        }
    }

    /// Close the view with `id`, which Reopen can bring back. The last view never closes. When
    /// the active view closes, the one to its left takes over (else the one to its right).
    pub fn close(&mut self, id: u64) -> bool {
        let Some(i) = self.index_of(id) else { return false };
        if self.views.len() < 2 {
            return false;
        }
        self.remove_closed(i);
        if self.active == id {
            self.active = self.views[i.saturating_sub(1)].id;
        }
        true
    }

    /// Close every view but the one with `id`, which becomes the active one.
    pub fn close_others(&mut self, id: u64) {
        if self.get(id).is_none() {
            return;
        }
        while let Some(i) = self.views.iter().position(|view| view.id != id) {
            self.remove_closed(i);
        }
        self.active = id;
    }

    /// Close the views after the one with `id`; when the active view was among them, `id` takes over.
    pub fn close_right_of(&mut self, id: u64) {
        let Some(i) = self.index_of(id) else { return };
        while self.views.len() > i + 1 {
            self.remove_closed(i + 1);
        }
        if self.index_of(self.active).is_none() {
            self.active = id;
        }
    }

    /// Bring back the view closed last, in its old place, and make it active. None when no view
    /// was closed (or the set is full).
    pub fn reopen(&mut self) -> Option<u64> {
        if self.views.len() >= MAX_VIEWS {
            return None;
        }
        let (at, view) = self.closed.pop()?;
        let id = view.id;
        self.views.insert(at.min(self.views.len()), view);
        self.active = id;
        Some(id)
    }

    /// Give the view with `id` a new name: trimmed, 1 to `MAX_NAME` characters, no control
    /// characters. False when the name or the view is not acceptable.
    pub fn rename(&mut self, id: u64, name: &str) -> bool {
        let Some(name) = valid_name(name) else { return false };
        match self.views.iter_mut().find(|view| view.id == id) {
            Some(view) => {
                view.name = name.to_owned();
                true
            }
            None => false,
        }
    }

    /// Copy the view with `id` into a new one right after it (same kind and subject, a new
    /// default name) and make it active. None when there is no such view or the set is full.
    pub fn duplicate(&mut self, id: u64) -> Option<u64> {
        let i = self.index_of(id)?;
        if self.views.len() >= MAX_VIEWS {
            return None;
        }
        let (kind, subject) = (self.views[i].kind, self.views[i].subject.clone());
        let copy = DesignView { id: self.take_id(), kind, name: self.default_name(kind), subject };
        self.active = copy.id;
        self.views.insert(i + 1, copy);
        Some(self.active)
    }

    /// Move the view at place `from` to place `to`, the ones between shifting over. The active
    /// view stays the same view. Places out of range change nothing.
    pub fn move_view(&mut self, from: usize, to: usize) {
        if from >= self.views.len() || to >= self.views.len() || from == to {
            return;
        }
        let view = self.views.remove(from);
        self.views.insert(to, view);
    }

    /// The set as saved with the design.
    pub fn to_json(&self) -> Value {
        let view_json = |view: &DesignView| json!({"id": view.id, "kind": view.kind.key(), "name": view.name, "subject": view.subject});
        json!({
            "active": self.active,
            "next_id": self.next_id,
            "views": self.views.iter().map(view_json).collect::<Vec<_>>(),
            "closed": self.closed.iter().map(|(at, view)| json!({"at": at, "view": view_json(view)})).collect::<Vec<_>>(),
        })
    }

    /// A set from [`Self::to_json`]. Anything else, or a set that breaks its rules (no views,
    /// too many, a repeated id, a bad name or kind), gives None; a closed view that does is
    /// dropped, and an active id that names no view falls back to the first view.
    pub fn from_json(value: &Value) -> Option<ViewSet> {
        let views = value.get("views")?.as_array()?;
        if views.is_empty() || views.len() > MAX_VIEWS {
            return None;
        }
        let views: Vec<DesignView> = views.iter().map(view_from_json).collect::<Option<_>>()?;
        let mut used: Vec<u64> = views.iter().map(|view| view.id).collect();
        used.sort_unstable();
        if used.windows(2).any(|pair| pair[0] == pair[1]) {
            return None;
        }
        let mut closed = Vec::new();
        for entry in value.get("closed").and_then(Value::as_array).into_iter().flatten() {
            let at = entry.get("at").and_then(Value::as_u64).and_then(|at| usize::try_from(at).ok());
            let view = entry.get("view").and_then(view_from_json);
            if let (Some(at), Some(view)) = (at, view)
                && !used.contains(&view.id)
            {
                used.push(view.id);
                closed.push((at.min(MAX_VIEWS), view));
            }
        }
        let skip = closed.len().saturating_sub(CLOSED_CAP);
        closed.drain(..skip);
        let highest = used.iter().copied().max().unwrap_or(0);
        let next_id = value.get("next_id").and_then(Value::as_u64).unwrap_or(0).clamp(highest + 1, MAX_ID + 1);
        let active = value.get("active").and_then(Value::as_u64).filter(|id| views.iter().any(|view| view.id == *id)).unwrap_or(views[0].id);
        Some(ViewSet { views, active, closed, next_id })
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// "Sketch 1", "Sketch 2"…: the first name of the kind no open view has. The first Model
    /// and Render views go without a number.
    fn default_name(&self, kind: ViewKind) -> String {
        (1..)
            .map(
                |n| {
                    if n == 1 && matches!(kind, ViewKind::Model | ViewKind::Render) {
                        kind.label().to_owned()
                    } else {
                        format!("{} {n}", kind.label())
                    }
                },
            )
            .find(|name| self.views.iter().all(|view| view.name != *name))
            .unwrap_or_default()
    }

    /// Take the view at `i` out of the set onto the closed stack.
    fn remove_closed(&mut self, i: usize) {
        let view = self.views.remove(i);
        self.closed.push((i, view));
        let skip = self.closed.len().saturating_sub(CLOSED_CAP);
        self.closed.drain(..skip);
    }
}

/// `name` as a view name, or None when it is empty, too long or holds control characters.
fn valid_name(name: &str) -> Option<&str> {
    let name = name.trim();
    (!name.is_empty() && name.chars().count() <= MAX_NAME && !name.chars().any(char::is_control)).then_some(name)
}

fn view_from_json(value: &Value) -> Option<DesignView> {
    let id = value.get("id")?.as_u64().filter(|id| (1..=MAX_ID).contains(id))?;
    let kind = ViewKind::from_key(value.get("kind")?.as_str()?)?;
    let name = valid_name(value.get("name")?.as_str()?)?.to_owned();
    let subject = match value.get("subject") {
        None | Some(Value::Null) => None,
        Some(Value::String(subject)) if subject.len() <= MAX_SUBJECT => Some(subject.clone()),
        Some(_) => return None,
    };
    Some(DesignView { id, kind, name, subject })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(set: &ViewSet) -> Vec<&str> {
        set.views().iter().map(|view| view.name.as_str()).collect()
    }

    fn sketches(n: usize) -> ViewSet {
        let mut set = ViewSet::new();
        for i in 0..n {
            set.open(ViewKind::Sketch, Some(format!("s{i}")), false);
        }
        set
    }

    #[test]
    fn a_new_set_is_one_active_model_view() {
        let set = ViewSet::new();
        assert_eq!((set.len(), set.active().kind, set.active().name.as_str()), (1, ViewKind::Model, "Model"));
        assert_eq!(ViewSet::default(), set);
    }

    #[test]
    fn open_numbers_names_per_kind_and_activates() {
        let mut set = ViewSet::new();
        let a = set.open(ViewKind::Sketch, Some("a".into()), false);
        let b = set.open(ViewKind::Sketch, Some("b".into()), false);
        let r = set.open(ViewKind::Render, None, false);
        let m = set.open(ViewKind::Model, None, false);
        assert_eq!(names(&set), ["Model", "Sketch 1", "Sketch 2", "Render", "Model 2"]);
        assert_eq!(set.active().id, m);
        assert!(a != b && b != r && r != m);
        set.close(a);
        assert_eq!(set.open(ViewKind::Sheet, None, false), set.active().id);
        assert_eq!(set.open(ViewKind::Sketch, Some("c".into()), false), set.active().id);
        assert_eq!(set.active().name, "Sketch 1", "a freed name is used again");
    }

    #[test]
    fn open_reuses_a_view_of_the_same_kind_and_subject_only_when_asked() {
        let mut set = ViewSet::new();
        let a = set.open(ViewKind::Sketch, Some("a".into()), true);
        set.open(ViewKind::Render, None, true);
        assert_eq!(set.open(ViewKind::Sketch, Some("a".into()), true), a);
        assert_eq!((set.len(), set.active().id), (3, a));
        assert_ne!(set.open(ViewKind::Sketch, Some("a".into()), false), a);
        assert_ne!(set.open(ViewKind::Sketch, Some("b".into()), true), a);
        assert_eq!(set.len(), 5);
        assert_eq!(set.open(ViewKind::Model, None, true), set.views()[0].id, "the first Model view is reused");
    }

    #[test]
    fn open_stops_at_the_most_views() {
        let mut set = sketches(MAX_VIEWS - 1);
        assert_eq!(set.len(), MAX_VIEWS);
        let active = set.active().id;
        assert_eq!(set.open(ViewKind::Render, None, false), active);
        assert_eq!(set.duplicate(active), None);
        assert_eq!(set.len(), MAX_VIEWS);
    }

    #[test]
    fn activate_by_id_and_by_index() {
        let mut set = sketches(2);
        let first = set.views()[0].id;
        assert!(set.activate(first) && set.active().id == first);
        assert!(set.activate_index(2) && set.active().id == set.views()[2].id);
        assert!(!set.activate(999) && !set.activate_index(3));
        assert_eq!(set.active().id, set.views()[2].id, "a refused activation changes nothing");
    }

    #[test]
    fn close_never_closes_the_last_view_and_moves_to_a_neighbour() {
        let mut set = ViewSet::new();
        let only = set.active().id;
        assert!(!set.close(only) && set.len() == 1);
        let a = set.open(ViewKind::Sketch, Some("a".into()), false);
        let b = set.open(ViewKind::Sketch, Some("b".into()), false);
        assert!(set.close(b));
        assert_eq!(set.active().id, a, "the left neighbour takes over");
        assert!(set.activate(only) && set.close(only));
        assert_eq!(set.active().id, a, "the first view closing hands over to the right");
        assert!(!set.close(only) && !set.close(12345), "unknown ids close nothing");
        assert!(!set.close(a), "the last one stays");
    }

    #[test]
    fn closing_an_inactive_view_keeps_the_active_one() {
        let mut set = sketches(2);
        let (first, last) = (set.views()[0].id, set.views()[2].id);
        assert!(set.close(first));
        assert_eq!(set.active().id, last);
    }

    #[test]
    fn close_others_and_close_right_of() {
        let mut set = sketches(4);
        let ids: Vec<u64> = set.views().iter().map(|view| view.id).collect();
        set.close_right_of(ids[2]);
        assert_eq!(set.views().iter().map(|view| view.id).collect::<Vec<_>>(), &ids[..3], "the active one went: the pivot takes over");
        assert_eq!(set.active().id, ids[2]);
        assert!(set.activate(ids[0]));
        set.close_right_of(ids[1]);
        assert_eq!((set.len(), set.active().id), (2, ids[0]), "an active view that stays stays");
        set.close_right_of(999);
        assert_eq!(set.len(), 2);
        set.close_others(ids[1]);
        assert_eq!((set.len(), set.active().id), (1, ids[1]));
        set.close_others(999);
        assert_eq!(set.len(), 1);
        assert_eq!(set.reopen(), Some(ids[0]), "everything closed can come back, latest first");
    }

    #[test]
    fn reopen_restores_the_latest_closed_view_in_its_place() {
        let mut set = sketches(3);
        let ids: Vec<u64> = set.views().iter().map(|view| view.id).collect();
        assert!(!set.can_reopen());
        set.close(ids[1]);
        set.close(ids[3]);
        assert!(set.can_reopen());
        assert_eq!(set.reopen(), Some(ids[3]));
        assert_eq!(set.active().id, ids[3]);
        assert_eq!(set.reopen(), Some(ids[1]));
        assert_eq!(set.views().iter().map(|view| view.id).collect::<Vec<_>>(), ids);
        assert_eq!(set.reopen(), None);
    }

    #[test]
    fn the_closed_stack_keeps_the_latest_sixteen() {
        let mut set = sketches(20);
        let ids: Vec<u64> = set.views().iter().map(|view| view.id).collect();
        for id in &ids[1..] {
            assert!(set.close(*id));
        }
        let mut back = 0;
        while set.reopen().is_some() {
            back += 1;
        }
        assert_eq!((back, set.len()), (CLOSED_CAP, CLOSED_CAP + 1));
        assert!(set.get(ids[1]).is_none() && set.get(ids[20]).is_some(), "the oldest closes were forgotten");
    }

    #[test]
    fn rename_trims_and_limits() {
        let mut set = ViewSet::new();
        let id = set.active().id;
        assert!(set.rename(id, "  Front  "));
        assert_eq!(set.active().name, "Front");
        assert!(set.rename(id, &"é".repeat(MAX_NAME)));
        let long = "x".repeat(MAX_NAME + 1);
        for bad in ["", "   ", "a\nb", "tab\there", long.as_str()] {
            assert!(!set.rename(id, bad), "{bad:?}");
        }
        assert_eq!(set.active().name, "é".repeat(MAX_NAME));
        assert!(!set.rename(999, "Name"));
    }

    #[test]
    fn duplicate_goes_next_to_the_original_with_a_new_name() {
        let mut set = sketches(1);
        let (model, sketch) = (set.views()[0].id, set.views()[1].id);
        let copy = set.duplicate(sketch).unwrap();
        assert_eq!(set.active().id, copy);
        assert_eq!(names(&set), ["Model", "Sketch 1", "Sketch 2"]);
        assert_eq!(set.views()[2].subject, set.views()[1].subject);
        let model_copy = set.duplicate(model).unwrap();
        assert_eq!(set.index_of(model_copy), Some(1));
        assert_eq!(set.get(model_copy).map(|view| view.name.as_str()), Some("Model 2"));
        assert_eq!(set.duplicate(999), None);
    }

    #[test]
    fn move_view_reorders_and_keeps_the_active_view() {
        let mut set = sketches(3);
        let ids: Vec<u64> = set.views().iter().map(|view| view.id).collect();
        set.activate(ids[1]);
        set.move_view(0, 3);
        assert_eq!(set.views().iter().map(|view| view.id).collect::<Vec<_>>(), [ids[1], ids[2], ids[3], ids[0]]);
        assert_eq!(set.active().id, ids[1]);
        set.move_view(3, 0);
        assert_eq!(set.views().iter().map(|view| view.id).collect::<Vec<_>>(), ids);
        for (from, to) in [(0, 4), (4, 0), (9, 9), (2, 2)] {
            set.move_view(from, to);
        }
        assert_eq!(set.views().iter().map(|view| view.id).collect::<Vec<_>>(), ids);
    }

    #[test]
    fn json_round_trips_views_active_closed_and_ids() {
        let mut set = sketches(3);
        set.open(ViewKind::Render, None, false);
        let ids: Vec<u64> = set.views().iter().map(|view| view.id).collect();
        set.rename(ids[2], "Profile");
        set.close(ids[1]);
        set.activate(ids[3]);
        let back = ViewSet::from_json(&set.to_json()).unwrap();
        assert_eq!(back, set);
        let mut back = back;
        assert_eq!(back.reopen(), Some(ids[1]));
        assert!(back.open(ViewKind::Sheet, None, false) > ids[4], "ids stay unique after a reload");
    }

    #[test]
    fn hostile_json_is_refused_or_repaired_never_a_panic() {
        let view = |id: Value, kind: &str, name: Value| json!({"id": id, "kind": kind, "name": name});
        for bad in [
            Value::Null,
            json!(5),
            json!([]),
            json!({}),
            json!({"views": []}),
            json!({"views": "x"}),
            json!({"views": [view(json!(1), "bogus", json!("A"))]}),
            json!({"views": [view(json!(0), "model", json!("A"))]}),
            json!({"views": [view(json!(-1), "model", json!("A"))]}),
            json!({"views": [view(json!(1.5), "model", json!("A"))]}),
            json!({"views": [view(json!(u64::MAX), "model", json!("A"))]}),
            json!({"views": [view(json!(1), "model", json!(""))]}),
            json!({"views": [view(json!(1), "model", json!(7))]}),
            json!({"views": [view(json!(1), "model", json!("x".repeat(65)))]}),
            json!({"views": [view(json!(1), "model", json!("A")), view(json!(1), "sketch", json!("B"))]}),
            json!({"views": [{"id": 1, "kind": "model", "name": "A", "subject": 3}]}),
            json!({"views": [{"id": 1, "kind": "model", "name": "A", "subject": "s".repeat(257)}]}),
            json!({"views": vec![view(json!(1), "model", json!("A")); MAX_VIEWS + 1]}),
        ] {
            assert!(ViewSet::from_json(&bad).is_none(), "{bad}");
        }
        let repaired = ViewSet::from_json(&json!({
            "active": "nope", "next_id": 0,
            "views": [view(json!(5), "model", json!("A")), view(json!(9), "sketch", json!(" B "))],
            "closed": [
                {"at": u64::MAX, "view": view(json!(5), "model", json!("dup"))},
                {"at": 1, "view": view(json!(2), "bogus", json!("bad"))},
                {"at": "x", "view": view(json!(3), "render", json!("R"))},
                {"at": 7, "view": view(json!(3), "render", json!("R"))},
                17,
            ],
        }))
        .unwrap();
        assert_eq!((repaired.active().id, names(&repaired)), (5, vec!["A", "B"]));
        let mut repaired = repaired;
        assert_eq!(repaired.reopen(), Some(3), "only the sound closed view is kept");
        assert_eq!(repaired.index_of(3), Some(2));
        assert_eq!(repaired.reopen(), None);
        assert!(repaired.open(ViewKind::Sheet, None, false) > 9);
    }
}
