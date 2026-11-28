//! Tab groups in the window: what the tab strip draws for them (labels, menus, a grouped tab's
//! menu rows), what its clicks do, and the `tab.group.*` commands. The groups themselves are in
//! [`crate::tab_groups`], kept by tab number in `documents::Documents`.

use serde_json::{Value, json};

use crate::SolveApp;
use crate::app_bar::{GroupLabel, TabAction, TabMenuRow};
use crate::documents;
use crate::tab_groups::{GroupColour, TabGroups, group_after_drop, place_with_group};

/// The menu rows of a tab group and of a grouped tab.
const RENAME: &str = "group.rename";
const UNGROUP: &str = "group.ungroup";
const NEW: &str = "group.new";
const REMOVE: &str = "group.remove";
/// "group.add." and the group's id: add the tab to that group.
const ADD: &str = "group.add.";

/// A group's name being typed in its label: the group, the text and whether the field has taken
/// the focus.
pub(crate) struct GroupRename {
    group: u64,
    text: String,
    focused: bool,
}

/// The menu row that gives a group `colour`.
fn colour_row(colour: GroupColour) -> &'static str {
    match colour {
        GroupColour::Blue => "group.colour.blue",
        GroupColour::Green => "group.colour.green",
        GroupColour::Orange => "group.colour.orange",
        GroupColour::Yellow => "group.colour.yellow",
        GroupColour::Purple => "group.colour.purple",
        GroupColour::Cyan => "group.colour.cyan",
        GroupColour::Pink => "group.colour.pink",
    }
}

/// A group's colour as the theme draws `colour`: the label's tint and dot, and the line under its
/// tabs. The name itself is in the bar's text colour, as the views' are.
pub fn fill(colour: GroupColour) -> egui::Color32 {
    let t = solidcraft_theme::Tokens::get();
    solidcraft_theme::TabAccent::for_ramp(&t, colour.ramp()).color
}

/// Each tab's group, in strip order.
fn marks(app: &mut SolveApp) -> (Vec<u64>, Vec<Option<u64>>) {
    let numbers = app.docs.tab_numbers();
    let marks = numbers.iter().map(|number| app.docs.groups.group_of(*number)).collect();
    (numbers, marks)
}

/// Forget closed tabs and empty groups, and keep each group's tabs together: a tab in no group
/// that sits between two tabs of one (a tab opened next to a grouped one) joins it.
pub(crate) fn tidy(app: &mut SolveApp) {
    let numbers = app.docs.tab_numbers();
    app.docs.groups.prune(&numbers);
    if app.docs.groups.is_empty() {
        return;
    }
    let (numbers, mut marks) = marks(app);
    for k in 0..marks.len() {
        if marks[k].is_none() {
            let joined = group_after_drop(&marks, k);
            if joined.is_some() {
                app.docs.groups.join(numbers[k], joined);
                marks[k] = joined;
            }
        }
    }
}

/// A group saved at the last quit, waiting in this launch: each of its designs joins it as it opens.
/// `id` is the group made for it, once its first design is open.
#[derive(Clone, Debug)]
pub(crate) struct PendingGroup {
    saved: crate::tab_groups::RememberedGroup,
    id: Option<u64>,
}

/// The groups to remember at quit: each group with designs open (its name, colour, folded state
/// and the paths of its designs, in tab order), then the saved groups whose designs have not
/// opened yet, so they are not lost by a launch that did not open them.
pub(crate) fn remembered(app: &mut SolveApp) -> Vec<crate::tab_groups::RememberedGroup> {
    let numbers = app.docs.tab_numbers();
    let mut entries: Vec<(Option<u64>, crate::tab_groups::RememberedGroup)> = Vec::new();
    for group in app.docs.groups.groups() {
        let designs: Vec<String> = (0..numbers.len())
            .filter(|&k| app.docs.groups.group_of(numbers[k]) == Some(group.id))
            .filter_map(|k| documents::design_session(app, k)?.path.clone())
            .collect();
        if !designs.is_empty() {
            let saved = crate::tab_groups::RememberedGroup {
                name: group.name.clone(),
                colour: group.colour.key().to_owned(),
                collapsed: group.collapsed,
                designs,
            };
            entries.push((Some(group.id), saved));
        }
    }
    for pending in app.docs.pending_groups.iter().flatten() {
        match pending.id.and_then(|id| entries.iter_mut().find(|(made, _)| *made == Some(id))) {
            Some((_, saved)) => saved.designs.extend(pending.saved.designs.iter().cloned()),
            None if !pending.saved.designs.is_empty() => entries.push((None, pending.saved.clone())),
            None => {}
        }
    }
    entries.into_iter().map(|(_, saved)| saved).take(crate::tab_groups::MAX_GROUPS).collect()
}

