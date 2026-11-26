//! Tab groups: open designs gathered under a named, coloured label in the tab strip. Pure data and
//! small functions, no egui. A group knows its designs by their tab numbers ([`crate::documents`]'s
//! `DocSlot::tab`), which stay the same while a tab moves or another design is shown; the strip
//! keeps each group's designs next to each other. Groups are the window's, never the design's.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use solidcraft_theme::AccentRamp;

/// Longest group name, in characters (what `tab.group.update` takes too).
pub const MAX_NAME: usize = 64;
/// Most groups open at once.
pub const MAX_GROUPS: usize = 64;

/// A group's colour: one of the theme's accent ramps, so the label follows the theme (and colour
/// vision modes) and its text keeps its contrast. Saved and scripted by [`GroupColour::key`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GroupColour {
    #[default]
    Blue,
    Green,
    Orange,
    Yellow,
    Purple,
    Cyan,
    Pink,
}

impl GroupColour {
    /// Every colour, in the order a new group takes them and the menu lists them.
    pub const ALL: [GroupColour; 7] =
        [GroupColour::Blue, GroupColour::Green, GroupColour::Orange, GroupColour::Yellow, GroupColour::Purple, GroupColour::Cyan, GroupColour::Pink];

    /// The colour's stable key: its name in commands.
    pub fn key(self) -> &'static str {
        match self {
            GroupColour::Blue => "blue",
            GroupColour::Green => "green",
            GroupColour::Orange => "orange",
            GroupColour::Yellow => "yellow",
            GroupColour::Purple => "purple",
            GroupColour::Cyan => "cyan",
            GroupColour::Pink => "pink",
        }
    }

    /// The colour with `key` ([`Self::key`]).
    pub fn from_key(key: &str) -> Option<GroupColour> {
        Self::ALL.into_iter().find(|colour| colour.key() == key)
    }

    /// The theme ramp it is drawn with.
    pub fn ramp(self) -> AccentRamp {
        match self {
            GroupColour::Blue => AccentRamp::Main,
            GroupColour::Green => AccentRamp::Sketch,
            GroupColour::Orange => AccentRamp::Surface,
            GroupColour::Yellow => AccentRamp::Mesh,
            GroupColour::Purple => AccentRamp::Form,
            GroupColour::Cyan => AccentRamp::Sheet,
            GroupColour::Pink => AccentRamp::Render,
        }
    }

    /// The message with its name ("Blue"), for the menu.
    pub fn message(self) -> &'static str {
        match self {
            GroupColour::Blue => "tab_group.colour_blue",
            GroupColour::Green => "tab_group.colour_green",
            GroupColour::Orange => "tab_group.colour_orange",
            GroupColour::Yellow => "tab_group.colour_yellow",
            GroupColour::Purple => "tab_group.colour_purple",
            GroupColour::Cyan => "tab_group.colour_cyan",
            GroupColour::Pink => "tab_group.colour_pink",
        }
    }
}

/// One group: its id (unique for as long as the window is open), its name (may be empty: the
/// label is then a coloured dot), its colour, and whether its designs are folded away to the label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabGroup {
    pub id: u64,
    pub name: String,
    pub colour: GroupColour,
    pub collapsed: bool,
}

/// A group as the last quit left it (the `open_design_groups` preference): its name, colour and
/// folded state, and the paths of its designs that were open then, in tab order. The designs open
/// again join it, so a group comes back with its designs after a restart.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RememberedGroup {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub colour: String,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default)]
    pub designs: Vec<String>,
}

/// The window's groups, in the order they were made, and which group each tab is in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TabGroups {
    groups: Vec<TabGroup>,
    /// Tab number to group id.
    members: BTreeMap<u64, u64>,
    next_id: u64,
}

impl TabGroups {
    /// Every group.
    pub fn groups(&self) -> &[TabGroup] {
        &self.groups
    }

    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// The group with `id`.
    pub fn get(&self, id: u64) -> Option<&TabGroup> {
        self.groups.iter().find(|group| group.id == id)
    }

    /// The group the tab numbered `tab` is in.
    pub fn group_of(&self, tab: u64) -> Option<u64> {
        self.members.get(&tab).copied()
    }

    /// The colour a new group takes: the first not in use, else the one used least.
    pub fn next_colour(&self) -> GroupColour {
        let used = |colour: GroupColour| self.groups.iter().filter(|group| group.colour == colour).count();
        GroupColour::ALL.into_iter().min_by_key(|colour| used(*colour)).unwrap_or_default()
    }

    /// Make a group named `name` (see [`valid_name`]) in `colour` (the next one when none) holding
    /// the tab numbered `tab`, taking it out of any group it was in. None when the name is not
    /// acceptable or there are [`MAX_GROUPS`] already.
    pub fn create(&mut self, tab: u64, name: &str, colour: Option<GroupColour>) -> Option<u64> {
        let name = valid_name(name)?.to_owned();
        if self.groups.len() >= MAX_GROUPS {
            return None;
        }
        self.next_id = self.next_id.max(1);
        let id = self.next_id;
        self.next_id += 1;
        let colour = colour.unwrap_or_else(|| self.next_colour());
        self.groups.push(TabGroup { id, name, colour, collapsed: false });
        self.join(tab, Some(id));
        Some(id)
    }

