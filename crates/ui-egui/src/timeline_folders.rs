//! Feature folders in the History bar. Each folder shows as one chip with its name and how many
//! items it holds; clicking it shows what it holds, and clicking again folds it. A folder is
//! folded until it is opened, and an open folder shows the folders in it folded in turn, so they
//! open level by level. Opening or folding a folder here or in the History beside the browser is
//! one choice, kept with the design (`history_folds`): both show it the same way.
//!
//! A folder holds joints too (`timeline.group` takes them): they show after the folder's
//! features, and the joints chip after the history shows the rest. A STEP insert puts its Import
//! feature and the rigid joints that keep its parts where the file put them into a folder of its
//! own, so an inserted assembly shows as one chip.
//!
//! How folders and the automatic component fold combine is in `timeline_fold`.
use std::collections::{HashMap, HashSet};

use serde_json::json;
use solidcraft_engine::doc::{Document, TimelineGroup};

use crate::SolveApp;
use crate::timeline_fold::{Folder, History, Strip};
use crate::ui_kit::HistoryGroupChip;

/// The key that remembers a folder was opened or folded in its design (the History beside the
/// browser uses it too; `history_folds`).
pub(crate) const KEY: &str = "timeline_group:";
/// Folders nested this deep are left out; their features show one by one.
const DEPTH_MAX: usize = 32;

fn key(id: u64) -> String {
    format!("{KEY}{id}")
}

/// Whether folder `id` shows what it holds: as last opened or folded in either History, else
/// `unless_clicked` (the History beside the browser shows folders open, the History bar folded).
pub(crate) fn is_open(app: &SolveApp, id: u64, unless_clicked: bool) -> bool {
    crate::history_folds::is_open(app, &key(id), unless_clicked)
}

/// Open or fold folder `id` in both Histories.
pub(crate) fn set_open(app: &mut SolveApp, id: u64, open: bool) {
    crate::history_folds::set_open(app, &key(id), open);
}

/// A folder's place in the history as the History bar shows it: its features (its own and its
/// folders') by index, its own joints, and the folders in it in order. An empty folder sits where
/// the run it is in ends.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Span {
    pub id: u64,
    pub start: usize,
    pub end: usize,
    pub joints: usize,
    pub children: Vec<Span>,
}

/// What the History bar shows of the design's folders: their places, names and joints.
#[derive(Default)]
pub(crate) struct Folders {
    pub spans: Vec<Span>,
    names: HashMap<u64, String>,
    /// Each folder's own joints, in the design's order.
    pub joints: HashMap<u64, Vec<u64>>,
    /// Every joint some folder shows, which the joints chip after the history leaves out.
    pub held: HashSet<u64>,
    /// Each folder's items: its features and the joints in it and its folders.
    count: HashMap<u64, usize>,
}

impl Folders {
    pub(crate) fn of(doc: &Document) -> Self {
        if doc.timeline_groups.is_empty() {
            return Self::default();
        }
        let ids: Vec<u64> = doc.features.iter().map(|f| f.id).collect();
        let joints = folder_joints(doc);
        let spans = spans(&ids, &doc.timeline_groups, &joints);
        let mut count = HashMap::new();
        for span in &spans {
            count_items(span, &joints, &mut count);
        }
        // Only the folders shown hold their joints; a folder left out leaves them to the chip.
        let held = count.keys().filter_map(|id| joints.get(id)).flatten().copied().collect();
        let names = doc.timeline_groups.iter().map(|g| (g.id, g.name.clone())).collect();
        Self { spans, names, joints, held, count }
    }

    pub(crate) fn name(&self, id: u64) -> &str {
        self.names.get(&id).map_or("Folder", String::as_str)
    }

    /// The deepest folder whose run holds the slot between features `slot - 1` and `slot` (a
    /// slot at a folder's edge is outside it), as a dropped feature joins it.
    pub(crate) fn at_slot(&self, slot: usize) -> Option<u64> {
        let mut level = &self.spans;
        let mut found = None;
        while let Some(span) = level.iter().find(|s| s.start < slot && slot < s.end) {
            found = Some(span.id);
            level = &span.children;
        }
        found
    }
}

