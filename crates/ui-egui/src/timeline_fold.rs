//! A long History bar folds each long run of one component's features into one chip with the
//! component's name and the number of features: a 2000-part assembly shows one chip for its top
//! component, which opens to a chip per subassembly, and so on down to the features. Clicking a
//! chip shows its features one by one, and clicking it again folds them (the choice belongs to the
//! design, `history_folds`). Until then, a run that holds the history marker or the component being
//! edited or picked is shown open.
//!
//! An open run or folder shows its items inside a tray drawn from its chip to its last item,
//! nested trays inside it. A run that holds runs which fold in turn (a subassembly's) is deep:
//! clicking it shows only its component's features instead (`timeline_scope`), so a 2000-part
//! assembly never fills the bar with every subassembly's chip at once.
//!
//! While the bar shows one component's features (`timeline_scope`), the fold reads only those,
//! from the components inside it down; the entries count the shown features (`View`).
//!
//! Feature folders (`timeline_folders`) come first: each folder is its own chip whatever the
//! history's length, and its features never join a run outside it. The component fold then
//! applies to the features outside folders and to those of an open folder, between the folders
//! in it, as it does to the whole history.
use std::collections::{HashMap, HashSet};

use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::Component;

use crate::SolveApp;
use crate::theme::TabAccent;
use crate::timeline_folders::Span;
use crate::ui_kit::HistoryGroupChip;

/// A history with no more features than this never folds: it still fits a window's width.
pub(crate) const FOLD_FROM: usize = 50;
/// A run of more features than this, all inside one component, folds into one chip.
pub(crate) const FOLD_MIN: usize = 6;
/// Folds go no deeper than this many nested components; deeper runs show their features.
const FOLD_DEPTH_MAX: usize = 16;
/// The browser-state key prefix that remembers a run was opened or folded.
pub(crate) const KEY: &str = "timeline_component:";

/// A run of features in one component, shown as one chip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Group {
    pub component: u64,
    /// The run's features, by their index in the history.
    pub start: usize,
    pub end: usize,
    /// How many open chips (runs and folders) hold this one.
    pub depth: usize,
    pub open: bool,
    /// It holds runs that fold in turn: a click shows only its component's features
    /// (`timeline_scope`).
    pub deep: bool,
}

/// A feature folder, shown as one chip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Folder {
    pub id: u64,
    /// Its features and its folders', by their index in the history; an empty folder's start
    /// and end are where it sits.
    pub start: usize,
    pub end: usize,
    /// How many open chips hold this one.
    pub depth: usize,
    pub open: bool,
}

/// What the History bar draws, in order: features, folded runs and folders, the joints of an
/// open folder, and the end of each open run or folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Entry {
    Feature(usize),
    Group(Group),
    Folder(Folder),
    /// The joints an open folder holds (an import's rigid group).
    Joints(u64),
    /// What the open run or folder at this depth holds came before.
    End(usize),
}

/// What a feature's chip would show that its run's chip shows while folded.
#[derive(Clone, Copy, Default)]
pub(crate) struct Marks {
    pub selected: bool,
    pub hovered: bool,
    pub error: bool,
    pub warning: bool,
}

/// The History bar's entries for one frame, with what each feature's chip would show. Entries,
/// runs and folders count the features the bar shows (`view`), not the design's.
pub(crate) struct History {
    pub entries: Vec<Entry>,
    /// By the bar's feature; empty when nothing folds.
    marks: Vec<Marks>,
    pub folders: crate::timeline_folders::Folders,
    pub view: View,
    /// The component whose features the bar shows (`timeline_scope`); none for the whole design.
    pub within: Option<u64>,
}

/// Which of the design's features the History bar shows: all of them, or those of one component
/// and the components inside it, without the components marked out of the bar (#10393). A slot
/// among the shown features maps back to a position in the design: just before the shown
/// feature there, or just after the last one shown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct View {
    /// The shown features' indices in the design, in order; none when all are shown.
    shown: Option<Vec<usize>>,
    /// How many features the design has.
    n: usize,
}