/// A design opened from `path`: when this launch brings groups back, it joins the saved group it
/// was in. The first of a group's designs to open makes the group (named, coloured and folded as
/// it was); the others join it next to it.
pub(crate) fn design_opened(app: &mut SolveApp, path: &str) {
    if app.docs.pending_groups.is_none() {
        let pending = app
            .preferences
            .open_design_groups
            .iter()
            .take(crate::tab_groups::MAX_GROUPS)
            .map(|saved| {
                let mut saved = saved.clone();
                saved.designs.truncate(documents::MAX_OPEN);
                PendingGroup { saved, id: None }
            })
            .collect();
        app.docs.pending_groups = Some(pending);
    }
    let Some(mut pending) = app.docs.pending_groups.take() else { return };
    for entry in pending.iter_mut() {
        let Some(at) = entry.saved.designs.iter().position(|p| documents::same_path(p, path)) else { continue };
        entry.saved.designs.remove(at);
        let Some(tab) = documents::index_of_path(app, path) else { continue };
        match entry.id.filter(|id| app.docs.groups.get(*id).is_some()) {
            Some(id) => {
                add(app, id, tab);
            }
            None => {
                if let Some(id) = create(app, tab, &entry.saved.name, GroupColour::from_key(&entry.saved.colour)) {
                    app.docs.groups.set_collapsed(id, entry.saved.collapsed);
                    entry.id = Some(id);
                }
            }
        }
    }
    pending.retain(|entry| !entry.saved.designs.is_empty());
    app.docs.pending_groups = Some(pending);
}

/// Put the design in tab `tab` in a new group named `name` (in `colour`, or the next one). Its id.
pub fn create(app: &mut SolveApp, tab: usize, name: &str, colour: Option<GroupColour>) -> Option<u64> {
    let number = *app.docs.tab_numbers().get(tab)?;
    let (_, marks) = marks(app);
    let old = marks.get(tab).copied().flatten();
    let id = app.docs.groups.create(number, name, colour)?;
    // Out of the middle of its old group, so that one stays together.
    leave_run(app, tab, old, &marks);
    Some(id)
}

/// Put the design in tab `tab` in group `group`, next to its other designs. False when there is
/// no such tab or group.
pub fn add(app: &mut SolveApp, group: u64, tab: usize) -> bool {
    let (numbers, marks) = marks(app);
    let Some(&number) = numbers.get(tab) else { return false };
    let old = marks.get(tab).copied().flatten();
    if !app.docs.groups.join(number, Some(group)) {
        return false;
    }
    if old != Some(group) {
        let (_, after) = self::marks(app);
        let to = place_with_group(&after, tab);
        if to != tab {
            documents::move_tab(app, tab, to);
        } else {
            leave_run(app, tab, old, &marks);
        }
    }
    true
}

/// Take the design in tab `tab` out of its group, past the group's other designs. The group it
/// left, if any.
pub fn remove(app: &mut SolveApp, tab: usize) -> Option<u64> {
    let (numbers, marks) = marks(app);
    let number = *numbers.get(tab)?;
    let group = app.docs.groups.group_of(number)?;
    app.docs.groups.join(number, None);
    leave_run(app, tab, Some(group), &marks);
    Some(group)
}