/// A folder's items, and those of the folders in it, into `count`. Returns them.
fn count_items(span: &Span, joints: &HashMap<u64, Vec<u64>>, count: &mut HashMap<u64, usize>) -> usize {
    let own = joints.get(&span.id).map_or(0, Vec::len);
    let nested: usize = span.children.iter().map(|c| count_items(c, joints, count) - (c.end - c.start)).sum();
    let total = (span.end - span.start) + own + nested;
    count.insert(span.id, total);
    total
}

/// Each folder's parent and depth, following only parents that are there (a missing parent
/// makes a folder outermost). A loop of parents (a damaged design) or a folder nested
/// `DEPTH_MAX` deep leaves the folder out (`None`): its features show one by one.
fn levels(folders: &[TimelineGroup]) -> HashMap<u64, Option<(Option<u64>, usize)>> {
    let parent: HashMap<u64, Option<u64>> = folders.iter().map(|g| (g.id, g.parent.filter(|p| *p != g.id))).collect();
    let up = |id: u64| parent.get(&id).copied().flatten().filter(|p| parent.contains_key(p));
    folders
        .iter()
        .map(|g| {
            let mut depth = 0;
            let mut at = up(g.id);
            while let Some(p) = at {
                depth += 1;
                if depth >= DEPTH_MAX {
                    break;
                }
                at = up(p);
            }
            (g.id, at.is_none().then_some((up(g.id), depth)))
        })
        .collect()
}

/// The folders' places for features `ids` (in history order), outermost first. Every feature of
/// a folder or of the folders in it counts toward its run, so a damaged design whose folder is
/// not one run still shows each feature once (`timeline_fold::entries` cuts runs that overlap).
pub(crate) fn spans(ids: &[u64], folders: &[TimelineGroup], joints: &HashMap<u64, Vec<u64>>) -> Vec<Span> {
    let position: HashMap<u64, usize> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let levels = levels(folders);
    let kept = |id: &u64| levels.get(id).copied().flatten();
    // Each folder's run, from its own features and then its folders', deepest first.
    let mut run: HashMap<u64, (usize, usize)> = HashMap::new();
    for g in folders.iter().filter(|g| kept(&g.id).is_some()) {
        let mut own = g.features.iter().filter_map(|f| position.get(f).copied()).peekable();
        if own.peek().is_some() {
            let (lo, hi) = own.fold((usize::MAX, 0), |(lo, hi), i| (lo.min(i), hi.max(i + 1)));
            run.insert(g.id, (lo, hi));
        }
    }
    let mut by_depth: Vec<(usize, u64, Option<u64>)> = folders.iter().filter_map(|g| kept(&g.id).map(|(p, d)| (d, g.id, p))).collect();
    by_depth.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    for (_, id, parent) in &by_depth {
        if let (Some(parent), Some((lo, hi))) = (parent, run.get(id).copied()) {
            let entry = run.entry(*parent).or_insert((lo, hi));
            *entry = (entry.0.min(lo), entry.1.max(hi));
        }
    }
    let mut children: HashMap<Option<u64>, Vec<u64>> = HashMap::new();
    for (_, id, parent) in by_depth.iter().rev() {
        children.entry(*parent).or_default().push(*id);
    }
    build(None, ids.len(), &children, &run, joints)
}

fn build(
    parent: Option<u64>,
    end: usize,
    children: &HashMap<Option<u64>, Vec<u64>>,
    run: &HashMap<u64, (usize, usize)>,
    joints: &HashMap<u64, Vec<u64>>,
) -> Vec<Span> {
    let mut out: Vec<Span> = children
        .get(&parent)
        .into_iter()
        .flatten()
        .map(|id| {
            let (start, stop) = run.get(id).copied().unwrap_or((end, end));
            Span { id: *id, start, end: stop, joints: joints.get(id).map_or(0, Vec::len), children: build(Some(*id), stop, children, run, joints) }
        })
        .collect();
    out.sort_by_key(|s| (s.start, s.start == s.end, s.id));
    out
}

/// Each folder's own joints, in the design's joint order; ids no joint has any more are left out.
fn folder_joints(doc: &Document) -> HashMap<u64, Vec<u64>> {
    let folder_of: HashMap<u64, u64> = doc.timeline_groups.iter().flat_map(|g| g.joints.iter().map(move |j| (*j, g.id))).collect();
    let mut out: HashMap<u64, Vec<u64>> = HashMap::new();
    for joint in &doc.assembly.joints {
        if let Some(folder) = folder_of.get(&joint.id) {
            out.entry(*folder).or_default().push(joint.id);
        }
    }
    out
}