impl View {
    /// The features of the components in `within`, or all of them, leaving out those of the
    /// components marked out of the History bar unless `show_hidden` asks for them (#10393).
    pub(crate) fn of(doc: &solidcraft_engine::doc::Document, within: Option<&HashSet<u64>>, show_hidden: bool) -> Self {
        let hides = !show_hidden && doc.components.iter().any(|c| c.history_hidden);
        let shown = (within.is_some() || hides).then(|| {
            doc.features
                .iter()
                .enumerate()
                .filter(|(_, f)| within.is_none_or(|w| w.contains(&f.component)))
                .filter(|(_, f)| !hides || !doc.component_history_hidden(f.component))
                .map(|(i, _)| i)
                .collect()
        });
        Self { shown, n: doc.features.len() }
    }

    /// How many features the bar shows.
    pub(crate) fn len(&self) -> usize {
        self.shown.as_ref().map_or(self.n, Vec::len)
    }

    /// The design's position of the bar's slot `k`, the one before its feature `k`; past its last
    /// feature, the position just after it.
    pub(crate) fn index(&self, k: usize) -> usize {
        match &self.shown {
            None => k.min(self.n),
            Some(shown) => shown.get(k).copied().unwrap_or_else(|| shown.last().map_or(self.n, |i| i + 1)),
        }
    }

    /// The bar's slot of the design's position `at`: before the first shown feature at or after it.
    pub(crate) fn slot(&self, at: usize) -> usize {
        match &self.shown {
            None => at.min(self.n),
            Some(shown) => shown.partition_point(|&i| i < at),
        }
    }

    /// Where the bar shows the design's feature `at`, if it does.
    pub(crate) fn find(&self, at: usize) -> Option<usize> {
        match &self.shown {
            None => (at < self.n).then_some(at),
            Some(shown) => shown.binary_search(&at).ok(),
        }
    }

    /// The shown features' indices in the design, ascending; none when the bar shows them all.
    pub(crate) fn shown(&self) -> Option<&[usize]> {
        self.shown.as_deref()
    }

    /// The design's indices of the bar's features `start..end`.
    pub(crate) fn indices(&self, start: usize, end: usize) -> Vec<usize> {
        (start..end.min(self.len())).map(|k| self.index(k)).collect()
    }

    /// Folders as the bar counts their features; a folder holding none of the shown features is
    /// left out, unless it or a folder in it holds joints: joints show whichever component the
    /// bar shows, so such a folder stays, empty, where its features would be.
    fn spans(&self, spans: &[Span]) -> Vec<Span> {
        if self.shown.is_none() {
            return spans.to_vec();
        }
        spans
            .iter()
            .filter_map(|s| {
                let (start, end) = (self.slot(s.start), self.slot(s.end));
                let children = self.spans(&s.children);
                (start < end || s.joints > 0 || !children.is_empty()).then_some(Span { id: s.id, start, end, joints: s.joints, children })
            })
            .collect()
    }
}

/// Where the History bar is drawn and what is under the pointer.
pub(crate) struct Strip {
    /// The history marker's position.
    pub marker: usize,
    /// The features that made what is under the pointer and what is picked in the viewport.
    pub from_view: [Option<u64>; 2],
    /// The floating History card, which also accents the last feature before the marker.
    pub floating: bool,
    pub accent: TabAccent,
}

/// Each component's chain of parents below the root, outermost first, ending with itself and
/// cut after the outermost `FOLD_DEPTH_MAX` (no fold reads deeper). A parent that isn't there or
/// a loop of parents (a damaged design) ends the chain. Each component is walked once.
fn chains(components: &[Component]) -> HashMap<u64, Vec<u64>> {
    let parent: HashMap<u64, u64> = components.iter().map(|c| (c.id, c.parent)).collect();
    let mut done: HashMap<u64, Vec<u64>> = HashMap::with_capacity(components.len());
    for component in components {
        let mut path = Vec::new();
        let mut seen = HashSet::new();
        let mut at = component.id;
        while at != 0 && !done.contains_key(&at) && seen.insert(at) {
            path.push(at);
            at = parent.get(&at).copied().unwrap_or(0);
        }
        let mut chain = done.get(&at).cloned().unwrap_or_default();
        for id in path.into_iter().rev() {
            if chain.len() < FOLD_DEPTH_MAX {
                chain.push(id);
            }
            done.insert(id, chain.clone());
        }
    }
    done
}