/// Move the tab at `tab`, which was in `group` (`marks`: every tab's group before it left), out of
/// the middle of that group's tabs to just past them, so they stay together.
fn leave_run(app: &mut SolveApp, tab: usize, group: Option<u64>, marks: &[Option<u64>]) {
    let Some(group) = group else { return };
    let run: Vec<usize> = (0..marks.len()).filter(|&k| marks[k] == Some(group)).collect();
    if let (Some(&first), Some(&last)) = (run.first(), run.last())
        && tab != first
        && tab != last
    {
        documents::move_tab(app, tab, last);
    }
}

/// A tab dragged to place `at`: it joins the group of the tabs either side of it, keeps its own
/// while it is still beside one of them, or leaves it.
pub(crate) fn dropped(app: &mut SolveApp, at: usize) {
    if app.docs.groups.is_empty() {
        return;
    }
    let (numbers, marks) = marks(app);
    if let Some(&number) = numbers.get(at) {
        app.docs.groups.join(number, group_after_drop(&marks, at));
    }
}

/// Start typing a new name for `group` in its label.
fn start_rename(app: &mut SolveApp, group: u64) {
    if let Some(name) = app.docs.groups.get(group).map(|g| g.name.clone()) {
        app.docs.group_rename = Some(GroupRename { group, text: name, focused: false });
    }
}

/// The group of the design on screen, if it is in one and the strip shows a design (not the start
/// page). That group always shows its designs.
fn shown_group(app: &mut SolveApp) -> Option<u64> {
    if app.home.open {
        return None;
    }
    let tab = *app.docs.tab_numbers().get(app.docs.active)?;
    app.docs.groups.group_of(tab)
}

/// Whether `group`'s designs are folded away this frame: it is folded, and it is not the group of
/// the design on screen.
fn folded(group: &crate::tab_groups::TabGroup, shown: Option<u64>) -> bool {
    group.collapsed && shown != Some(group.id)
}

/// What the strip shows of the groups this frame, in its order: owned here so the strip can
/// borrow it.
pub(crate) struct StripGroups {
    /// Every group: its id, name, hover text, fill and colour.
    groups: Vec<(u64, String, String, egui::Color32, GroupColour)>,
    /// Each group's designs are folded away this frame (see [`folded`]).
    collapsed: Vec<bool>,
    /// Each tab's group: a place in `groups`.
    pub(crate) of_tab: Vec<Option<usize>>,
    /// Each tab is folded away this frame: it is not drawn.
    pub(crate) hidden: Vec<bool>,
    /// Each tab's own menu rows: (id, label).
    tab_rows: Vec<Vec<(String, String)>>,
    rename: String,
    colour: String,
    ungroup: String,
    colour_names: Vec<String>,
}

impl StripGroups {
    pub(crate) fn build(app: &mut SolveApp) -> Self {
        tidy(app);
        let (_, marks) = marks(app);
        let shown = shown_group(app);
        let tr = |key: &str| solidcraft_ui_text::tr(key).to_string();
        let colour_names: Vec<String> = GroupColour::ALL.iter().map(|c| tr(c.message())).collect();
        let named = |group: &crate::tab_groups::TabGroup| {
            if group.name.is_empty() { tr(group.colour.message()) } else { group.name.clone() }
        };
        let groups: Vec<_> = app
            .docs
            .groups
            .groups()
            .iter()
            .map(|group| {
                let tip = solidcraft_ui_text::format("tab_group.label", &[("name", &named(group))]);
                (group.id, group.name.clone(), tip, fill(group.colour), group.colour)
            })
            .collect();
        let collapsed: Vec<bool> = app.docs.groups.groups().iter().map(|group| folded(group, shown)).collect();
        let of_tab: Vec<Option<usize>> = marks.iter().map(|mark| mark.and_then(|id| groups.iter().position(|g| g.0 == id))).collect();
        let hidden: Vec<bool> =
            marks.iter().map(|mark| mark.and_then(|id| app.docs.groups.get(id)).is_some_and(|group| folded(group, shown))).collect();
        let tab_rows = marks
            .iter()
            .map(|mark| {
                let mut rows = vec![(NEW.to_owned(), tr("tab_group.new"))];
                for group in app.docs.groups.groups().iter().filter(|g| Some(g.id) != *mark) {
                    let label = solidcraft_ui_text::format("tab_group.add_to", &[("name", &named(group))]);
                    rows.push((format!("{ADD}{}", group.id), label));
                }
                if mark.is_some() {
                    rows.push((REMOVE.to_owned(), tr("tab_group.remove")));
                }
                rows
            })
            .collect();
        StripGroups {
            groups,
            collapsed,
            of_tab,
            hidden,
            tab_rows,
            rename: tr("tab_group.rename"),
            colour: tr("tab_group.colour"),
            ungroup: tr("tab_group.ungroup"),
            colour_names,
        }
    }