    /// Put the tab numbered `tab` in the group `group`, or in none. A group left with no tab goes.
    /// False when `group` names no group.
    pub fn join(&mut self, tab: u64, group: Option<u64>) -> bool {
        match group {
            Some(id) if self.get(id).is_none() => return false,
            Some(id) => {
                self.members.insert(tab, id);
            }
            None => {
                self.members.remove(&tab);
            }
        }
        self.drop_empty();
        true
    }

    /// Give the group `id` a new name ([`valid_name`]). False when the name or the group is not
    /// acceptable.
    pub fn rename(&mut self, id: u64, name: &str) -> bool {
        let Some(name) = valid_name(name) else { return false };
        match self.groups.iter_mut().find(|group| group.id == id) {
            Some(group) => {
                group.name = name.to_owned();
                true
            }
            None => false,
        }
    }

    /// Give the group `id` a new colour. False when there is no such group.
    pub fn recolour(&mut self, id: u64, colour: GroupColour) -> bool {
        match self.groups.iter_mut().find(|group| group.id == id) {
            Some(group) => {
                group.colour = colour;
                true
            }
            None => false,
        }
    }

    /// Fold the group `id`'s designs away to its label (or show them again). False when there is no
    /// such group.
    pub fn set_collapsed(&mut self, id: u64, collapsed: bool) -> bool {
        match self.groups.iter_mut().find(|group| group.id == id) {
            Some(group) => {
                group.collapsed = collapsed;
                true
            }
            None => false,
        }
    }

    /// Take the group `id` away; its tabs stay open, in no group. False when there is none.
    pub fn ungroup(&mut self, id: u64) -> bool {
        let found = self.get(id).is_some();
        self.members.retain(|_, group| *group != id);
        self.groups.retain(|group| group.id != id);
        found
    }

    /// Forget the tabs that are not in `open` (closed since) and the groups left empty.
    pub fn prune(&mut self, open: &[u64]) {
        self.members.retain(|tab, _| open.contains(tab));
        self.drop_empty();
    }

    fn drop_empty(&mut self) {
        let members = &self.members;
        self.groups.retain(|group| members.values().any(|id| *id == group.id));
    }
}

/// `name` as a group name: trimmed, at most [`MAX_NAME`] characters, no control characters. An
/// empty name is accepted (the label is then a coloured dot).
pub fn valid_name(name: &str) -> Option<&str> {
    let name = name.trim();
    (name.chars().count() <= MAX_NAME && !name.chars().any(char::is_control)).then_some(name)
}

/// The place `from` lands at when a tab in the group of `tabs[from]` (or none) must sit with the
/// rest of its group: right after the group's last other tab, or where it is when it has no other
/// tab or already sits with them. `groups[k]` is the group of the tab at place `k`.
pub fn place_with_group(groups: &[Option<u64>], from: usize) -> usize {
    let Some(Some(group)) = groups.get(from) else { return from };
    let others: Vec<usize> = (0..groups.len()).filter(|&k| k != from && groups[k] == Some(*group)).collect();
    let Some(&last) = others.last() else { return from };
    let first = others[0];
    // Already next to the run (directly before or after, or inside it).
    if (first.saturating_sub(1)..=last + 1).contains(&from) {
        return from;
    }
    if from < first { last } else { last + 1 }
}