/// Says whether a fold is shown open: a run of `component`'s features (`start..end`), deep or
/// not, or a folder.
pub(crate) trait Opens {
    fn run(&mut self, component: u64, start: usize, end: usize, deep: bool) -> bool;
    fn folder(&mut self, id: u64) -> bool;
}

/// The entries for `n` features in `folders`, whose components have these chains (`chains[i]`
/// for feature `i`; none when the component fold is off).
pub(crate) fn entries(n: usize, chains: &[&[u64]], folders: &[Span], open: &mut dyn Opens) -> Vec<Entry> {
    let mut out = Vec::with_capacity(n);
    let fold = Fold { chains: if n > FOLD_FROM { chains } else { &[] } };
    fold.folders(0, n, folders, 0, open, &mut out);
    out
}

/// The component chains the component fold reads (empty: it is off).
struct Fold<'a> {
    chains: &'a [&'a [u64]],
}

impl Fold<'_> {
    /// Features `start..end`, holding `folders` (in order), at `depth` open chips deep.
    fn folders(&self, start: usize, end: usize, folders: &[Span], depth: usize, open: &mut dyn Opens, out: &mut Vec<Entry>) {
        let mut i = start;
        for span in folders {
            // A damaged design's folders may overlap: each feature still shows once, in order.
            let from = span.start.max(i).min(end);
            let to = span.end.max(from).min(end);
            self.runs(i, from, 0, depth, open, out);
            let shown = open.folder(span.id);
            out.push(Entry::Folder(Folder { id: span.id, start: from, end: to, depth, open: shown }));
            if shown {
                self.folders(from, to, &span.children, depth + 1, open, out);
                if span.joints > 0 {
                    out.push(Entry::Joints(span.id));
                }
                out.push(Entry::End(depth));
            }
            i = to;
        }
        self.runs(i, end, 0, depth, open, out);
    }

    /// Feature `i`'s component at chain `level`: none at the root or past the deepest level.
    fn at(&self, i: usize, level: usize) -> Option<u64> {
        if level < FOLD_DEPTH_MAX { self.chains.get(i).and_then(|c| c.get(level)).copied() } else { None }
    }

    /// The end of the run of one component at chain `level` that starts at feature `start`.
    fn run_end(&self, start: usize, end: usize, level: usize) -> usize {
        let component = self.at(start, level);
        (start + 1..end).find(|&j| self.at(j, level) != component).unwrap_or(end)
    }

    /// Whether features `start..end` hold a run that folds at chain `level`.
    fn folds(&self, start: usize, end: usize, level: usize) -> bool {
        let mut i = start;
        while i < end {
            let j = self.run_end(i, end, level);
            if self.at(i, level).is_some() && j - i > FOLD_MIN {
                return true;
            }
            i = j;
        }
        false
    }

    /// Features `start..end`, folding long runs of one component at chain `level`.
    fn runs(&self, start: usize, end: usize, level: usize, depth: usize, open: &mut dyn Opens, out: &mut Vec<Entry>) {
        let mut i = start;
        while i < end {
            let j = self.run_end(i, end, level);
            match self.at(i, level) {
                Some(component) if j - i > FOLD_MIN => {
                    let deep = self.folds(i, j, level + 1);
                    let shown = open.run(component, i, j, deep);
                    out.push(Entry::Group(Group { component, start: i, end: j, depth, open: shown, deep }));
                    if shown {
                        self.runs(i, j, level + 1, depth + 1, open, out);
                        out.push(Entry::End(depth));
                    }
                }
                _ => out.extend((i..j).map(Entry::Feature)),
            }
            i = j;
        }
    }
}

/// The key that remembers a run's choice: its component and its first feature (`first`, by its
/// index in the design), so two runs of one component apart in the history fold on their own.
fn key(app: &SolveApp, component: u64, first: usize) -> String {
    let first = app.session.doc.features.get(first).map_or(0, |f| f.id);
    format!("{KEY}{component}:{first}")
}

/// Whether a run is shown one by one: as last clicked, else `unless_clicked`.
fn is_open(app: &SolveApp, key: &str, unless_clicked: bool) -> bool {
    crate::history_folds::is_open(app, key, unless_clicked)
}

fn set_open(app: &mut SolveApp, key: &str, open: bool) {
    crate::history_folds::set_open(app, key, open);
}