/// The innermost folder that holds every one of `features` (directly or in its folders): where
/// a folder made of them goes, so grouping features inside a folder nests the new one there.
pub(crate) fn group_parent(folders: &[TimelineGroup], features: &[u64]) -> Option<u64> {
    let parent: HashMap<u64, Option<u64>> = folders.iter().map(|g| (g.id, g.parent)).collect();
    let chain = |feature: &u64| -> Vec<u64> {
        let mut out = Vec::new();
        let mut at = folders.iter().find(|g| g.features.contains(feature)).map(|g| g.id);
        while let Some(id) = at {
            if out.contains(&id) || out.len() > DEPTH_MAX {
                break;
            }
            out.push(id);
            at = parent.get(&id).copied().flatten();
        }
        out
    };
    let mut chains = features.iter().map(chain);
    let first = chains.next()?;
    let rest: Vec<Vec<u64>> = chains.collect();
    first.into_iter().find(|id| rest.iter().all(|c| c.contains(id)))
}

/// Group Features in a feature's menu: a new folder of the selected features (or of `id` alone
/// when it isn't selected), then its name to type. Shown off, with the reason, when the features
/// can't be grouped.
pub(crate) fn group_button(app: &mut SolveApp, ui: &mut egui::Ui, id: u64, at: egui::Pos2) {
    let features = crate::timeline::menu_targets(app, id);
    let parent = group_parent(&app.session.doc.timeline_groups, &features);
    let refusal = app.session.group_refusal(&features, parent);
    let response = crate::context_menu::button_enabled(ui, "Group Features", "feature_group", refusal.is_none());
    if let Some(why) = refusal {
        response.on_hover_text(why);
        return;
    }
    if response.clicked() {
        group(app, &features, parent, at);
        ui.close();
    }
}

/// A new folder of `features` in `parent`, folded, with its name to type at `at`.
pub(crate) fn group(app: &mut SolveApp, features: &[u64], parent: Option<u64>, at: egui::Pos2) -> Option<u64> {
    let made = app.run("timeline.group", json!({ "features": features, "parent": parent })).ok()?;
    let folder = made.get("group").and_then(serde_json::Value::as_u64)?;
    set_open(app, folder, false);
    crate::context_menu::start_rename(app, crate::context_menu::RenameWhat::TimelineGroup(folder), at);
    Some(folder)
}

/// Draws a folder's chip with its top left at `at`; a click shows what it holds or folds it, and
/// its menu also renames, ungroups or selects it, or rolls the marker after it. Returns its width.
pub(crate) fn folder_chip(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    p: &egui::Painter,
    at: egui::Pos2,
    folder: &Folder,
    history: &History,
    strip: &Strip,
) -> f32 {
    let name = history.folders.name(folder.id).to_string();
    // While the bar shows one component's features, a folder counts those and its own joints.
    let own_joints = history.folders.joints.get(&folder.id).map_or(0, Vec::len);
    let count = match history.within {
        Some(_) => folder.end - folder.start + own_joints,
        None => history.folders.count.get(&folder.id).copied().unwrap_or(folder.end - folder.start),
    };
    let joints = count.saturating_sub(folder.end - folder.start);
    let state = history.inside(folder.start, folder.end, folder.open);
    let picked = app.tree.picked_timeline_groups.contains(&folder.id)
        || (!folder.open && history.folders.joints.get(&folder.id).is_some_and(|j| j.iter().any(|j| app.tree.picked_joints.contains(j))));
    let chip = HistoryGroupChip {
        icon: "feature_group",
        name: &name,
        count,
        open: folder.open,
        selected: state.selected || picked,
        hovered: state.hovered,
        dim: history.view.index(folder.start) >= strip.marker && folder.start < folder.end,
        error: state.error,
        warning: state.warning,
        accent: strip.accent,
    };
    let rect = egui::Rect::from_min_size(at, chip.size(p));
    let response = ui.interact(rect, ui.id().with(("feature_folder", folder.id)), egui::Sense::click());
    crate::scenario::publish_target(&format!("timeline-folder-chip:{name}"), rect);
    chip.paint(p, rect, response.hover_pos());
    let features = folder.end - folder.start;
    let what = match joints {
        0 => counted(features, "feature"),
        _ => format!("{} and {}", counted(features, "feature"), counted(joints, "joint")),
    };
    let tip = if folder.open { "click to fold them" } else { "click to show each one" };
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(format!("{name}: {what}, {tip}"));
    if response.clicked() {
        set_open(app, folder.id, !folder.open);
    }
    crate::ui_kit::Menu::context(&response, |ui| folder_menu(app, ui, folder, &history.view, rect.left_bottom()));
    rect.width()
}