    /// Each group's menu: rename, its colours (the current one checked), ungroup.
    pub(crate) fn group_menus(&self) -> Vec<Vec<TabMenuRow<'_>>> {
        self.groups
            .iter()
            .map(|(_, _, _, _, current)| {
                let mut rows = vec![TabMenuRow::new(RENAME, &self.rename, ""), TabMenuRow::heading(&self.colour).separated()];
                for (colour, name) in GroupColour::ALL.iter().zip(&self.colour_names) {
                    rows.push(TabMenuRow::new(colour_row(*colour), name, "").with_swatch(fill(*colour), colour == current));
                }
                rows.push(TabMenuRow::new(UNGROUP, &self.ungroup, "").separated());
                rows
            })
            .collect()
    }

    /// The labels, with `menus` from [`Self::group_menus`].
    pub(crate) fn labels<'a>(&'a self, menus: &'a [Vec<TabMenuRow<'a>>]) -> Vec<GroupLabel<'a>> {
        self.groups
            .iter()
            .zip(menus)
            .zip(&self.collapsed)
            .map(|(((_, name, tip, fill, _), menu), collapsed)| GroupLabel { name, fill: *fill, tip, menu, collapsed: *collapsed })
            .collect()
    }

    /// Each tab's own rows: new group, the other groups, out of its group.
    pub(crate) fn tab_menus(&self) -> Vec<Vec<TabMenuRow<'_>>> {
        self.tab_rows.iter().map(|rows| rows.iter().map(|(id, label)| TabMenuRow::new(id, label, "")).collect()).collect()
    }

    /// The place among the strip's groups of the group being renamed.
    pub(crate) fn place_of(&self, group: u64) -> Option<usize> {
        self.groups.iter().position(|g| g.0 == group)
    }

    fn id_at(&self, place: usize) -> Option<u64> {
        self.groups.get(place).map(|g| g.0)
    }
}

/// Take the group name being typed, for the strip while it draws ([`finish_rename`] after).
pub(crate) fn take_rename(app: &mut SolveApp) -> Option<GroupRename> {
    app.docs.group_rename.take().filter(|r| app.docs.groups.get(r.group).is_some())
}

/// The strip's field for `rename`: the group's place and the text.
pub(crate) fn rename_field<'a>(groups: &StripGroups, rename: Option<&'a mut GroupRename>) -> Option<(usize, crate::app_bar::TabRename<'a>)> {
    let rename = rename?;
    let place = groups.place_of(rename.group)?;
    Some((place, crate::app_bar::TabRename { text: &mut rename.text, focused: &mut rename.focused, max: crate::tab_groups::MAX_NAME }))
}

/// After the strip drew: keep the name typed (Enter or a click elsewhere), give it up (Escape), or
/// keep typing.
pub(crate) fn finish_rename(app: &mut SolveApp, rename: Option<GroupRename>, actions: &[TabAction]) {
    let Some(rename) = rename else { return };
    if actions.contains(&TabAction::GroupRenameDone) {
        app.docs.groups.rename(rename.group, &rename.text);
    } else if !actions.contains(&TabAction::GroupRenameCancelled) {
        app.docs.group_rename = Some(rename);
    }
}