/// What the History bar shows of the design's features this frame: the whole history, or the
/// features of one component and those inside it (`timeline_scope`).
pub(crate) fn history(app: &SolveApp, strip: &Strip) -> History {
    let doc = &app.session.doc;
    let within = crate::timeline_scope::shown(app);
    let view = View::of(doc, within.map(|c| crate::timeline_scope::components_within(app, c)).as_ref(), app.tree.history_show_hidden);
    let n = view.len();
    // Off, the bar shows each feature and starts folders open, as it did before (Preferences >
    // Layout, "Fold the History bar into groups").
    let folds = n > FOLD_FROM && app.preferences.history_bar_groups;
    if !folds && doc.timeline_groups.is_empty() {
        return History { entries: (0..n).map(Entry::Feature).collect(), marks: Vec::new(), folders: Default::default(), view, within };
    }
    let folders = crate::timeline_folders::Folders::of(doc);
    let spans = view.spans(&folders.spans);
    let chains = chains(&doc.components);
    let of = |c: u64| chains.get(&c).map_or(&[][..], Vec::as_slice);
    // Inside a component, runs fold by the components inside it.
    let level = within.map_or(0, |c| of(c).len());
    let per_feature: Vec<&[u64]> = if folds {
        let chain = |i: usize| doc.features.get(i).and_then(|f| of(f.component).get(level..)).unwrap_or_default();
        view.indices(0, n).into_iter().map(chain).collect()
    } else {
        Vec::new()
    };
    let focus = crate::timeline_scope::focus(app);
    let mut on_the_way = HashSet::new();
    // The way to the edited or picked component opens, while the bar shows more than it.
    if let Some(focus) = focus.filter(|f| within != Some(*f)) {
        let mine = crate::timeline_scope::components_within(app, focus);
        if let Some(last) = doc.features.iter().rposition(|f| mine.contains(&f.component)).and_then(|i| view.find(i)) {
            folders_holding(&spans, last, &mut on_the_way);
        }
    }
    let mut open = Shown { app, view: &view, marker: view.slot(strip.marker), focus: focus.map_or(&[][..], of), folders: on_the_way };
    let entries = entries(n, &per_feature, &spans, &mut open);
    History { entries, marks: marks(app, strip, &view), folders, view, within }
}

/// The folders whose features hold feature `at`, at any depth.
fn folders_holding(spans: &[Span], at: usize, out: &mut HashSet<u64>) {
    for span in spans.iter().filter(|s| s.start <= at && at < s.end) {
        out.insert(span.id);
        folders_holding(&span.children, at, out);
    }
}

/// Which folds the History bar shows open: the runs on the way to the component being edited or
/// picked, and the folders holding its last feature; the rest as last clicked, and until then a
/// run that holds the marker. A deep run elsewhere stays folded: a click on it shows only its
/// component's features.
struct Shown<'a> {
    app: &'a SolveApp,
    view: &'a View,
    /// The marker's slot among the shown features.
    marker: usize,
    /// The edited or picked component and those around it.
    focus: &'a [u64],
    /// The folders on the way to it.
    folders: HashSet<u64>,
}

impl Opens for Shown<'_> {
    fn run(&mut self, component: u64, start: usize, end: usize, deep: bool) -> bool {
        let on_the_way = self.focus.contains(&component);
        if deep {
            return on_the_way;
        }
        let unless_clicked = (start < self.marker && self.marker < end) || on_the_way;
        is_open(self.app, &key(self.app, component, self.view.index(start)), unless_clicked)
    }

    fn folder(&mut self, id: u64) -> bool {
        let unless_clicked = self.folders.contains(&id) || !self.app.preferences.history_bar_groups;
        crate::timeline_folders::is_open(self.app, id, unless_clicked)
    }
}

/// Each feature's marks, reading the results and the selection once.
fn marks(app: &SolveApp, strip: &Strip, view: &View) -> Vec<Marks> {
    let flags: HashMap<u64, (bool, bool)> = app.session.model.results.iter().map(|r| (r.id, (r.error.is_some(), r.warning.is_some()))).collect();
    let in_dialog = app.dialog.as_ref().map(|d| d.items()).unwrap_or_default();
    let picked: HashSet<u64> = app
        .session
        .selection
        .iter()
        .chain(in_dialog.iter())
        .filter_map(|s| match s {
            Sel::Feature { id } => Some(*id),
            _ => None,
        })
        .collect();
    let last_done = strip.marker.checked_sub(1).filter(|_| strip.floating);
    let features = &app.session.doc.features;
    view.indices(0, view.len())
        .into_iter()
        .filter_map(|i| features.get(i).map(|f| (i, f)))
        .map(|(i, f)| {
            let (error, warning) = flags.get(&f.id).copied().unwrap_or_default();
            Marks { selected: picked.contains(&f.id) || last_done == Some(i), hovered: strip.from_view.contains(&Some(f.id)), error, warning }
        })
        .collect()
}