/// `n` things: "1 feature", "3 features".
fn counted(n: usize, thing: &str) -> String {
    if n == 1 { format!("1 {thing}") } else { format!("{n} {thing}s") }
}

fn folder_menu(app: &mut SolveApp, ui: &mut egui::Ui, folder: &Folder, view: &crate::timeline_fold::View, at: egui::Pos2) {
    let fold = if folder.open { "Fold Features" } else { "Show Each Feature" };
    if crate::context_menu::button(ui, fold, "list").clicked() {
        set_open(app, folder.id, !folder.open);
        ui.close();
    }
    if crate::context_menu::button(ui, "Rename", "rename").clicked() {
        crate::context_menu::start_rename(app, crate::context_menu::RenameWhat::TimelineGroup(folder.id), at);
        ui.close();
    }
    if crate::context_menu::button(ui, "Ungroup", "feature_group").on_hover_text("Keep its features and folders in the history").clicked() {
        app.tree.picked_timeline_groups.retain(|g| *g != folder.id);
        let _ = app.run("timeline.ungroup", json!({ "group": folder.id }));
        ui.close();
    }
    ui.separator();
    if crate::context_menu::button(ui, "Select Features", "select").clicked() {
        let features = &app.session.doc.features;
        let items: Vec<_> = view
            .indices(folder.start, folder.end)
            .into_iter()
            .filter_map(|i| features.get(i))
            .map(|f| json!({"type": "feature", "id": f.id}))
            .collect();
        app.tree.picked_joints.clear();
        app.tree.picked_timeline_groups.clear();
        let _ = app.run("select.set", json!({ "items": items }));
        ui.close();
    }
    if crate::context_menu::button(ui, "Roll History Marker Here", "rollback").clicked() {
        let end = view.index(folder.end);
        let position = if end >= app.session.doc.features.len() { json!({}) } else { json!({ "position": end }) };
        let _ = app.run_heavy("timeline.roll_to", position);
        ui.close();
    }
}