/// Carry out a strip action about groups. False for any other action.
pub(crate) fn handle(app: &mut SolveApp, groups: &StripGroups, action: &TabAction) -> bool {
    match action {
        TabAction::Menu { tab, id } if id == NEW => {
            if let Some(group) = create(app, *tab, "", None) {
                start_rename(app, group);
            }
        }
        TabAction::Menu { tab, id } if id == REMOVE => {
            remove(app, *tab);
        }
        TabAction::Menu { tab, id } if id.starts_with(ADD) => {
            if let Some(group) = id.strip_prefix(ADD).and_then(|rest| rest.parse::<u64>().ok()) {
                add(app, group, *tab);
            }
        }
        TabAction::GroupMenu { group, id } => {
            let Some(group) = groups.id_at(*group) else { return true };
            match id.as_str() {
                RENAME => start_rename(app, group),
                UNGROUP => {
                    app.docs.groups.ungroup(group);
                }
                _ => {
                    if let Some(colour) = GroupColour::ALL.into_iter().find(|c| colour_row(*c) == id) {
                        app.docs.groups.recolour(group, colour);
                    }
                }
            }
        }
        TabAction::StartGroupRename(place) => {
            if let Some(group) = groups.id_at(*place) {
                start_rename(app, group);
            }
        }
        TabAction::GroupRenameDone | TabAction::GroupRenameCancelled => {}
        TabAction::ToggleGroup(place) => {
            // The state shown is the one turned over: a group folded for the design on screen
            // shows open, and folds once another design is shown.
            if let Some(group) = groups.id_at(*place) {
                let shown = groups.collapsed.get(*place).copied().unwrap_or(false);
                app.docs.groups.set_collapsed(group, !shown);
            }
        }
        _ => return false,
    }
    true
}

/// Carry out a checked `tab.group.*` command (`v`: the engine's echo of its parameters) and return
/// the reply with what was done filled in: the engine holds no tabs.
pub(crate) fn command(app: &mut SolveApp, command: &str, mut v: Value) -> Result<Value, String> {
    let count = app.docs.count();
    let shown = if count > 1 { app.docs.active } else { 0 };
    let tab = match v.get("tab").and_then(Value::as_u64) {
        None => shown,
        Some(tab) => usize::try_from(tab).ok().filter(|t| *t < count).ok_or_else(|| format!("tabs are 0 to {}", count - 1))?,
    };
    let group = v.get("group").and_then(Value::as_u64);
    let colour = v.get("colour").and_then(Value::as_str).and_then(GroupColour::from_key);
    let name = v.get("name").and_then(Value::as_str).map(str::to_owned);
    let known = |app: &SolveApp, group: Option<u64>| group.filter(|g| app.docs.groups.get(*g).is_some()).ok_or("there is no such tab group");
    let done = match command {
        "tab.group.create" => Some(create(app, tab, name.as_deref().unwrap_or(""), colour).ok_or("no more tab groups can be made")?),
        "tab.group.add" => {
            let group = known(app, group)?;
            add(app, group, tab);
            Some(group)
        }
        "tab.group.remove" => Some(remove(app, tab).ok_or("that design is in no tab group")?),
        "tab.group.update" => {
            let group = known(app, group)?;
            if let Some(name) = &name
                && !app.docs.groups.rename(group, name)
            {
                return Err("the name is not acceptable".into());
            }
            if let Some(colour) = colour {
                app.docs.groups.recolour(group, colour);
            }
            if let Some(collapsed) = v.get("collapsed").and_then(Value::as_bool) {
                app.docs.groups.set_collapsed(group, collapsed);
            }
            Some(group)
        }
        "tab.group.ungroup" => {
            let group = known(app, group)?;
            app.docs.groups.ungroup(group);
            Some(group)
        }
        _ => None,
    };
    if let Some(reply) = v.as_object_mut() {
        if let Some(group) = done {
            reply.insert("group".into(), json!(group));
        }
        if command == "tab.group.list" {
            reply.insert("groups".into(), list(app));
        }
    }
    Ok(v)
}