/// The group a tab dropped at place `at` (after the move) takes, given `groups` (after the move,
/// with the moved tab still marked as its old group at `at`): the group of both neighbours when
/// they share one, else its own while a neighbour is in it, else none.
pub fn group_after_drop(groups: &[Option<u64>], at: usize) -> Option<u64> {
    let own = groups.get(at).copied().flatten();
    let left = at.checked_sub(1).and_then(|k| groups.get(k)).copied().flatten();
    let right = groups.get(at + 1).copied().flatten();
    match (left, right) {
        (Some(l), Some(r)) if l == r => Some(l),
        _ if own.is_some() && (left == own || right == own) => own,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_have_stable_keys_ramps_and_messages() {
        for colour in GroupColour::ALL {
            assert_eq!(GroupColour::from_key(colour.key()), Some(colour));
            assert!(colour.message().starts_with("tab_group.colour_"));
        }
        assert_eq!(GroupColour::from_key("BLUE"), None);
        assert_eq!(GroupColour::from_key(""), None);
        let ramps: std::collections::HashSet<_> = GroupColour::ALL.iter().map(|c| c.ramp()).collect();
        assert_eq!(ramps.len(), GroupColour::ALL.len(), "each colour its own ramp");
    }

    #[test]
    fn groups_are_made_named_coloured_and_dropped_when_empty() {
        let mut g = TabGroups::default();
        let a = g.create(10, "  Parts  ", None).unwrap();
        assert_eq!(g.get(a).map(|x| (x.name.as_str(), x.colour)), Some(("Parts", GroupColour::Blue)));
        let b = g.create(11, "", None).unwrap();
        assert_eq!(g.get(b).map(|x| x.colour), Some(GroupColour::Green), "the next unused colour");
        assert!(a != b && g.group_of(10) == Some(a) && g.group_of(11) == Some(b));
        assert!(g.join(12, Some(a)));
        assert!(!g.join(12, Some(999)), "no such group");
        assert!(g.rename(a, "Fixtures") && g.get(a).unwrap().name == "Fixtures");
        assert!(!g.rename(a, "a\nb") && !g.rename(a, &"x".repeat(MAX_NAME + 1)) && !g.rename(999, "x"));
        assert!(g.recolour(b, GroupColour::Pink) && !g.recolour(999, GroupColour::Pink));
        // Moving 11 into a leaves b empty: it goes.
        g.join(11, Some(a));
        assert!(g.get(b).is_none());
        // A closed tab is forgotten, and a group with no tab left goes.
        g.prune(&[10, 12]);
        assert_eq!(g.group_of(11), None);
        g.prune(&[]);
        assert!(g.is_empty());
        // Ungroup keeps nothing of the group.
        let c = g.create(1, "C", Some(GroupColour::Cyan)).unwrap();
        g.join(2, Some(c));
        assert!(g.ungroup(c) && !g.ungroup(c));
        assert_eq!((g.group_of(1), g.group_of(2)), (None, None));
    }

    #[test]
    fn hostile_names_and_too_many_groups_are_refused() {
        let mut g = TabGroups::default();
        assert!(g.create(1, "\u{7}", None).is_none());
        assert!(g.create(1, &"é".repeat(MAX_NAME + 1), None).is_none());
        assert!(g.create(1, &"é".repeat(MAX_NAME), None).is_some(), "characters, not bytes");
        for tab in 2..=MAX_GROUPS as u64 {
            assert!(g.create(tab, "", None).is_some());
        }
        assert!(g.create(1000, "", None).is_none(), "at most {MAX_GROUPS}");
        assert_eq!(g.groups().len(), MAX_GROUPS);
    }

    #[test]
    fn a_tab_is_placed_with_its_group() {
        let (a, b) = (Some(1), Some(2));
        // Already with its group, or alone in it: it stays.
        assert_eq!(place_with_group(&[a, a, None], 1), 1);
        assert_eq!(place_with_group(&[a, a, None], 2), 2);
        assert_eq!(place_with_group(&[None, a, a, a], 0), 0);
        assert_eq!(place_with_group(&[a, None, b], 2), 2, "alone in b");
        // Away from it: right after its last other tab.
        assert_eq!(place_with_group(&[a, None, None, a], 3), 1, "to right after the first a");
        assert_eq!(place_with_group(&[a, None, a, a], 0), 3, "from before: lands after the last");
        assert_eq!(place_with_group(&[a, a, None, None, a], 4), 2);
        assert_eq!(place_with_group(&[], 0), 0);
        assert_eq!(place_with_group(&[a], 5), 5);
    }

    #[test]
    fn a_dropped_tab_joins_or_leaves_by_its_neighbours() {
        let (a, b) = (Some(1), Some(2));
        assert_eq!(group_after_drop(&[a, None, a], 1), a, "between two of a: joins a");
        assert_eq!(group_after_drop(&[a, b, a], 1), a, "from b into a");
        assert_eq!(group_after_drop(&[a, a, None], 1), a, "still next to its own");
        assert_eq!(group_after_drop(&[a, a, None], 0), a, "at the run's start, still its own");
        assert_eq!(group_after_drop(&[None, a, a], 0), None, "before a's first tab, not of it");
        assert_eq!(group_after_drop(&[None, a, None], 1), None, "away from the rest of a");
        assert_eq!(group_after_drop(&[a, None, None], 1), None, "beside a, not of it");
        assert_eq!(group_after_drop(&[b, a, None], 1), None, "beside b, not of it");
        assert_eq!(group_after_drop(&[], 3), None);
    }

    /// A group folds away and shows again, and keeps its folded state while designs join it.
    #[test]
    fn a_group_folds_and_shows_again_and_keeps_its_state_as_designs_join() {
        let mut groups = TabGroups::default();
        let id = groups.create(1, "Parts", None).unwrap();
        assert!(!groups.get(id).unwrap().collapsed, "shown open when made");
        assert!(groups.set_collapsed(id, true));
        assert!(groups.get(id).unwrap().collapsed);
        assert!(groups.join(2, Some(id)));
        assert!(groups.get(id).unwrap().collapsed, "joining keeps it folded");
        assert!(groups.set_collapsed(id, false));
        assert!(!groups.get(id).unwrap().collapsed);
        assert!(!groups.set_collapsed(99, true), "no such group");
    }
}