/// Where a feature dropped at `slot` goes: `timeline.reorder` while it stays in its folder (or
/// out of every folder), else `timeline.move_many` into the folder the slot is in (or out of
/// folders), so a feature dropped between an open folder's features joins it.
/// `active`: the drop lands before the history marker, which sits at `marker_slot`; crossing it
/// rolls the feature back or forward in the same step.
pub(crate) fn drop_feature(app: &mut SolveApp, folders: &Folders, id: u64, slot: usize, active: bool, marker_slot: usize) {
    let target = folders.at_slot(slot);
    let from = app.session.doc.feature_index(id).unwrap_or(0);
    let home = app.session.doc.timeline_groups.iter().find(|g| g.features.contains(&id)).map(|g| g.id);
    if home == target {
        // Dropping after itself means the slot shifts by one.
        let to = if slot > from { slot - 1 } else { slot };
        if to != from || active != (from < marker_slot) {
            let _ = app.run_heavy("timeline.reorder", json!({ "feature": id, "position": to, "active": active }));
        }
    } else {
        let _ = app.run_heavy("timeline.move_many", json!({ "features": [id], "group": target, "position": slot, "active": active }));
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use solidcraft_engine::doc::TimelineGroup;

    use super::{Span, group_parent, spans};

    fn folder(id: u64, parent: Option<u64>, features: &[u64]) -> TimelineGroup {
        TimelineGroup { id, name: format!("F{id}"), parent, features: features.to_vec(), joints: Vec::new() }
    }

    fn span(id: u64, start: usize, end: usize, children: Vec<Span>) -> Span {
        Span { id, start, end, joints: 0, children }
    }

    /// A folder's run takes in its folders' features; folders sit in history order, an empty one
    /// where its parent's run ends.
    #[test]
    fn spans_nest_in_history_order() {
        let ids: Vec<u64> = (100..110).collect();
        let folders =
            [folder(1, None, &[102, 103]), folder(2, Some(1), &[104, 105]), folder(3, None, &[107]), folder(4, Some(1), &[]), folder(5, None, &[])];
        let got = spans(&ids, &folders, &HashMap::new());
        assert_eq!(got, vec![span(1, 2, 6, vec![span(2, 4, 6, vec![]), span(4, 6, 6, vec![])]), span(3, 7, 8, vec![]), span(5, 10, 10, vec![])]);
    }

    /// A damaged design's loops of parents, missing parents and very deep nesting end without
    /// hanging, and leave out only the folders they can't place.
    #[test]
    fn spans_survive_loops_missing_parents_and_deep_nesting() {
        let ids: Vec<u64> = (0..200).collect();
        let mut folders = vec![folder(1, Some(2), &[1]), folder(2, Some(1), &[2]), folder(3, Some(99), &[5]), folder(4, Some(4), &[6])];
        folders.extend((10..60).map(|id| folder(id, (id > 10).then(|| id - 1), &[100 + id])));
        let got = spans(&ids, &folders, &HashMap::new());
        let ids: Vec<u64> = got.iter().map(|s| s.id).collect();
        assert!(!ids.contains(&1) && !ids.contains(&2), "a loop of parents is left out: {ids:?}");
        assert!(ids.contains(&3) && ids.contains(&4), "a missing parent or a folder its own parent is outermost: {ids:?}");
        let mut depth = 0;
        let mut at = got.iter().find(|s| s.id == 10);
        while let Some(s) = at {
            depth += 1;
            at = s.children.first();
        }
        assert_eq!(depth, super::DEPTH_MAX, "nesting stops at the deepest allowed level");
    }

    /// An inserted assembly's folder holds its whole rigid group, also the joints inside its
    /// sub-assemblies, whose two ends meet in a component below the import's own; none is left
    /// for the joints chip after the history.
    #[test]
    fn an_import_folder_holds_the_joints_of_its_sub_assemblies() {
        use serde_json::json;
        use solidcraft_engine::Session;
        use solidcraft_engine::doc::joints::JointKind;
        let run = |s: &mut Session, command: &str, params: serde_json::Value| s.execute(command, &params).unwrap();
        // Outer holds a plate and Inner, placed twice: the file nests Inner in Outer.
        let mut s = Session::default();
        run(&mut s, "component.create", json!({"name": "Outer"}));
        run(&mut s, "solid.box", json!({"length": 30, "width": 30, "height": 5, "body_name": "Plate"}));
        run(&mut s, "component.create", json!({"name": "Inner"}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Block"}));
        run(&mut s, "component.activate", json!({"component": "Outer"}));
        run(&mut s, "occurrence.copy", json!({"component": "Inner", "translate": [0, 0, 30]}));
        let dir = std::env::temp_dir().join(format!("solidcraft-timeline-folders-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("nested.step").to_string_lossy().to_string();
        run(&mut s, "file.export", json!({"path": path}));

        let mut s = Session::default();
        let folder = run(&mut s, "file.insert_step", json!({"path": path}))["group"].as_u64().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let mut rigid: Vec<u64> = s.doc.assembly.joints.iter().filter(|j| j.kind == JointKind::Rigid).map(|j| j.id).collect();
        rigid.sort_unstable();
        let mut held = super::Folders::of(&s.doc).joints.get(&folder).cloned().unwrap_or_default();
        held.sort_unstable();
        assert!(rigid.len() >= 3, "the plate and both Inner placements are held: {rigid:?}");
        assert_eq!(held, rigid, "every joint of the import is in its folder");
    }

    /// A new folder goes in the innermost folder holding all the features grouped.
    #[test]
    fn a_new_folder_nests_where_all_its_features_are() {
        let folders = [folder(1, None, &[1, 2]), folder(2, Some(1), &[3, 4]), folder(3, None, &[9])];
        assert_eq!(group_parent(&folders, &[3, 4]), Some(2));
        assert_eq!(group_parent(&folders, &[2, 3]), Some(1));
        assert_eq!(group_parent(&folders, &[4, 9]), None);
        assert_eq!(group_parent(&folders, &[5]), None);
        assert_eq!(group_parent(&folders, &[]), None);
    }
}