/// Every group with its tabs, as `tab.group.list` answers.
fn list(app: &mut SolveApp) -> Value {
    tidy(app);
    let (_, marks) = marks(app);
    let groups: &TabGroups = &app.docs.groups;
    let rows = groups.groups().iter().map(|group| {
        let tabs: Vec<usize> = (0..marks.len()).filter(|&k| marks[k] == Some(group.id)).collect();
        json!({"id": group.id, "name": group.name, "colour": group.colour.key(), "collapsed": group.collapsed, "tabs": tabs})
    });
    Value::Array(rows.collect())
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::documents;
    use crate::scenario::Harness;
    use crate::tab_groups::GroupColour;

    /// A window with three designs, each with a box, the first shown.
    fn three_designs() -> Harness {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.custom_titlebar = true;
        h.call("ui.resize", json!({"width": 1400, "height": 720}));
        for k in 0..3 {
            if k > 0 {
                documents::open_tab(&mut h.app, solidcraft_engine::Session::default()).unwrap();
            }
            h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        }
        documents::switch(&mut h.app, 0);
        h.frames(3);
        h
    }

    fn call(h: &mut Harness, command: &str, params: Value) -> Value {
        h.call("engine.execute", json!({"command": command, "params": params}))
    }

    fn groups(h: &mut Harness) -> Vec<(String, String, Vec<u64>)> {
        let reply = call(h, "tab.group.list", json!({}));
        let rows = reply["result"]["groups"].as_array().cloned().unwrap_or_default();
        rows.iter()
            .map(|g| {
                let tabs = g["tabs"].as_array().unwrap().iter().map(|t| t.as_u64().unwrap()).collect();
                (g["name"].as_str().unwrap().to_owned(), g["colour"].as_str().unwrap().to_owned(), tabs)
            })
            .collect()
    }

    /// Folding a group hides its designs but keeps its label; the design on screen always shows its
    /// tab, so the group it is in stays open. Its chevron and `tab.group.update` fold and show.
    #[test]
    fn a_folded_group_keeps_its_label_and_hides_its_other_designs() {
        let mut h = three_designs();
        let group = call(&mut h, "tab.group.create", json!({"tab": 0, "name": "Pair"}))["result"]["group"].as_u64().unwrap();
        call(&mut h, "tab.group.add", json!({"group": group, "tab": 1}));
        documents::switch(&mut h.app, 2);
        h.frames(2);
        assert!(crate::scenario::rect_of("tab:design:0").is_some() && crate::scenario::rect_of("tab:design:1").is_some());
        h.step(&json!({"click": {"handle": "tab:group:0:toggle"}})).unwrap();
        h.frames(2);
        assert!(crate::scenario::rect_of("tab:design:0").is_none(), "a folded design is not drawn");
        assert!(crate::scenario::rect_of("tab:design:1").is_none(), "a folded design is not drawn");
        assert!(crate::scenario::rect_of("tab:design:2").is_some(), "the design outside the group is");
        assert!(crate::scenario::rect_of("tab:group:0").is_some(), "the label stays");
        let reply = call(&mut h, "tab.group.list", json!({}));
        assert_eq!(reply["result"]["groups"][0]["collapsed"], json!(true), "the group reports it is folded");
        // Showing a design of the group shows its designs again, and the group stays open while it is shown.
        documents::switch(&mut h.app, 1);
        h.frames(2);
        assert!(crate::scenario::rect_of("tab:design:0").is_some() && crate::scenario::rect_of("tab:design:1").is_some(), "its own group is open");
        documents::switch(&mut h.app, 2);
        h.frames(2);
        assert!(crate::scenario::rect_of("tab:design:0").is_none(), "folded again once another design is shown");
        call(&mut h, "tab.group.update", json!({"group": group, "collapsed": false}));
        h.frames(2);
        assert!(crate::scenario::rect_of("tab:design:0").is_some() && crate::scenario::rect_of("tab:design:1").is_some(), "the command shows them");
    }

    /// Groups saved at quit come back with their designs: each saved design that opens again joins
    /// its group, which is made as it was (named, coloured, folded), and a design that was in none
    /// joins none.
    #[test]
    fn groups_come_back_with_their_designs_after_a_restart() {
        let dir = std::env::temp_dir().join(format!("solidcraft-groups-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths: Vec<String> = (0..3).map(|k| dir.join(format!("{k}.solidcraft")).to_string_lossy().into_owned()).collect();
        let mut h = three_designs();
        for (k, path) in paths.iter().enumerate() {
            documents::switch(&mut h.app, k);
            h.app.session.execute("file.save", &json!({"path": path})).unwrap();
        }
        let group = call(&mut h, "tab.group.create", json!({"tab": 0, "name": "Pair", "colour": "green"}))["result"]["group"].as_u64().unwrap();
        call(&mut h, "tab.group.add", json!({"group": group, "tab": 1}));
        call(&mut h, "tab.group.update", json!({"group": group, "collapsed": true}));
        let saved = super::remembered(&mut h.app);
        let pair = crate::tab_groups::RememberedGroup { name: "Pair".into(), colour: "green".into(), collapsed: true, designs: paths[..2].to_vec() };
        assert_eq!(saved, vec![pair.clone()], "the group and its two designs are remembered");
        // The next launch: the saved designs open again, in the tab order they were saved in, and
        // each joins the group.
        let mut again = Harness::new();
        again.app.home.open = false;
        again.app.preferences.open_design_groups = saved;
        for path in [&paths[0], &paths[1]] {
            again.app.open_path(path);
            super::design_opened(&mut again.app, path);
        }
        again.frames(2);
        assert_eq!(groups(&mut again).len(), 1, "one group comes back");
        assert!(again.app.docs.groups.groups()[0].collapsed, "folded as it was");
        assert_eq!(super::remembered(&mut again.app), vec![pair.clone()], "both designs are in it");
        // A design that was in no group is not put in one.
        again.app.open_path(&paths[2]);
        super::design_opened(&mut again.app, &paths[2]);
        assert_eq!(super::remembered(&mut again.app), vec![pair], "the design in no group stays out");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// From a tab's menu: a new group, named in its label as soon as it is made; another design
    /// added from its menu moves next to it; the label's menu recolours it; a double click renames
    /// it, Escape keeps the old name; Remove and Ungroup leave the designs open.
    #[test]
    fn a_tab_group_is_made_named_coloured_and_taken_apart_from_the_strip() {
        let mut h = three_designs();
        h.step(&json!({"click": {"handle": "tab:design:0"}, "button": "right"})).unwrap();
        h.frames(2);
        h.step(&json!({"click": {"handle": "tab_menu:group.new"}})).unwrap();
        h.frames(2);
        assert!(crate::scenario::rect_of("tab:rename").is_some(), "the new group's name field is open");
        h.step(&json!({"text": "Parts"})).unwrap();
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        assert_eq!(groups(&mut h), [("Parts".into(), "blue".into(), vec![0])]);
        assert!(crate::scenario::rect_of("tab:group:0").is_some(), "the label is drawn");
        let first = h.app.docs.groups.groups()[0].id;
        // The third design joins from its own menu, and moves next to the first.
        h.step(&json!({"click": {"handle": "tab:design:2"}, "button": "right"})).unwrap();
        h.frames(2);
        h.step(&json!({"click": {"handle": format!("tab_menu:group.add.{first}")}})).unwrap();
        h.frames(2);
        assert_eq!(groups(&mut h)[0].2, [0, 1], "the two designs sit together");
        // The label's menu: a colour.
        h.step(&json!({"click": {"handle": "tab:group:0"}, "button": "right"})).unwrap();
        h.frames(2);
        h.step(&json!({"click": {"handle": "tab_menu:group.colour.pink"}})).unwrap();
        h.frames(2);
        assert_eq!(groups(&mut h)[0].1, "pink");
        // A double click on the label renames it; Escape gives the edit up.
        h.step(&json!({"click": {"handle": "tab:group:0"}, "double": true})).unwrap();
        h.frames(2);
        assert!(h.app.docs.group_rename.is_some());
        h.step(&json!({"text": "Discarded"})).unwrap();
        h.step(&json!({"key": "Escape"})).unwrap();
        h.frames(2);
        assert!(h.app.docs.group_rename.is_none());
        assert_eq!(groups(&mut h)[0].0, "Parts");
        // Remove the second from its tab's menu, then ungroup from the label's.
        h.step(&json!({"click": {"handle": "tab:design:1"}, "button": "right"})).unwrap();
        h.frames(2);
        h.step(&json!({"click": {"handle": "tab_menu:group.remove"}})).unwrap();
        h.frames(2);
        assert_eq!(groups(&mut h)[0].2, [0]);
        h.step(&json!({"click": {"handle": "tab:group:0"}, "button": "right"})).unwrap();
        h.frames(2);
        h.step(&json!({"click": {"handle": "tab_menu:group.ungroup"}})).unwrap();
        h.frames(2);
        assert!(groups(&mut h).is_empty());
        assert_eq!(h.app.docs.count(), 3, "every design stays open");
        assert!(crate::scenario::rect_of("tab:group:0").is_none());
    }

    /// The commands: make, add (the design moves next to the group), rename and recolour, remove,
    /// ungroup; a group whose designs all close goes; bad requests are refused.
    #[test]
    fn the_tab_group_commands_make_and_change_groups() {
        let mut h = three_designs();
        let made = call(&mut h, "tab.group.create", json!({"tab": 0, "name": "Jigs", "colour": "cyan"}));
        let jigs = made["result"]["group"].as_u64().unwrap();
        assert_eq!(call(&mut h, "tab.group.add", json!({"group": jigs, "tab": 2}))["ok"], true);
        assert_eq!(groups(&mut h), [("Jigs".into(), "cyan".into(), vec![0, 1])]);
        // A second group takes the next unused colour.
        let other = call(&mut h, "tab.group.create", json!({"tab": 2}))["result"]["group"].as_u64().unwrap();
        assert_eq!(h.app.docs.groups.get(other).map(|g| g.colour), Some(GroupColour::Blue));
        assert_eq!(call(&mut h, "tab.group.update", json!({"group": jigs, "name": " Fixtures ", "colour": "orange"}))["ok"], true);
        assert_eq!(groups(&mut h)[0], ("Fixtures".into(), "orange".into(), vec![0, 1]));
        // The second design leaves; its group (now only the first) keeps it together.
        assert_eq!(call(&mut h, "tab.group.remove", json!({"tab": 0}))["result"]["group"], jigs);
        assert_eq!(groups(&mut h)[0].2.len(), 1);
        // Refused: no such group, no such tab, a design in no group.
        assert_eq!(call(&mut h, "tab.group.add", json!({"group": 999}))["ok"], false);
        assert_eq!(call(&mut h, "tab.group.update", json!({"group": 999, "name": "x"}))["ok"], false);
        assert_eq!(call(&mut h, "tab.group.create", json!({"tab": 7}))["ok"], false);
        let numbers = h.app.docs.tab_numbers();
        let loose = (0..3).find(|k| h.app.docs.groups.group_of(numbers[*k]).is_none()).unwrap();
        assert_eq!(call(&mut h, "tab.group.remove", json!({"tab": loose}))["ok"], false);
        assert_eq!(call(&mut h, "tab.group.ungroup", json!({"group": other}))["ok"], true);
        // Closing the last design of a group takes the group away.
        let fixtures_tab = groups(&mut h)[0].2[0] as usize;
        documents::close(&mut h.app, fixtures_tab, true);
        h.frames(2);
        assert!(groups(&mut h).is_empty());
    }

    /// A design opened next to a grouped one in the middle of its group joins the group, so the
    /// group's tabs stay together.
    #[test]
    fn a_design_opened_inside_a_group_joins_it() {
        let mut h = three_designs();
        let group = call(&mut h, "tab.group.create", json!({"tab": 0}))["result"]["group"].as_u64().unwrap();
        call(&mut h, "tab.group.add", json!({"group": group, "tab": 1}));
        documents::switch(&mut h.app, 0);
        documents::open_tab(&mut h.app, solidcraft_engine::Session::default()).unwrap();
        h.frames(2);
        assert_eq!(groups(&mut h)[0].2, [0, 1, 2], "the new design opened between two of the group");
    }
}