impl History {
    /// The marks a chip for features `start..end` shows: any of its features' (selected only
    /// while it is folded).
    pub(crate) fn inside(&self, start: usize, end: usize, open: bool) -> Marks {
        let marks = self.marks.get(start..end).unwrap_or_default();
        Marks {
            selected: !open && marks.iter().any(|m| m.selected),
            hovered: marks.iter().any(|m| m.hovered),
            error: marks.iter().any(|m| m.error),
            warning: marks.iter().any(|m| m.warning),
        }
    }
}

/// The name a run's chip shows: its component's.
pub(crate) fn name(app: &SolveApp, component: u64) -> String {
    app.session.doc.components.iter().find(|c| c.id == component).map_or_else(|| format!("Component {component}"), |c| c.name.clone())
}

/// Draws a run's chip with its top left at `at`; a click shows its features one by one or folds
/// them again (a deep run shows only its component's features), and its menu also selects them
/// or rolls the marker after them. Returns its width.
pub(crate) fn group_chip(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    p: &egui::Painter,
    at: egui::Pos2,
    group: &Group,
    history: &History,
    strip: &Strip,
) -> f32 {
    let name = name(app, group.component);
    let state = history.inside(group.start, group.end, group.open);
    let chip = HistoryGroupChip {
        icon: "component",
        name: &name,
        count: group.end - group.start,
        open: group.open,
        selected: state.selected,
        hovered: state.hovered,
        dim: history.view.index(group.start) >= strip.marker,
        error: state.error,
        warning: state.warning,
        accent: strip.accent,
    };
    let rect = egui::Rect::from_min_size(at, chip.size(p));
    let response = ui.interact(rect, ui.id().with(("component_run", group.component, group.start)), egui::Sense::click());
    crate::scenario::publish_handle(&format!("timeline-group:{name}"), rect.center());
    chip.paint(p, rect, response.hover_pos());
    let tip = match (group.deep, group.open) {
        (true, _) => "click to show only its features",
        (false, true) => "click to fold them",
        (false, false) => "click to show each one",
    };
    let response =
        response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(format!("{name}: {} features, {tip}", group.end - group.start));
    if response.clicked() {
        fold(app, group, &history.view);
    }
    crate::ui_kit::Menu::context(&response, |ui| group_menu(app, ui, group, &history.view));
    rect.width()
}

/// A click on a run's chip: opens or folds it; a deep one shows only its component's features.
fn fold(app: &mut SolveApp, group: &Group, view: &View) {
    if group.deep {
        crate::timeline_scope::enter(app, group.component);
    } else {
        let key = key(app, group.component, view.index(group.start));
        set_open(app, &key, !group.open);
    }
}

fn group_menu(app: &mut SolveApp, ui: &mut egui::Ui, group: &Group, view: &View) {
    let fold_label = if group.open { "Fold Features" } else { "Show Each Feature" };
    if crate::context_menu::button(ui, fold_label, "list").clicked() {
        fold(app, group, view);
        ui.close();
    }
    if crate::context_menu::button(ui, "Select Features", "select").clicked() {
        let features = &app.session.doc.features;
        let items: Vec<_> = view
            .indices(group.start, group.end)
            .into_iter()
            .filter_map(|i| features.get(i))
            .map(|f| json!({"type": "feature", "id": f.id}))
            .collect();
        app.tree.picked_joints.clear();
        app.tree.picked_timeline_groups.clear();
        let _ = app.run("select.set", json!({ "items": items }));
        ui.close();
    }
    if crate::context_menu::button(ui, crate::i18n::tr_label("Roll History Marker Here"), "rollback").clicked() {
        let end = view.index(group.end);
        let position = if end >= app.session.doc.features.len() { json!({}) } else { json!({ "position": end }) };
        let _ = app.run_heavy("timeline.roll_to", position);
        ui.close();
    }
}

#[cfg(test)]
mod tests {
    use solidcraft_engine::doc::Component;

    use super::{Entry, FOLD_DEPTH_MAX, FOLD_FROM, FOLD_MIN, Folder, Group, Opens, chains, entries};
    use crate::timeline_folders::Span;

    /// Opens the runs of these components and these folders.
    struct Open<'a>(&'a [u64]);

    impl Opens for Open<'_> {
        fn run(&mut self, component: u64, _: usize, _: usize, _: bool) -> bool {
            self.0.contains(&component)
        }

        fn folder(&mut self, id: u64) -> bool {
            self.0.contains(&id)
        }
    }

    fn run(chains: &[&[u64]], open: &[u64]) -> Vec<Entry> {
        entries(chains.len(), chains, &[], &mut Open(open))
    }

    fn folder(id: u64, start: usize, end: usize, depth: usize, open: bool) -> Entry {
        Entry::Folder(Folder { id, start, end, depth, open })
    }

    fn span(id: u64, start: usize, end: usize, joints: usize, children: Vec<Span>) -> Span {
        Span { id, start, end, joints, children }
    }

    fn group(component: u64, start: usize, end: usize, depth: usize, open: bool) -> Entry {
        Entry::Group(Group { component, start, end, depth, open, deep: false })
    }

    /// A run that holds runs folding in turn.
    fn deep(component: u64, start: usize, end: usize, depth: usize, open: bool) -> Entry {
        Entry::Group(Group { component, start, end, depth, open, deep: true })
    }

    fn component(id: u64, parent: u64) -> Component {
        Component { id, name: format!("C{id}"), parent, ..Component::default() }
    }

    /// A short history shows every feature, whatever their components.
    #[test]
    fn a_short_history_never_folds() {
        let chains: Vec<&[u64]> = vec![&[1u64][..]; FOLD_FROM];
        assert_eq!(run(&chains, &[]), (0..FOLD_FROM).map(Entry::Feature).collect::<Vec<_>>());
    }

    /// Root features stay one by one; a long run inside a component folds; a short run inside
    /// one doesn't; opening a run folds its subassemblies' runs in turn.
    #[test]
    fn long_runs_fold_by_component_and_open_level_by_level() {
        let mut chains: Vec<&[u64]> = vec![&[]; 2];
        chains.extend(vec![&[1u64, 2][..]; 30]);
        chains.extend(vec![&[1u64, 3, 4][..]; 30]);
        chains.push(&[1]);
        chains.extend(vec![&[5u64][..]; FOLD_MIN]);
        chains.push(&[]);
        let n = chains.len();
        let closed = run(&chains, &[]);
        assert_eq!(closed[..2], [Entry::Feature(0), Entry::Feature(1)]);
        assert_eq!(closed[2], deep(1, 2, 63, 0, false), "its subassemblies' runs fold too");
        assert_eq!(closed[3..], (63..n).map(Entry::Feature).collect::<Vec<_>>()[..], "a run of {FOLD_MIN} shows each feature");

        let open = run(&chains, &[1, 3]);
        let mut expected = vec![Entry::Feature(0), Entry::Feature(1), deep(1, 2, 63, 0, true), group(2, 2, 32, 1, false), deep(3, 32, 62, 1, true)];
        expected.push(group(4, 32, 62, 2, false));
        expected.push(Entry::End(1));
        expected.push(Entry::Feature(62));
        expected.push(Entry::End(0));
        expected.extend((63..n).map(Entry::Feature));
        assert_eq!(open, expected);
    }

    /// Every feature appears once, in order, however deep the components nest.
    #[test]
    fn deep_nesting_keeps_every_feature_once_in_order() {
        let deep: Vec<u64> = (1..=40).collect();
        let chains: Vec<&[u64]> = vec![&deep[..]; FOLD_FROM + 10];
        let shown = run(&chains, &deep);
        let features: Vec<usize> = shown.iter().filter_map(|e| if let Entry::Feature(i) = e { Some(*i) } else { None }).collect();
        assert_eq!(features, (0..chains.len()).collect::<Vec<_>>());
        let groups = shown.iter().filter(|e| matches!(e, Entry::Group(_))).count();
        let ends = shown.iter().filter(|e| matches!(e, Entry::End(_))).count();
        assert_eq!(groups, FOLD_DEPTH_MAX, "folds stop at the deepest allowed level");
        assert_eq!(ends, groups);
    }

    /// Chains run from the outermost component down; a damaged design's loops, missing parents
    /// and very deep nesting end them without hanging.
    #[test]
    fn chains_survive_loops_missing_parents_and_deep_nesting() {
        let mut components =
            vec![component(1, 0), component(2, 1), component(3, 2), component(10, 11), component(11, 10), component(12, 12), component(20, 99)];
        components.extend((100..5100).map(|id| component(id, if id == 100 { 0 } else { id - 1 })));
        let chains = chains(&components);
        assert_eq!(chains[&3], [1, 2, 3]);
        assert!(chains[&10].contains(&10) && chains[&11].contains(&11), "a loop ends the chain");
        assert_eq!(chains[&12], [12], "a component that is its own parent");
        assert_eq!(chains[&20], [99, 20], "a missing parent is the outermost");
        assert_eq!(chains[&5099], (100..100 + FOLD_DEPTH_MAX as u64).collect::<Vec<_>>(), "only the outermost levels are kept");
    }

    /// A folder is one chip whatever the history's length; open, it shows its features, its
    /// folders folded in turn and its joints. A short history folds nothing else.
    #[test]
    fn folders_fold_on_their_own_and_open_level_by_level() {
        let chains: Vec<&[u64]> = vec![&[]; 10];
        let folders = [span(100, 2, 8, 3, vec![span(101, 4, 6, 0, vec![])]), span(102, 10, 10, 0, vec![])];
        let closed = entries(10, &chains, &folders, &mut Open(&[]));
        let mut expected = vec![Entry::Feature(0), Entry::Feature(1), folder(100, 2, 8, 0, false)];
        expected.extend([8, 9].map(Entry::Feature));
        expected.push(folder(102, 10, 10, 0, false));
        assert_eq!(closed, expected);

        let open = entries(10, &chains, &folders, &mut Open(&[100]));
        let mut expected = vec![Entry::Feature(0), Entry::Feature(1), folder(100, 2, 8, 0, true), Entry::Feature(2), Entry::Feature(3)];
        expected.extend([folder(101, 4, 6, 1, false), Entry::Feature(6), Entry::Feature(7), Entry::Joints(100), Entry::End(0)]);
        expected.extend([8, 9].map(Entry::Feature));
        expected.push(folder(102, 10, 10, 0, false));
        assert_eq!(open, expected);
    }

    /// In a long history the component fold applies outside folders and inside an open one, but
    /// a run never reaches across a folder's edge; the runs inside count the folder's depth.
    #[test]
    fn the_component_fold_runs_outside_folders_and_inside_open_ones() {
        let chains: Vec<&[u64]> = vec![&[1u64][..]; FOLD_FROM + 20];
        let n = chains.len();
        let folders = [span(100, 10, 30, 0, vec![])];
        let closed = entries(n, &chains, &folders, &mut Open(&[]));
        assert_eq!(closed, vec![group(1, 0, 10, 0, false), folder(100, 10, 30, 0, false), group(1, 30, n, 0, false)]);
        let open = entries(n, &chains, &folders, &mut Open(&[100]));
        assert_eq!(
            open,
            vec![group(1, 0, 10, 0, false), folder(100, 10, 30, 0, true), group(1, 10, 30, 1, false), Entry::End(0), group(1, 30, n, 0, false)]
        );
    }

    /// Folders that overlap (a damaged design) still show every feature once, in order.
    #[test]
    fn overlapping_folders_keep_every_feature_once_in_order() {
        let chains: Vec<&[u64]> = vec![&[]; 20];
        let folders = [span(1, 2, 12, 0, vec![span(3, 0, 30, 0, vec![])]), span(2, 5, 15, 0, vec![]), span(4, 40, 50, 0, vec![])];
        let shown = entries(20, &chains, &folders, &mut Open(&[1, 2, 3, 4]));
        let features: Vec<usize> = shown.iter().filter_map(|e| if let Entry::Feature(i) = e { Some(*i) } else { None }).collect();
        assert_eq!(features, (0..20).collect::<Vec<_>>());
    }
}
